// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {EventChainGenesis} from "../utils/EventChainGenesis.sol";

/// @title UntronIntentsIndex
/// @notice Hash-chain-based event index for UntronIntents, friendly to offchain indexers.
/// @dev
/// Design goals:
/// - Canonical ordering: every meaningful state transition appends exactly one entry to a global
///   event hash chain (see {eventChainTip} / {eventSeq} / {EventAppended}).
/// - Deterministic replay: an indexer can reconstruct contract state by replaying the typed events
///   emitted here, optionally verifying integrity by recomputing the hash chain tip offchain.
///
/// Index format:
/// - Each `_emit*` function emits a typed event (e.g. {IntentCreated}), then appends a chain entry
///   using `eventSignature = <EventName>.selector` and `abiEncodedEventData = abi.encode(...)`
///   with the same fields in the same order.
///
/// Integrity check:
/// - Given all {EventAppended} logs, compute:
///   `tip[n] = sha256(abi.encodePacked(tip[n-1], seq, block.number, block.timestamp, sig, data))`
///   starting from {EventChainGenesis.UntronIntentsIndex}, and compare to the onchain {eventChainTip}.
/// @author Ultrasound Labs
contract UntronIntentsIndex {
    /*//////////////////////////////////////////////////////////////
                                  INDEX
    //////////////////////////////////////////////////////////////*/

    /// @notice Hash of the latest event in the chain.
    bytes32 public eventChainTip = EventChainGenesis.UntronIntentsIndex;

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

    /// @notice Emitted when the recommended fee schedule is updated.
    /// @param feePpm Fee in parts-per-million of the amount.
    /// @param feeFlat Flat fee component.
    event RecommendedIntentFeeSet(uint256 feePpm, uint256 feeFlat);

    /// @notice Emitted when an intent is created.
    /// @param id Deterministic id used as the mapping key in UntronIntents.
    /// @param creator Creator address (caller that caused the intent to be created).
    /// @param intentType Intent type enum value, encoded as `uint8`.
    /// @param token Escrow token on this chain (address(0) = native token).
    /// @param amount Escrow amount.
    /// @param refundBeneficiary Recipient of refunds on expiry/close.
    /// @param deadline Unix timestamp after which the intent can be closed/refunded.
    /// @param intentSpecs ABI-encoded intent-specific payload.
    event IntentCreated(
        bytes32 indexed id,
        address indexed creator,
        uint8 intentType,
        address token,
        uint256 amount,
        address refundBeneficiary,
        uint256 deadline,
        bytes intentSpecs
    );

    /// @notice Emitted for receiver-originated intents to allow offchain reconstruction of receiver params.
    /// @param id Deterministic id used as the mapping key in UntronIntents.
    /// @param forwarder IntentsForwarder contract that owns the receiver.
    /// @param toTron Tron recipient address (raw `0x41 || 20 bytes` cast into `address`).
    /// @param forwardSalt Forwarder salt used for the ephemeral receiver.
    /// @param token Escrow token on this chain.
    /// @param amount Expected ephemeral receiver balance (also used in address derivation).
    event ReceiverIntentParams(
        bytes32 indexed id,
        address indexed forwarder,
        address indexed toTron,
        bytes32 forwardSalt,
        address token,
        uint256 amount
    );

    /// @notice Emitted for receiver-originated intents to snapshot fee parameters used to compute the Tron payment.
    /// @param id Deterministic id used as the mapping key in UntronIntents.
    /// @param feePpm Fee in parts-per-million of the amount.
    /// @param feeFlat Flat fee component.
    /// @param tronPaymentAmount Amount the solver must pay on Tron for this intent (post-fee).
    event ReceiverIntentFeeSnap(bytes32 indexed id, uint256 feePpm, uint256 feeFlat, uint256 tronPaymentAmount);

    /// @notice Emitted when an intent is claimed by a solver.
    /// @param id Intent id.
    /// @param solver Solver that claimed the intent.
    /// @param depositAmount Amount of deposit token required for the claim.
    event IntentClaimed(bytes32 indexed id, address indexed solver, uint256 depositAmount);

    /// @notice Emitted when an intent is unclaimed (claim cleared) and deposits are distributed.
    /// @param id Intent id.
    /// @param caller Caller that triggered the unclaim.
    /// @param prevSolver Solver whose claim was cleared.
    /// @param funded Whether the intent escrow was funded at the time of unclaim.
    /// @param depositToCaller Deposit amount paid to `caller`.
    /// @param depositToRefundBeneficiary Deposit amount paid to the intent's `refundBeneficiary`.
    /// @param depositToPrevSolver Deposit amount refunded to `prevSolver` (unfunded-only).
    event IntentUnclaimed(
        bytes32 indexed id,
        address indexed caller,
        address indexed prevSolver,
        bool funded,
        uint256 depositToCaller,
        uint256 depositToRefundBeneficiary,
        uint256 depositToPrevSolver
    );

    /// @notice Emitted when a solver proves an intent fill on Tron.
    /// @param id Intent id.
    /// @param solver Solver that submitted the proof.
    /// @param tronTxId Tron transaction id (`sha256(raw_data)`), as exposed by explorers.
    /// @param tronBlockNumber Tron block number containing the transaction.
    event IntentSolved(bytes32 indexed id, address indexed solver, bytes32 tronTxId, uint256 tronBlockNumber);

    /// @notice Emitted when an intent becomes funded (escrow is present on this chain).
    /// @param id Intent id.
    /// @param funder Caller that caused funding (either intent creator or the funding caller).
    /// @param token Escrow token on this chain.
    /// @param amount Escrow amount funded.
    event IntentFunded(bytes32 indexed id, address indexed funder, address token, uint256 amount);

    /// @notice Emitted when a solved+funded intent is settled and the solver is paid.
    /// @param id Intent id.
    /// @param solver Solver paid for executing the intent.
    /// @param escrowToken Escrow token paid out to the solver.
    /// @param escrowAmount Escrow amount paid out to the solver.
    /// @param depositToken Deposit token paid out to the solver.
    /// @param depositAmount Deposit amount paid out to the solver.
    event IntentSettled(
        bytes32 indexed id,
        address indexed solver,
        address escrowToken,
        uint256 escrowAmount,
        address depositToken,
        uint256 depositAmount
    );

    /// @notice Emitted when an intent is closed and removed from storage.
    /// @param id Intent id.
    /// @param caller Caller that closed the intent.
    /// @param solved Whether the intent was solved at close time.
    /// @param funded Whether the escrow was funded at close time.
    /// @param settled Whether the intent was already settled at close time.
    /// @param refundBeneficiary Recipient of any escrow refund and/or penalty share.
    /// @param escrowToken Escrow token for this intent.
    /// @param escrowRefunded Escrow amount refunded (0 if none).
    /// @param depositToken Deposit token (solver claim deposit).
    /// @param depositToCaller Deposit amount paid to `caller` (0 if none).
    /// @param depositToRefundBeneficiary Deposit amount paid to `refundBeneficiary` (0 if none).
    /// @param depositToSolver Deposit amount paid to the solver (0 if none).
    event IntentClosed(
        bytes32 indexed id,
        address indexed caller,
        bool solved,
        bool funded,
        bool settled,
        address refundBeneficiary,
        address escrowToken,
        uint256 escrowRefunded,
        address depositToken,
        uint256 depositToCaller,
        uint256 depositToRefundBeneficiary,
        uint256 depositToSolver
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

    function _emitRecommendedIntentFeeSet(uint256 feePpm, uint256 feeFlat) internal {
        emit RecommendedIntentFeeSet(feePpm, feeFlat);
        _appendEventChain(RecommendedIntentFeeSet.selector, abi.encode(feePpm, feeFlat));
    }

    function _emitIntentCreated(
        bytes32 id,
        address creator,
        uint8 intentType,
        address token,
        uint256 amount,
        address refundBeneficiary,
        uint256 deadline,
        bytes memory intentSpecs
    ) internal {
        emit IntentCreated(id, creator, intentType, token, amount, refundBeneficiary, deadline, intentSpecs);
        _appendEventChain(
            IntentCreated.selector,
            abi.encode(id, creator, intentType, token, amount, refundBeneficiary, deadline, intentSpecs)
        );
    }

    function _emitReceiverIntentParams(
        bytes32 id,
        address forwarder,
        address toTron,
        bytes32 forwardSalt,
        address token,
        uint256 amount
    ) internal {
        emit ReceiverIntentParams(id, forwarder, toTron, forwardSalt, token, amount);
        _appendEventChain(ReceiverIntentParams.selector, abi.encode(id, forwarder, toTron, forwardSalt, token, amount));
    }

    function _emitReceiverIntentFeeSnap(bytes32 id, uint256 feePpm, uint256 feeFlat, uint256 tronPaymentAmount)
        internal
    {
        emit ReceiverIntentFeeSnap(id, feePpm, feeFlat, tronPaymentAmount);
        _appendEventChain(ReceiverIntentFeeSnap.selector, abi.encode(id, feePpm, feeFlat, tronPaymentAmount));
    }

    function _emitIntentClaimed(bytes32 id, address solver, uint256 depositAmount) internal {
        emit IntentClaimed(id, solver, depositAmount);
        _appendEventChain(IntentClaimed.selector, abi.encode(id, solver, depositAmount));
    }

    function _emitIntentUnclaimed(
        bytes32 id,
        address caller,
        address prevSolver,
        bool funded,
        uint256 depositToCaller,
        uint256 depositToRefundBeneficiary,
        uint256 depositToPrevSolver
    ) internal {
        emit IntentUnclaimed(
            id, caller, prevSolver, funded, depositToCaller, depositToRefundBeneficiary, depositToPrevSolver
        );
        _appendEventChain(
            IntentUnclaimed.selector,
            abi.encode(id, caller, prevSolver, funded, depositToCaller, depositToRefundBeneficiary, depositToPrevSolver)
        );
    }

    function _emitIntentSolved(bytes32 id, address solver, bytes32 tronTxId, uint256 tronBlockNumber) internal {
        emit IntentSolved(id, solver, tronTxId, tronBlockNumber);
        _appendEventChain(IntentSolved.selector, abi.encode(id, solver, tronTxId, tronBlockNumber));
    }

    function _emitIntentFunded(bytes32 id, address funder, address token, uint256 amount) internal {
        emit IntentFunded(id, funder, token, amount);
        _appendEventChain(IntentFunded.selector, abi.encode(id, funder, token, amount));
    }

    function _emitIntentSettled(
        bytes32 id,
        address solver,
        address escrowToken,
        uint256 escrowAmount,
        address depositToken,
        uint256 depositAmount
    ) internal {
        emit IntentSettled(id, solver, escrowToken, escrowAmount, depositToken, depositAmount);
        _appendEventChain(
            IntentSettled.selector, abi.encode(id, solver, escrowToken, escrowAmount, depositToken, depositAmount)
        );
    }

    function _emitIntentClosed(
        bytes32 id,
        address caller,
        bool solved,
        bool funded,
        bool settled,
        address refundBeneficiary,
        address escrowToken,
        uint256 escrowRefunded,
        address depositToken,
        uint256 depositToCaller,
        uint256 depositToRefundBeneficiary,
        uint256 depositToSolver
    ) internal {
        emit IntentClosed(
            id,
            caller,
            solved,
            funded,
            settled,
            refundBeneficiary,
            escrowToken,
            escrowRefunded,
            depositToken,
            depositToCaller,
            depositToRefundBeneficiary,
            depositToSolver
        );
        _appendEventChain(
            IntentClosed.selector,
            abi.encode(
                id,
                caller,
                solved,
                funded,
                settled,
                refundBeneficiary,
                escrowToken,
                escrowRefunded,
                depositToken,
                depositToCaller,
                depositToRefundBeneficiary,
                depositToSolver
            )
        );
    }

    function _emitOwnershipTransferred(address oldOwner, address newOwner) internal virtual {
        emit OwnershipTransferred(oldOwner, newOwner);
        _appendEventChain(OwnershipTransferred.selector, abi.encode(oldOwner, newOwner));
    }
}
