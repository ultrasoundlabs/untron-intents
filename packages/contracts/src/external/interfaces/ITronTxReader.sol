// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

/// @notice Parsed subset of a Tron `TriggerSmartContract` transaction.
/// @dev `txId` is the Tron transaction identifier shown by explorers and equals `sha256(raw_data)`.
struct TriggerSmartContract {
    bytes32 txId;
    uint256 tronBlockNumber;
    uint32 tronBlockTimestamp;
    bytes21 senderTron;
    bytes21 toTron;
    /// @notice TRX attached to the call, in sun (1 TRX = 1_000_000 sun).
    /// @dev This is `TriggerSmartContract.call_value` from Tron protobuf.
    uint256 callValueSun;
    bytes data;
}

/// @notice Parsed subset of a Tron `TransferContract` transaction (TRX transfer).
/// @dev `txId` is the Tron transaction identifier shown by explorers and equals `sha256(raw_data)`.
struct TransferContract {
    bytes32 txId;
    uint256 tronBlockNumber;
    uint32 tronBlockTimestamp;
    bytes21 senderTron;
    bytes21 toTron;
    /// @notice TRX amount transferred, in sun (1 TRX = 1_000_000 sun).
    uint256 amountSun;
}

/// @notice Parsed subset of a Tron `DelegateResourceContract` transaction (resource delegation).
/// @dev `txId` is the Tron transaction identifier shown by explorers and equals `sha256(raw_data)`.
struct DelegateResourceContract {
    bytes32 txId;
    uint256 tronBlockNumber;
    /// @notice Amount of TRX staked for the delegation, in sun.
    uint256 balanceSun;
    /// @notice Lock period value as encoded by Tron (typically interpreted as # of blocks).
    uint256 lockPeriod;
    bytes21 ownerTron;
    bytes21 receiverTron;
    uint32 tronBlockTimestamp;
    /// @notice Which resource is delegated (BANDWIDTH=0, ENERGY=1, TRON_POWER=2).
    uint8 resource;
    /// @notice Whether delegation is time-locked.
    bool lock;
}

/// @title ITronTxReader
/// @notice Common interface for contracts that verify+decode Tron `TriggerSmartContract` transactions.
/// @dev Implemented by `StatefulTronTxReader` and test/dev mocks.
/// @author Ultrasound Labs
interface ITronTxReader {
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

    /// @notice Verifies inclusion of `encodedTx` in the first block and returns parsed TRX transfer data.
    /// @param blocks 20 Protobuf-encoded Tron `BlockHeader` bytes, including signature.
    ///               The first block must be the one containing the transaction.
    /// @param encodedTx Raw protobuf-encoded Tron `Transaction` bytes.
    /// @param proof SHA-256 Merkle proof for the transaction leaf within the block's transaction tree.
    /// @param index 0-based leaf index in the Merkle tree used by the verifier.
    /// @return transfer Parsed `TransferContract` subset containing sender, recipient, amount, and block metadata.
    function readTransferContract(
        bytes[20] calldata blocks,
        bytes calldata encodedTx,
        bytes32[] calldata proof,
        uint256 index
    ) external view returns (TransferContract memory transfer);

    /// @notice Verifies inclusion of `encodedTx` in the first block and returns parsed resource delegation data.
    /// @param blocks 20 Protobuf-encoded Tron `BlockHeader` bytes, including signature.
    ///               The first block must be the one containing the transaction.
    /// @param encodedTx Raw protobuf-encoded Tron `Transaction` bytes.
    /// @param proof SHA-256 Merkle proof for the transaction leaf within the block's transaction tree.
    /// @param index 0-based leaf index in the Merkle tree used by the verifier.
    /// @return delegation Parsed `DelegateResourceContract` subset containing params and block metadata.
    function readDelegateResourceContract(
        bytes[20] calldata blocks,
        bytes calldata encodedTx,
        bytes32[] calldata proof,
        uint256 index
    ) external view returns (DelegateResourceContract memory delegation);
}
