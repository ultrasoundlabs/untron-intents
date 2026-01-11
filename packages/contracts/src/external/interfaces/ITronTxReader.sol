// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

/// @title ITronTxReader
/// @notice Common interface for contracts that verify+decode Tron `TriggerSmartContract` transactions.
/// @dev Implemented by `StatefulTronTxReader` and test/dev mocks.
/// @author Ultrasound Labs
interface ITronTxReader {
    /// @notice Parsed subset of a Tron `TriggerSmartContract` transaction.
    /// @dev `txId` is the Tron transaction identifier shown by explorers and equals `sha256(raw_data)`.
    struct TriggerSmartContract {
        bytes32 txId;
        uint256 tronBlockNumber;
        uint32 tronBlockTimestamp;
        bytes21 senderTron;
        bytes21 toTron;
        bytes data;
    }

    /// @notice Verifies inclusion of `encodedTx` in the first block and returns parsed call data.
    /// @param blocks 20 Protobuf-encoded Tron `BlockHeader` bytes, including signature.
    ///               The first block must be the one containing the transaction.
    /// @param encodedTx Raw protobuf-encoded Tron `Transaction` bytes.
    /// @param proof SHA-256 Merkle proof for the transaction leaf within the block's transaction tree.
    /// @param index 0-based leaf index in the Merkle tree used by the verifier.
    /// @return callData Parsed `TriggerSmartContract` subset containing the call data bytes.
    function readTriggerSmartContract(
        bytes[20] calldata blocks,
        bytes calldata encodedTx,
        bytes32[] calldata proof,
        uint256 index
    ) external view returns (TriggerSmartContract memory callData);
}
