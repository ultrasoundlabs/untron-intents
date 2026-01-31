// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

/// @title TronTxReaderErrors
/// @notice Shared custom errors for Tron tx reader implementations and helpers.
/// @dev Declared in a standalone contract so libraries can reference them via
///      `revert TronTxReaderErrors.SomeError(...)` without duplicating selectors.
/// @author Ultrasound Labs
abstract contract TronTxReaderErrors {
    // SR set / consensus
    error SrSetNotSorted(uint256 index, bytes20 prev, bytes20 next);
    error UnknownSr(bytes20 sr);
    error DuplicateSr(bytes20 sr);
    error InvalidBlockSequence();
    error InvalidEncodedBlockLength(uint256 got);
    error InvalidHeaderPrefix();
    error InvalidWitnessAddressPrefix(uint8 got);
    error InvalidWitnessSignature();
    error TimestampOverflow();

    // Inclusion
    error InvalidTxMerkleProof();

    // Tx types / success
    error NotTriggerSmartContract();
    error NotTransferContract();
    error NotDelegateResourceContract();
    error TronTxNotSuccessful();

    // Address / field validation
    error TronInvalidOwnerLength();
    error TronInvalidOwnerPrefix();
    error TronInvalidContractLength();
    error TronInvalidContractPrefix();
    error TronInvalidCallValue();
    error TronInvalidResource();
    error TronInvalidReceiverLength();
    error TronInvalidReceiverPrefix();
    error TronInvalidBalance();
    error TronInvalidLock();
    error TronInvalidLockPeriod();
}
