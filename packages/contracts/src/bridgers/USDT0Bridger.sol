// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {IBridger} from "./interfaces/IBridger.sol";
import {TokenUtils} from "../utils/TokenUtils.sol";

import {Ownable} from "solady/auth/Ownable.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {IOFT, SendParam, OFTReceipt} from "@layerzerolabs/oft-evm/contracts/interfaces/IOFT.sol";
import {MessagingFee} from "@layerzerolabs/oapp-evm/contracts/oapp/OAppSender.sol";

/// @title USDT0Bridger
/// @notice Bridger for the USDT0 (USD₮0) protocol core mesh using LayerZero V2 OFT.
/// @dev Integration notes:
/// - This bridger is intended to be called only by a single authorized forwarder
///   ({AUTHORIZED_CALLER}), not by arbitrary users.
/// - LayerZero V2 OFT sends an “omnichain token transfer” message, paying a native gas fee.
///
/// `extraData`:
/// - Intentionally unused. See {IBridger} for why `extraData` is untrusted relayer input.
/// - By ignoring `extraData`, this bridger avoids a common class of relayer-driven parameter manipulation.
///
/// Fee model and `expectedAmountOut`:
/// - The OFT protocol can apply transfer fees and can result in `amountReceivedLD` < `amountLD`.
/// - This bridger calls {IOFT.quoteOFT} first and returns the quoted minimum amount received.
/// @author Ultrasound Labs
contract USDT0Bridger is IBridger, Ownable {
    /*//////////////////////////////////////////////////////////////
                                  ERRORS
    //////////////////////////////////////////////////////////////*/

    error NotAuthorizedCaller();
    error UnsupportedToken(address token);
    error UnsupportedChainId(uint256 chainId);
    error ZeroOutputAddress();
    error AmountZero();
    error ZeroAddress();
    error ArrayLengthMismatch(uint256 a, uint256 b);
    error DuplicateChainId(uint256 chainId);
    error InsufficientNativeValue(uint256 have, uint256 need);

    /*//////////////////////////////////////////////////////////////
                                IMMUTABLES
    //////////////////////////////////////////////////////////////*/

    /// @notice The only caller allowed to initiate bridging (expected to be UntronIntentsForwarder).
    address public immutable AUTHORIZED_CALLER;

    /// @notice The USDT0 token address on the current chain.
    IERC20 public immutable USDT0;

    /// @notice The LayerZero V2 OFT module on this chain for USDT0 (OAdapterUpgradeable / OUpgradeable).
    IOFT public immutable OFT;

    /*//////////////////////////////////////////////////////////////
                                 STORAGE
    //////////////////////////////////////////////////////////////*/

    /// @notice EVM chainId -> LayerZero endpoint ID (eid) for USDT0 core mesh destinations.
    mapping(uint256 => uint32) public eidByChainId;

    /*//////////////////////////////////////////////////////////////
                               CONSTRUCTOR
    //////////////////////////////////////////////////////////////*/

    /// @notice Initializes the bridger.
    /// @param authorizedCaller The contract allowed to call {bridge}.
    /// @param usdt0 USDT0 token address on the current chain.
    /// @param oft The LayerZero V2 OFT module used to send USDT0 on the current chain.
    /// @param supportedChainIds Supported destination EVM chain ids.
    /// @param eids LayerZero endpoint IDs (eid) corresponding 1:1 with `supportedChainIds`.
    constructor(
        address authorizedCaller,
        address usdt0,
        address oft,
        uint256[] memory supportedChainIds,
        uint32[] memory eids
    ) {
        if (authorizedCaller == address(0) || usdt0 == address(0) || oft == address(0)) revert ZeroAddress();
        if (supportedChainIds.length != eids.length) revert ArrayLengthMismatch(supportedChainIds.length, eids.length);

        AUTHORIZED_CALLER = authorizedCaller;
        USDT0 = IERC20(usdt0);
        OFT = IOFT(oft);

        _initializeOwner(msg.sender);

        for (uint256 i = 0; i < supportedChainIds.length; ++i) {
            uint256 chainId = supportedChainIds[i];
            if (eidByChainId[chainId] != 0) revert DuplicateChainId(chainId);
            uint32 eid = eids[i];
            if (eid == 0) revert UnsupportedChainId(chainId);
            eidByChainId[chainId] = eid;
        }
    }

    /*//////////////////////////////////////////////////////////////
                              IBRIDGER
    //////////////////////////////////////////////////////////////*/

    /// @inheritdoc IBridger
    function bridge(
        address inputToken,
        uint256 inputAmount,
        address outputAddress,
        uint256 outputChainId,
        bytes calldata
    ) external payable returns (uint256 expectedAmountOut) {
        if (msg.sender != AUTHORIZED_CALLER) revert NotAuthorizedCaller();
        if (inputAmount == 0) revert AmountZero();
        if (inputToken != address(USDT0)) revert UnsupportedToken(inputToken);
        if (outputAddress == address(0)) revert ZeroOutputAddress();

        uint32 dstEid = eidByChainId[outputChainId];
        if (dstEid == 0) revert UnsupportedChainId(outputChainId);

        // Construct a send param and quote the received amount. `minAmountLD` is set to the
        // expected received amount to enforce slippage/fee bounds at send time.
        SendParam memory sp = SendParam({
            dstEid: dstEid,
            to: bytes32(uint256(uint160(outputAddress))),
            amountLD: inputAmount,
            minAmountLD: inputAmount,
            extraOptions: "",
            composeMsg: "",
            oftCmd: ""
        });

        (,, OFTReceipt memory oftReceipt) = OFT.quoteOFT(sp);
        sp.minAmountLD = oftReceipt.amountReceivedLD;

        // Approve and compute the required native fee for the LayerZero message.
        TokenUtils.approve(inputToken, address(OFT), inputAmount);

        MessagingFee memory msgFee = OFT.quoteSend(sp, false);
        if (msg.value < msgFee.nativeFee) revert InsufficientNativeValue(msg.value, msgFee.nativeFee);

        // Send the OFT message, paying exactly the quoted fee and refunding any excess msg.value.
        // solhint-disable-next-line check-send-result
        OFT.send{value: msgFee.nativeFee}(sp, msgFee, msg.sender);

        uint256 refund = msg.value - msgFee.nativeFee;
        if (refund != 0) TokenUtils.transfer(address(0), payable(msg.sender), refund);

        return sp.minAmountLD;
    }

    /*//////////////////////////////////////////////////////////////
                              OWNER HELPERS
    //////////////////////////////////////////////////////////////*/

    /// @notice Withdraws tokens accidentally left in this contract.
    /// @dev Owner-only escape hatch.
    /// @param token Token to withdraw (address(0) = native ETH).
    /// @param amount Amount to withdraw.
    function withdraw(address token, uint256 amount) external onlyOwner {
        if (amount != 0) TokenUtils.transfer(token, payable(msg.sender), amount);
    }

    /// @notice Accepts native token (e.g. ETH) deposits.
    /// @dev This contract may temporarily custody ETH used to pay LayerZero message fees.
    receive() external payable {}
}
