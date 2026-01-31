// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {
    ITronTxReader,
    TriggerSmartContract,
    TransferContract,
    DelegateResourceContract
} from "./external/interfaces/ITronTxReader.sol";

import {TronTxReaderErrors} from "./TronTxReaderErrors.sol";
import {TronBlockHeaderVerifier} from "./utils/TronBlockHeaderVerifier.sol";
import {TronTxInclusionVerifier} from "./utils/TronTxInclusionVerifier.sol";
import {TronTxParser} from "./utils/TronTxParser.sol";

/// @title StatefulTronTxReader
/// @notice Verifies 20 Tron headers for SR finality, proves tx inclusion, then decodes a narrow tx view.
/// @author Ultrasound Labs
contract StatefulTronTxReader is ITronTxReader {
    /// @notice index of a witness delegatee for this witness plus 1, or 0 if not an SR.
    mapping(bytes20 => uint8) public srIndexPlus1; // 0 = not allowed, else (index+1) in [1..27]

    /// @notice SR owner accounts for the epoch (used for `witnessAddress` in header encoding).
    bytes20[27] public srs;

    /// @notice SR signing keys for the epoch (used for signature recovery checks).
    bytes20[27] public witnessDelegatees;

    constructor(bytes20[27] memory _srs, bytes20[27] memory _witnessDelegatees) {
        srIndexPlus1[_srs[0]] = uint8(1);
        for (uint256 i = 1; i < 27; ++i) {
            bytes20 prev = _srs[i - 1];
            bytes20 next = _srs[i];
            // solhint-disable-next-line gas-strict-inequalities
            if (uint160(prev) >= uint160(next)) revert TronTxReaderErrors.SrSetNotSorted(i, prev, next);
            // casting to 'uint8' is safe because i is in [1..26], so (i+1) is in [2..27]
            // forge-lint: disable-next-line(unsafe-typecast)
            srIndexPlus1[_srs[i]] = uint8(i + 1);
        }

        srs = _srs;
        witnessDelegatees = _witnessDelegatees;
    }

    /// @notice Verifies inclusion of a Tron `TriggerSmartContract` transaction and returns a parsed view.
    /// @param blocks 20 packed Tron block headers (first contains the tx).
    /// @param encodedTx Protobuf-encoded Tron `Transaction`.
    /// @param proof SHA-256 Merkle proof for tx inclusion.
    /// @param index 0-based tx leaf index in the Merkle tree.
    /// @return callData Parsed `TriggerSmartContract` subset, including block metadata.
    function readTriggerSmartContract(
        bytes[20] calldata blocks,
        bytes calldata encodedTx,
        bytes32[] calldata proof,
        uint256 index
    ) external view returns (TriggerSmartContract memory callData) {
        (uint256 blockNumber, uint32 blockTimestamp, bytes32 txTrieRoot) =
            TronBlockHeaderVerifier.verifyFirstBlockFinality(blocks, srIndexPlus1, witnessDelegatees);
        TronTxInclusionVerifier.verifyTxMerkleProof(txTrieRoot, encodedTx, proof, index);
        callData = TronTxParser.parseTriggerSmartContract(encodedTx);
        callData.tronBlockNumber = blockNumber;
        callData.tronBlockTimestamp = blockTimestamp;
    }

    /// @notice Verifies inclusion of a Tron `TransferContract` transaction and returns a parsed view.
    /// @param blocks 20 packed Tron block headers (first contains the tx).
    /// @param encodedTx Protobuf-encoded Tron `Transaction`.
    /// @param proof SHA-256 Merkle proof for tx inclusion.
    /// @param index 0-based tx leaf index in the Merkle tree.
    /// @return transfer Parsed `TransferContract` subset, including block metadata.
    function readTransferContract(
        bytes[20] calldata blocks,
        bytes calldata encodedTx,
        bytes32[] calldata proof,
        uint256 index
    ) external view returns (TransferContract memory transfer) {
        (uint256 blockNumber, uint32 blockTimestamp, bytes32 txTrieRoot) =
            TronBlockHeaderVerifier.verifyFirstBlockFinality(blocks, srIndexPlus1, witnessDelegatees);
        TronTxInclusionVerifier.verifyTxMerkleProof(txTrieRoot, encodedTx, proof, index);
        transfer = TronTxParser.parseTransferContract(encodedTx);
        transfer.tronBlockNumber = blockNumber;
        transfer.tronBlockTimestamp = blockTimestamp;
    }

    /// @notice Verifies inclusion of a Tron `DelegateResourceContract` transaction and returns a parsed view.
    /// @param blocks 20 packed Tron block headers (first contains the tx).
    /// @param encodedTx Protobuf-encoded Tron `Transaction`.
    /// @param proof SHA-256 Merkle proof for tx inclusion.
    /// @param index 0-based tx leaf index in the Merkle tree.
    /// @return delegation Parsed `DelegateResourceContract` subset, including block metadata.
    function readDelegateResourceContract(
        bytes[20] calldata blocks,
        bytes calldata encodedTx,
        bytes32[] calldata proof,
        uint256 index
    ) external view returns (DelegateResourceContract memory delegation) {
        (uint256 blockNumber, uint32 blockTimestamp, bytes32 txTrieRoot) =
            TronBlockHeaderVerifier.verifyFirstBlockFinality(blocks, srIndexPlus1, witnessDelegatees);
        TronTxInclusionVerifier.verifyTxMerkleProof(txTrieRoot, encodedTx, proof, index);
        delegation = TronTxParser.parseDelegateResourceContract(encodedTx);
        delegation.tronBlockNumber = blockNumber;
        delegation.tronBlockTimestamp = blockTimestamp;
    }
}
