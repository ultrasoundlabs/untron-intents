// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {IBridger} from "./bridgers/interfaces/IBridger.sol";
import {IQuoter} from "./quoters/interfaces/IQuoter.sol";
import {TokenUtils} from "./utils/TokenUtils.sol";
import {UntronReceiver} from "./UntronReceiver.sol";
import {SwapExecutor, Call} from "./SwapExecutor.sol";

import {IntentsForwarderIndexedOwnable} from "./auth/IntentsForwarderIndexedOwnable.sol";

/// @title Intents Forwarder
/// @notice Cross-chain “sweep + optional swap + bridge” router for Untron Intents.
/// @dev High-level design:
/// - Users (or integrators) send funds to deterministic “receiver” addresses. These receiver
///   contracts are minimal proxies deployed via CREATE2 and owned by this forwarder.
/// - Anyone (a permissionless relayer) can then call {pullFromReceiver} to:
///   1) pull tokens/ETH from the receiver into this forwarder,
///   2) optionally swap `tokenIn` -> `tokenOut` via a protocol-owned {SwapExecutor},
///   3) either (a) pay out locally to `beneficiary`, or (b) bridge to `targetChain` via
///      whitelisted {IBridger} implementations.
///
/// Deterministic addressing across chains:
/// - Receiver addresses are derived from (this contract address, salt, receiver initcode hash).
/// - IntentsForwarder is expected to be deployed at the SAME address on all supported EVM
///   chains (typically via CREATE3 in deployment tooling). With the same forwarder address
///   and receiver initcode hash, the same receiver salt yields the same receiver address
///   on every chain.
///
/// Bridging to receiver addresses:
/// - When bridging cross-chain, the bridge destination is an “ephemeral receiver” address.
/// - On the destination chain (e.g. the hub), the forwarder at the same address can deploy
///   the corresponding receiver contract (if it does not exist yet) and pull the bridged
///   funds out, continuing intent execution.
/// This contract must issue no events. All events must be emitted by the index contract.
/// @author Ultrasound Labs
contract IntentsForwarder is IntentsForwarderIndexedOwnable {
    struct ForwardState {
        bytes32 forwardId;
        bytes32 bridgeDataHash;
    }

    struct ForwardOutcome {
        uint256 amountPulled;
        uint256 amountForwarded;
        uint256 relayerRebate;
        uint256 msgValueRefunded;
        bool settledLocally;
        address bridger;
        uint256 expectedBridgeOut;
    }

    struct PullRequest {
        uint256 targetChain;
        address payable beneficiary;
        bool beneficiaryClaimOnly;
        bytes32 intentHash;
        bytes32 forwardSalt;
        uint256 balance;
        address tokenIn;
        address tokenOut;
        Call[] swapData;
        bytes bridgeData;
    }

    /*//////////////////////////////////////////////////////////////
                                 IMMUTABLES
    //////////////////////////////////////////////////////////////*/

    /// @notice Protocol-owned executor that performs the actual swap call sequence.
    /// @dev Deployed once by this forwarder; only this forwarder can call {SwapExecutor.execute}.
    SwapExecutor public immutable SWAP_EXECUTOR;

    /// @notice Hash of the initcode used to deploy receiver proxies via CREATE2.
    /// @dev Used in {predictReceiverAddress}. It is computed from the minimal proxy creation
    ///      bytecode plus {RECEIVER_IMPLEMENTATION}.
    bytes32 public immutable RECEIVER_BYTECODE_HASH;

    /// @notice Address of the receiver implementation contract used by the minimal proxy receivers.
    /// @dev Each deployed receiver is an EIP-1167 proxy that delegates to this implementation.
    address public immutable RECEIVER_IMPLEMENTATION;

    /// @notice Canonical USDT-like output token supported for bridging by this forwarder on this chain.
    address public immutable USDT;

    /// @notice Canonical USDC output token supported for bridging by this forwarder on this chain.
    address public immutable USDC;

    /*//////////////////////////////////////////////////////////////
                                  STORAGE
    //////////////////////////////////////////////////////////////*/

    /// @notice Bridger used when `tokenOut == USDT`.
    IBridger public usdtBridger;

    /// @notice Bridger used when `tokenOut == USDC`.
    IBridger public usdcBridger;

    /// @notice Maps an input token to the quoter used to compute the minimum expected output for swaps.
    /// @dev Keyed by `tokenIn` (not `tokenOut`), because the quote depends on the input asset.
    mapping(address => IQuoter) public quoterByToken;

    /*//////////////////////////////////////////////////////////////
                                   ERRORS
    //////////////////////////////////////////////////////////////*/

    /// @notice Reverts when `beneficiaryClaimOnly` is enabled and the caller is not the beneficiary.
    error PullerUnauthorized();

    /// @notice Reverts when attempting to bridge a token other than the configured USDT/USDC outputs.
    error UnsupportedOutputToken();

    /// @notice Reverts when the bridger reports an amount-out that does not match the expected forwarded `balance`.
    error InsufficientOutputAmount();

    /// @notice Reverts when attempting to perform a swap while pulling from an ephemeral receiver.
    error SwapOnEphemeralReceiversNotAllowed();

    /*//////////////////////////////////////////////////////////////
                               CONSTRUCTOR
    //////////////////////////////////////////////////////////////*/

    /// @notice Initializes the contract with the given parameters.
    /// @param _usdt Address of the USDT output token on this chain.
    /// @param _usdc Address of the USDC output token on this chain.
    /// @param _owner Initial owner for admin functions (bridger/quoter configuration).
    constructor(address _usdt, address _usdc, address _owner) {
        _initializeOwner(_owner);

        // Deploy the receiver implementation once; receivers will be EIP-1167 proxies to it.
        RECEIVER_IMPLEMENTATION = address(new UntronReceiver());

        // Compute the CREATE2 initcode hash for the minimal proxy that points at RECEIVER_IMPLEMENTATION.
        // This enables counterfactual address prediction for receiver deployments.
        RECEIVER_BYTECODE_HASH = keccak256(
            abi.encodePacked(
                hex"3d602d80600a3d3981f3363d3d373d3d3d363d73",
                RECEIVER_IMPLEMENTATION,
                hex"5af43d82803e903d91602b57fd5bf3"
            )
        );

        // Deploy the swap executor once; only this forwarder can call it.
        SWAP_EXECUTOR = new SwapExecutor();
        USDT = _usdt;
        USDC = _usdc;
    }

    /// @notice Accepts native token (e.g. ETH) deposits.
    /// @dev Enables bridger implementations to refund unused native fees back to this contract.
    receive() external payable {}

    // Admin functions

    /// @notice Sets the bridger contracts used for USDT and USDC forwarding.
    /// @dev These bridgers are expected to restrict access so that only this forwarder can call them.
    /// @param _usdtBridger Bridger to use when bridging USDT.
    /// @param _usdcBridger Bridger to use when bridging USDC.
    function setBridgers(IBridger _usdtBridger, IBridger _usdcBridger) external onlyOwner {
        usdtBridger = _usdtBridger;
        usdcBridger = _usdcBridger;
        _emitBridgersSet(address(_usdtBridger), address(_usdcBridger));
    }

    /// @notice Sets the quoter used when swapping from `targetToken` into some `tokenOut`.
    /// @dev The mapping key is the input token (tokenIn). The quoter is responsible for returning
    ///      a minimum output amount used to enforce swap execution correctness.
    /// @param targetToken The input token address (tokenIn) that will use `quoter`.
    /// @param quoter The quoter contract used for swaps originating from `targetToken`.
    function setQuoter(address targetToken, IQuoter quoter) external onlyOwner {
        quoterByToken[targetToken] = quoter;
        _emitQuoterSet(targetToken, address(quoter));
    }

    // External functions

    // solhint-disable function-max-lines

    /// @notice Pulls funds from a deterministic receiver, optionally swaps, then pays locally or bridges cross-chain.
    /// @dev This is a permissionless function intended to be called by relayers.
    ///
    /// Receiver model:
    /// - A “base receiver” address is derived from `(targetChain, beneficiary, beneficiaryClaimOnly, intentHash)`.
    /// - An “ephemeral receiver” address is further derived from `(base receiver salt, forwardSalt, tokenOut, balance)`.
    /// - If `balance != 0`, the call is treated as “ephemeral mode” and funds are pulled from the
    ///   ephemeral receiver. If `balance == 0`, the call is treated as “base mode” and funds are
    ///   pulled from the base receiver.
    ///
    /// Swap model:
    /// - Swaps are only permitted in base mode. Ephemeral mode reverts with
    ///   {SwapOnEphemeralReceiversNotAllowed} if `tokenIn != tokenOut`.
    /// - A per-`tokenIn` {IQuoter} provides the minimum required output.
    /// - Any output amount above the quoted minimum is paid to `msg.sender` as a relayer incentive.
    ///
    /// Bridging model:
    /// - Cross-chain transfers only support `tokenOut` equal to the configured `USDT` or `USDC`.
    /// - The bridge destination is always the ephemeral receiver address (even if pulling from
    ///   the base receiver). This ensures each forward has a unique destination “bucket” on the
    ///   target chain, and the same address can be derived and controlled by the target-chain forwarder.
    /// - Some bridgers (e.g., LayerZero-based) require a native fee. This function is payable to allow
    ///   relayers to supply that fee via `msg.value`. For bridgers that do not use native fees, any
    ///   provided `msg.value` is refunded to the caller.
    ///
    /// Notes on `balance`:
    /// - In ephemeral mode (`balance != 0`), `balance` is the amount pulled/forwarded, and also
    ///   contributes to the ephemeral receiver address derivation.
    /// - In base mode (`balance == 0`), the function uses the receiver’s current `tokenIn` balance.
    ///
    /// @param req Pull parameters. See {PullRequest} for field semantics.
    /// @return amountOut The amount of `tokenOut` forwarded (local payout amount or bridged amount on the source chain).
    function pullFromReceiver(PullRequest calldata req) external payable returns (uint256 amountOut) {
        uint256 balanceParam = req.balance;

        // Base receiver salt: stable per (targetChain, beneficiary, claim policy, intent hash).
        bytes32 baseReceiverSalt =
            keccak256(abi.encodePacked(req.targetChain, req.beneficiary, req.beneficiaryClaimOnly, req.intentHash));

        bytes32 ephemeralReceiverSalt =
            keccak256(abi.encodePacked(baseReceiverSalt, req.forwardSalt, req.tokenOut, balanceParam));
        bool ephemeral = balanceParam != 0;

        bytes32 forwardId = keccak256(
            abi.encode(
                address(this),
                block.chainid,
                baseReceiverSalt,
                req.forwardSalt,
                req.tokenIn,
                req.tokenOut,
                balanceParam,
                req.targetChain,
                req.beneficiary,
                req.beneficiaryClaimOnly
            )
        );

        ForwardState memory st;
        st.forwardId = forwardId;
        st.bridgeDataHash = keccak256(req.bridgeData);

        address receiverUsed = predictReceiverAddress(ephemeral ? ephemeralReceiverSalt : baseReceiverSalt);
        address ephemeralReceiver = predictReceiverAddress(ephemeralReceiverSalt);

        _emitForwardStarted(
            forwardId,
            baseReceiverSalt,
            req.forwardSalt,
            req.intentHash,
            req.targetChain,
            req.beneficiary,
            req.beneficiaryClaimOnly,
            balanceParam,
            req.tokenIn,
            req.tokenOut,
            receiverUsed,
            ephemeralReceiver
        );

        ForwardOutcome memory out =
            _pullFromReceiverCore(forwardId, baseReceiverSalt, ephemeralReceiverSalt, ephemeral, req);

        _emitForwardCompleted(
            forwardId,
            ephemeral,
            out.amountPulled,
            out.amountForwarded,
            out.relayerRebate,
            out.msgValueRefunded,
            out.settledLocally,
            out.bridger,
            out.expectedBridgeOut,
            st.bridgeDataHash
        );

        return out.amountForwarded;
    }

    // solhint-enable function-max-lines

    // Public functions

    /// @notice Returns the receiver contract for `salt`, deploying it if it does not already exist.
    /// @dev The receiver is a CREATE2-deployed minimal proxy owned by this forwarder.
    /// @param salt The CREATE2 salt that determines the receiver address.
    /// @return receiver The receiver instance at the predicted address.
    function getReceiver(bytes32 salt) public returns (UntronReceiver receiver) {
        receiver = UntronReceiver(predictReceiverAddress(salt));
        if (address(receiver).code.length == 0) {
            _deployReceiver(salt);
        }
    }

    /// @notice Predict the deterministic address for a receiver deployed via CREATE2.
    /// @param salt The CREATE2 salt.
    /// @return predicted The predicted address of the receiver.
    function predictReceiverAddress(bytes32 salt) public view returns (address payable predicted) {
        // CREATE2 address formula (EIP-1014):
        // keccak256(0xff ++ deployerAddress ++ salt ++ keccak256(initcode))[12:]
        predicted = payable(address(
                uint160(uint256(keccak256(abi.encodePacked(bytes1(0xff), address(this), salt, RECEIVER_BYTECODE_HASH))))
            ));
    }

    // Internal functions

    // solhint-disable function-max-lines

    function _pullFromReceiverCore(
        bytes32 forwardId,
        bytes32 baseReceiverSalt,
        bytes32 ephemeralReceiverSalt,
        bool ephemeral,
        PullRequest calldata req
    ) internal returns (ForwardOutcome memory out) {
        uint256 balance = req.balance;
        address tokenIn = req.tokenIn;
        address tokenOut = req.tokenOut;

        // Ensure the ephemeral receiver exists (it is always the bridge destination).
        UntronReceiver ephemeralReceiver = getReceiver(ephemeralReceiverSalt);

        // Pull from the ephemeral receiver in ephemeral mode; otherwise pull from the base receiver.
        UntronReceiver receiver = ephemeral ? ephemeralReceiver : getReceiver(baseReceiverSalt);

        if (balance == 0) {
            // NOTE: In base mode, `balance == 0` is treated as “use the receiver’s current balance”.
            // Callers generally pass a nonzero `balance` when they want to pull a specific amount.
            balance = TokenUtils.getBalanceOf(tokenIn, address(receiver));
        }

        // Pull `tokenIn` from the receiver into this contract (only possible because this contract owns receivers).
        receiver.pull(tokenIn, balance);
        out.amountPulled = balance;

        if (tokenIn != tokenOut) {
            if (ephemeral) revert SwapOnEphemeralReceiversNotAllowed();

            // Quote the minimum acceptable output. The quoter is configured per tokenIn by the owner.
            uint256 amountOut = quoterByToken[tokenIn].quote(tokenIn, tokenOut, balance, block.timestamp + 10);

            // Hand the input tokens to the protocol-owned executor and execute the swap call sequence.
            TokenUtils.transfer(tokenIn, payable(address(SWAP_EXECUTOR)), balance);
            uint256 swapOut = SWAP_EXECUTOR.execute(req.swapData, tokenOut, amountOut, payable(address(this)));

            // Keep the quoted minimum `amountOut` as the forwarded balance; rebate any surplus to the relayer.
            balance = amountOut;
            out.relayerRebate = swapOut - amountOut;
            TokenUtils.transfer(tokenOut, payable(msg.sender), out.relayerRebate);
            _emitSwapExecuted(forwardId, tokenIn, tokenOut, amountOut, swapOut);
        }

        if (req.targetChain == block.chainid) {
            // Local settlement: optionally enforce that only the beneficiary can “claim”.
            if (req.beneficiaryClaimOnly && msg.sender != req.beneficiary) revert PullerUnauthorized();
            TokenUtils.transfer(tokenOut, req.beneficiary, balance);
            out.settledLocally = true;
        } else {
            // Cross-chain settlement: only allow bridging of supported stablecoins to known bridgers.
            IBridger bridger;
            if (tokenOut == USDT) {
                bridger = usdtBridger;
            } else if (tokenOut == USDC) {
                bridger = usdcBridger;
            } else {
                revert UnsupportedOutputToken();
            }
            out.bridger = address(bridger);

            uint256 expectedAmountOut;
            if (tokenOut == USDT) {
                // USDT bridging may require a native fee. Forward `msg.value` to the bridger and
                // refund any unused portion back to the relayer.
                uint256 ethBeforeBridge = address(this).balance;
                expectedAmountOut = bridger.bridge{value: msg.value}(
                    tokenOut,
                    TokenUtils.getBalanceOf(tokenOut, address(this)),
                    address(ephemeralReceiver),
                    req.targetChain,
                    req.bridgeData
                );

                // If the bridger refunded any unused native fee to this contract, pass it through to the caller.
                // The bridger is expected to either use some/all of `msg.value` or refund it back to msg.sender.
                uint256 ethAfterBridge = address(this).balance;
                uint256 refund = ethAfterBridge + msg.value - ethBeforeBridge;
                out.msgValueRefunded = refund;
                if (refund != 0) TokenUtils.transfer(address(0), payable(msg.sender), refund);
            } else {
                // USDC bridging via CCTP does not require a native fee; refund any accidental msg.value.
                out.msgValueRefunded = msg.value;
                if (msg.value != 0) TokenUtils.transfer(address(0), payable(msg.sender), msg.value);

                expectedAmountOut = bridger.bridge(
                    tokenOut,
                    TokenUtils.getBalanceOf(tokenOut, address(this)),
                    address(ephemeralReceiver),
                    req.targetChain,
                    req.bridgeData
                );
            }

            if (expectedAmountOut != balance) revert InsufficientOutputAmount();
            out.expectedBridgeOut = expectedAmountOut;
            _emitBridgeInitiated(forwardId, out.bridger, tokenOut, balance, req.targetChain);
        }

        out.amountForwarded = balance;
    }

    // solhint-enable function-max-lines

    /// @notice Deploys a receiver minimal proxy via CREATE2 for `salt`.
    /// @dev Reverts if deployment fails (e.g., due to an existing contract at the address).
    /// @param salt CREATE2 salt.
    /// @return receiver Address of the deployed receiver.
    function _deployReceiver(bytes32 salt) internal returns (address payable receiver) {
        address impl = RECEIVER_IMPLEMENTATION;
        // solhint-disable-next-line no-inline-assembly
        assembly {
            let ptr := mload(0x40)

            // EIP-1167 minimal proxy creation code that delegates to `impl`.
            mstore(ptr, 0x3d602d80600a3d3981f3363d3d373d3d3d363d73000000000000000000000000)
            mstore(add(ptr, 0x14), shl(0x60, impl))
            mstore(add(ptr, 0x28), 0x5af43d82803e903d91602b57fd5bf30000000000000000000000000000000000)

            receiver := create2(0, ptr, 0x37, salt)
            if iszero(receiver) {
                returndatacopy(0, 0, returndatasize())
                revert(0, returndatasize())
            }
        }
        _emitReceiverDeployed(salt, receiver, impl);
    }
}
