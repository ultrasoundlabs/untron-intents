// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {EventChainGenesis} from "../utils/EventChainGenesis.sol";

/// @title IntentsForwarderIndex
/// @notice Hash-chain-based event index for IntentsForwarder, friendly to offchain indexers.
/// @dev IntentsForwarder should emit canonical events only through the `_emit*` helpers here.
/// @author Ultrasound Labs
contract IntentsForwarderIndex {
    /*//////////////////////////////////////////////////////////////
                                  INDEX
    //////////////////////////////////////////////////////////////*/

    /// @notice Hash of the latest event in the chain.
    bytes32 public eventChainTip = EventChainGenesis.IntentsForwarderIndex;

    /// @notice Monotonically increasing sequence number for appended events.
    uint256 public eventSeq;

    /// @notice Emitted when an event is appended to the event chain.
    /// @param eventSeq The sequence number of the appended event.
    /// @param prevTip The hash of the previous event in the chain.
    /// @param newTip The hash of the newly appended event.
    /// @param eventSignature The signature (selector) of the event.
    /// @param abiEncodedEventData ABI-encoded event data.
    event EventAppended(
        uint256 indexed eventSeq,
        bytes32 indexed prevTip,
        bytes32 indexed newTip,
        bytes32 eventSignature,
        bytes abiEncodedEventData
    );

    /*//////////////////////////////////////////////////////////////
                                  EVENTS
    //////////////////////////////////////////////////////////////*/

    /// @notice Emitted when the configured bridgers are updated.
    /// @param usdtBridger Bridger used when bridging `USDT`.
    /// @param usdcBridger Bridger used when bridging `USDC`.
    event BridgersSet(address indexed usdtBridger, address indexed usdcBridger);

    /// @notice Emitted when the quoter for a `tokenIn` is updated.
    /// @param tokenIn Input token for swaps (the key in the forwarder's `quoterByToken` mapping).
    /// @param quoter Quoter contract address.
    event QuoterSet(address indexed tokenIn, address indexed quoter);

    /// @notice Emitted when a receiver proxy is deployed via CREATE2.
    /// @param receiverSalt CREATE2 salt used to derive the receiver address.
    /// @param receiver Deployed receiver address.
    /// @param implementation Receiver implementation (EIP-1167 delegate target).
    event ReceiverDeployed(bytes32 indexed receiverSalt, address indexed receiver, address implementation);

    /// @notice Emitted at the start of a forward attempt (will not persist if the tx reverts).
    /// @param forwardId Deterministic identifier for this forward attempt.
    /// @param baseReceiverSalt Base receiver salt derived from `(targetChain, beneficiary, beneficiaryClaimOnly, intentHash)`.
    /// @param forwardSalt Extra salt used to create unique ephemeral receivers per forward.
    /// @param intentHash User-supplied identifier squashed into the base receiver salt.
    /// @param targetChain Destination EVM chainId.
    /// @param beneficiary Recipient on the local chain (or claimant, if `beneficiaryClaimOnly`).
    /// @param beneficiaryClaimOnly Whether only `beneficiary` may call when settling locally.
    /// @param balanceParam Original `balance` parameter supplied by the caller.
    /// @param tokenIn Token pulled from the receiver (address(0) = native token).
    /// @param tokenOut Token delivered/bridged (address(0) = native token).
    /// @param receiverUsed Receiver address funds are pulled from (base or ephemeral).
    /// @param ephemeralReceiver Ephemeral receiver address for this forward (bridge destination).
    event ForwardStarted(
        bytes32 indexed forwardId,
        bytes32 indexed baseReceiverSalt,
        bytes32 indexed forwardSalt,
        bytes32 intentHash,
        uint256 targetChain,
        address beneficiary,
        bool beneficiaryClaimOnly,
        uint256 balanceParam,
        address tokenIn,
        address tokenOut,
        address receiverUsed,
        address ephemeralReceiver
    );

    /// @notice Emitted after successful completion of a forward.
    /// @param forwardId Deterministic identifier for this forward attempt.
    /// @param ephemeral Whether the forward ran in ephemeral mode (`balanceParam != 0`).
    /// @param amountPulled Amount of `tokenIn` pulled from the receiver into the forwarder.
    /// @param amountForwarded Amount of `tokenOut` forwarded (local payout or bridged amount).
    /// @param relayerRebate Swap surplus rebated to the relayer (0 if no swap).
    /// @param msgValueRefunded Native value refunded to the relayer (0 if none).
    /// @param settledLocally True if `targetChain == block.chainid` and a local transfer was performed.
    /// @param bridger Bridger contract used for cross-chain settlement (0 if local).
    /// @param expectedBridgeOut Expected destination amount as returned by the bridger (0 if local).
    /// @param bridgeDataHash `keccak256(bridgeData)` from the forward call.
    event ForwardCompleted(
        bytes32 indexed forwardId,
        bool ephemeral,
        uint256 amountPulled,
        uint256 amountForwarded,
        uint256 relayerRebate,
        uint256 msgValueRefunded,
        bool settledLocally,
        address bridger,
        uint256 expectedBridgeOut,
        bytes32 bridgeDataHash
    );

    /// @notice Emitted when a swap was executed (base mode only).
    /// @param forwardId Deterministic identifier for this forward attempt.
    /// @param tokenIn Token swapped from.
    /// @param tokenOut Token swapped to.
    /// @param minOut Minimum output enforced (from the configured quoter).
    /// @param actualOut Total output produced by swap execution.
    event SwapExecuted(
        bytes32 indexed forwardId, address indexed tokenIn, address indexed tokenOut, uint256 minOut, uint256 actualOut
    );

    /// @notice Emitted when a bridge was initiated.
    /// @param forwardId Deterministic identifier for this forward attempt.
    /// @param bridger Bridger contract used for bridging.
    /// @param tokenOut Token bridged.
    /// @param amountIn Amount of `tokenOut` bridged.
    /// @param targetChain Destination EVM chainId.
    event BridgeInitiated(
        bytes32 indexed forwardId,
        address indexed bridger,
        address indexed tokenOut,
        uint256 amountIn,
        uint256 targetChain
    );

    /// @dev Kept identical to OZ Ownable / EIP-173 for compatibility.
    /// @notice Emitted when contract ownership is transferred.
    /// @param oldOwner The previous owner.
    /// @param newOwner The new owner.
    event OwnershipTransferred(address indexed oldOwner, address indexed newOwner);

    /*//////////////////////////////////////////////////////////////
                            APPEND EVENT CHAIN
    //////////////////////////////////////////////////////////////*/

    function _appendEventChain(bytes32 eventSignature, bytes memory abiEncodedEventData) internal {
        unchecked {
            ++eventSeq;
        }
        bytes32 prevTip = eventChainTip;
        eventChainTip = sha256(
            abi.encodePacked(
                eventChainTip, eventSeq, block.number, block.timestamp, eventSignature, abiEncodedEventData
            )
        );
        emit EventAppended(eventSeq, prevTip, eventChainTip, eventSignature, abiEncodedEventData);
    }

    /*//////////////////////////////////////////////////////////////
                                 EMITTERS
    //////////////////////////////////////////////////////////////*/

    function _emitBridgersSet(address usdtBridger, address usdcBridger) internal {
        emit BridgersSet(usdtBridger, usdcBridger);
        _appendEventChain(BridgersSet.selector, abi.encode(usdtBridger, usdcBridger));
    }

    function _emitQuoterSet(address tokenIn, address quoter) internal {
        emit QuoterSet(tokenIn, quoter);
        _appendEventChain(QuoterSet.selector, abi.encode(tokenIn, quoter));
    }

    function _emitReceiverDeployed(bytes32 receiverSalt, address receiver, address implementation) internal {
        emit ReceiverDeployed(receiverSalt, receiver, implementation);
        _appendEventChain(ReceiverDeployed.selector, abi.encode(receiverSalt, receiver, implementation));
    }

    function _emitForwardStarted(
        bytes32 forwardId,
        bytes32 baseReceiverSalt,
        bytes32 forwardSalt,
        bytes32 intentHash,
        uint256 targetChain,
        address beneficiary,
        bool beneficiaryClaimOnly,
        uint256 balanceParam,
        address tokenIn,
        address tokenOut,
        address receiverUsed,
        address ephemeralReceiver
    ) internal {
        emit ForwardStarted(
            forwardId,
            baseReceiverSalt,
            forwardSalt,
            intentHash,
            targetChain,
            beneficiary,
            beneficiaryClaimOnly,
            balanceParam,
            tokenIn,
            tokenOut,
            receiverUsed,
            ephemeralReceiver
        );
        _appendEventChain(
            ForwardStarted.selector,
            abi.encode(
                forwardId,
                baseReceiverSalt,
                forwardSalt,
                intentHash,
                targetChain,
                beneficiary,
                beneficiaryClaimOnly,
                balanceParam,
                tokenIn,
                tokenOut,
                receiverUsed,
                ephemeralReceiver
            )
        );
    }

    function _emitForwardCompleted(
        bytes32 forwardId,
        bool ephemeral,
        uint256 amountPulled,
        uint256 amountForwarded,
        uint256 relayerRebate,
        uint256 msgValueRefunded,
        bool settledLocally,
        address bridger,
        uint256 expectedBridgeOut,
        bytes32 bridgeDataHash
    ) internal {
        emit ForwardCompleted(
            forwardId,
            ephemeral,
            amountPulled,
            amountForwarded,
            relayerRebate,
            msgValueRefunded,
            settledLocally,
            bridger,
            expectedBridgeOut,
            bridgeDataHash
        );
        _appendEventChain(
            ForwardCompleted.selector,
            abi.encode(
                forwardId,
                ephemeral,
                amountPulled,
                amountForwarded,
                relayerRebate,
                msgValueRefunded,
                settledLocally,
                bridger,
                expectedBridgeOut,
                bridgeDataHash
            )
        );
    }

    function _emitSwapExecuted(bytes32 forwardId, address tokenIn, address tokenOut, uint256 minOut, uint256 actualOut)
        internal
    {
        emit SwapExecuted(forwardId, tokenIn, tokenOut, minOut, actualOut);
        _appendEventChain(SwapExecuted.selector, abi.encode(forwardId, tokenIn, tokenOut, minOut, actualOut));
    }

    function _emitBridgeInitiated(
        bytes32 forwardId,
        address bridger,
        address tokenOut,
        uint256 amountIn,
        uint256 targetChain
    ) internal {
        emit BridgeInitiated(forwardId, bridger, tokenOut, amountIn, targetChain);
        _appendEventChain(BridgeInitiated.selector, abi.encode(forwardId, bridger, tokenOut, amountIn, targetChain));
    }

    function _emitOwnershipTransferred(address oldOwner, address newOwner) internal virtual {
        emit OwnershipTransferred(oldOwner, newOwner);
        _appendEventChain(OwnershipTransferred.selector, abi.encode(oldOwner, newOwner));
    }
}
