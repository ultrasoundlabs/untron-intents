// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {IBridger} from "./interfaces/IBridger.sol";
import {TokenUtils} from "../utils/TokenUtils.sol";

import {Ownable} from "solady/auth/Ownable.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";

/// @title ITokenMessengerV2
/// @notice Minimal interface for Circle CCTP V2 `TokenMessengerV2`.
/// @dev Signature matches Circle's `TokenMessengerV2.depositForBurn`.
/// @author Ultrasound Labs
interface ITokenMessengerV2 {
    /// @notice Burns USDC on the source chain and initiates a CCTP V2 message to mint on the destination.
    /// @dev This is a minimal interface for Circle's `TokenMessengerV2.depositForBurn`.
    /// @param amount Amount of `burnToken` to burn.
    /// @param destinationDomain Circle CCTP domain id of the destination chain (not an EVM chainId).
    /// @param mintRecipient Recipient on the destination chain, encoded as bytes32.
    /// @param burnToken Token to burn (USDC).
    /// @param destinationCaller Optional “destination caller” restriction (bytes32(0) for none).
    /// @param maxFee Maximum fee paid for fast transfers (0 for standard).
    /// @param minFinalityThreshold Finality threshold for fast vs standard transfers.
    function depositForBurn(
        uint256 amount,
        uint32 destinationDomain,
        bytes32 mintRecipient,
        address burnToken,
        bytes32 destinationCaller,
        uint256 maxFee,
        uint32 minFinalityThreshold
    ) external;
}

/// @title CCTPV2Bridger
/// @notice Simple, stateless CCTP V2 bridger (USDC-only).
/// @dev Integration notes:
/// - This bridger is intended to be called only by a single authorized forwarder
///   ({AUTHORIZED_CALLER}), not by arbitrary users.
/// - CCTP V2 works by burning USDC on the source chain via Circle’s TokenMessenger and minting
///   USDC on the destination chain for the recipient.
///
/// `extraData` semantics:
/// - This implementation uses `extraData` only to choose fast vs standard transfers.
/// - Because `extraData` is supplied by permissionless relayers, it is parsed defensively and
///   only accepts encodings that can be safely interpreted as a boolean flag.
///
/// Fee model and `expectedAmountOut`:
/// - In fast mode, Circle charges a max fee (here approximated as `ceil(amount / 10_000)` = 1 bps).
/// - {bridge} returns `inputAmount - maxFee` in fast mode and `inputAmount` in standard mode.
/// @author Ultrasound Labs
contract CCTPV2Bridger is IBridger, Ownable {
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
    error InvalidExtraData();

    /*//////////////////////////////////////////////////////////////
                                IMMUTABLES
    //////////////////////////////////////////////////////////////*/

    /// @notice The only caller allowed to initiate a burn (expected to be UntronIntentsForwarder).
    address public immutable AUTHORIZED_CALLER;

    /// @notice Circle TokenMessengerV2 on this chain.
    ITokenMessengerV2 public immutable TOKEN_MESSENGER_V2;

    /// @notice The only supported token (CCTP burns/mints USDC).
    IERC20 public immutable USDC;

    /*//////////////////////////////////////////////////////////////
                                 STORAGE
    //////////////////////////////////////////////////////////////*/

    /// @notice EVM chainId -> Circle CCTP domain id.
    /// @dev Circle domains are NOT EVM chainIds.
    mapping(uint256 => uint32) public circleDomainByChainId;

    /// @notice Whether an EVM chainId is supported by this bridger.
    /// @dev Needed because Circle domain id `0` (Ethereum) is valid.
    mapping(uint256 => bool) public isSupportedChainId;

    uint32 internal constant _FINALITY_FAST = 1000;
    uint32 internal constant _FINALITY_STANDARD = 2000;
    uint256 internal constant _ONE_BPS_DENOMINATOR = 10_000;

    /*//////////////////////////////////////////////////////////////
                               CONSTRUCTOR
    //////////////////////////////////////////////////////////////*/

    /// @notice Initializes the bridger.
    /// @param authorizedCaller The contract allowed to call {bridge}.
    /// @param tokenMessengerV2 Circle TokenMessengerV2 address on the current chain.
    /// @param usdc USDC token address on the current chain.
    /// @param supportedChainIds Supported destination EVM chain ids.
    /// @param circleDomains Circle CCTP domain ids corresponding 1:1 with `supportedChainIds`.
    constructor(
        address authorizedCaller,
        address tokenMessengerV2,
        address usdc,
        uint256[] memory supportedChainIds,
        uint32[] memory circleDomains
    ) {
        if (authorizedCaller == address(0) || tokenMessengerV2 == address(0) || usdc == address(0)) revert ZeroAddress();
        if (supportedChainIds.length != circleDomains.length) {
            revert ArrayLengthMismatch(supportedChainIds.length, circleDomains.length);
        }

        AUTHORIZED_CALLER = authorizedCaller;
        TOKEN_MESSENGER_V2 = ITokenMessengerV2(tokenMessengerV2);
        USDC = IERC20(usdc);

        _initializeOwner(msg.sender);

        for (uint256 i = 0; i < supportedChainIds.length; ++i) {
            uint256 chainId = supportedChainIds[i];
            if (isSupportedChainId[chainId]) revert DuplicateChainId(chainId);
            isSupportedChainId[chainId] = true;
            circleDomainByChainId[chainId] = circleDomains[i];
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
        bytes calldata extraData
    ) external payable returns (uint256 expectedAmountOut) {
        if (msg.sender != AUTHORIZED_CALLER) revert NotAuthorizedCaller();
        if (inputAmount == 0) revert AmountZero();
        if (inputToken != address(USDC)) revert UnsupportedToken(inputToken);
        if (outputAddress == address(0)) revert ZeroOutputAddress();

        // Parse the relayer-controlled flag and map EVM chainId to Circle's destination domain.
        bool useFastTransfer = _parseFastTransferFlag(extraData);
        uint32 destinationDomain = _circleDomainForChainId(outputChainId);

        // Fast transfers use a fee and a lower finality threshold; standard transfers use no fee.
        uint256 maxFee = useFastTransfer ? _ceilDiv(inputAmount, _ONE_BPS_DENOMINATOR) : 0;
        uint32 minFinalityThreshold = useFastTransfer ? _FINALITY_FAST : _FINALITY_STANDARD;

        // Approve Circle's messenger to pull USDC for the burn and initiate the CCTP transfer.
        TokenUtils.approve(inputToken, address(TOKEN_MESSENGER_V2), inputAmount);

        TOKEN_MESSENGER_V2.depositForBurn(
            inputAmount,
            destinationDomain,
            bytes32(uint256(uint160(outputAddress))),
            inputToken,
            bytes32(0),
            maxFee,
            minFinalityThreshold
        );

        return useFastTransfer ? (inputAmount - maxFee) : inputAmount;
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
    /// @dev Included for compatibility with integrations that may send ETH alongside calls.
    receive() external payable {}

    /*//////////////////////////////////////////////////////////////
                               INTERNALS
    //////////////////////////////////////////////////////////////*/

    function _circleDomainForChainId(uint256 chainId) internal view returns (uint32) {
        if (!isSupportedChainId[chainId]) revert UnsupportedChainId(chainId);
        return circleDomainByChainId[chainId];
    }

    function _parseFastTransferFlag(bytes calldata extraData) internal pure returns (bool useFastTransfer) {
        if (extraData.length == 0) return false;
        if (extraData.length == 1) return extraData[0] != 0;
        if (extraData.length == 32) return abi.decode(extraData, (bool));
        revert InvalidExtraData();
    }

    function _ceilDiv(uint256 x, uint256 y) internal pure returns (uint256) {
        return (x + y - 1) / y;
    }
}
