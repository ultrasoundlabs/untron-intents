// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {
    ITronTxReader,
    TriggerSmartContract,
    TransferContract,
    DelegateResourceContract
} from "../external/interfaces/ITronTxReader.sol";

import {ProtoVarint} from "../utils/ProtoVarint.sol";
import {TronTxInclusionVerifier} from "../utils/TronTxInclusionVerifier.sol";
import {TronTxParser} from "../utils/TronTxParser.sol";
import {TronTxReaderErrors} from "../TronTxReaderErrors.sol";

/// @title TestTronTxReaderNoSig
/// @notice Test-only Tron tx reader that verifies inclusion proofs but skips SR signature/finality checks.
/// @dev Intended for e2e tests against private Tron networks (e.g. tronbox/tre) where the witness set
///      is not configured on the hub chain.
contract TestTronTxReaderNoSig is ITronTxReader {
    function readTriggerSmartContract(
        bytes[20] calldata blocks,
        bytes calldata encodedTx,
        bytes32[] calldata proof,
        uint256 index
    ) external pure returns (TriggerSmartContract memory callData) {
        (uint256 blockNumber, uint32 blockTimestamp, bytes32 txTrieRoot) = _parseFirstBlock(blocks[0]);
        TronTxInclusionVerifier.verifyTxMerkleProof(txTrieRoot, encodedTx, proof, index);
        callData = TronTxParser.parseTriggerSmartContract(encodedTx);
        callData.tronBlockNumber = blockNumber;
        callData.tronBlockTimestamp = blockTimestamp;
    }

    function readTransferContract(
        bytes[20] calldata blocks,
        bytes calldata encodedTx,
        bytes32[] calldata proof,
        uint256 index
    ) external pure returns (TransferContract memory transfer) {
        (uint256 blockNumber, uint32 blockTimestamp, bytes32 txTrieRoot) = _parseFirstBlock(blocks[0]);
        TronTxInclusionVerifier.verifyTxMerkleProof(txTrieRoot, encodedTx, proof, index);
        transfer = TronTxParser.parseTransferContract(encodedTx);
        transfer.tronBlockNumber = blockNumber;
        transfer.tronBlockTimestamp = blockTimestamp;
    }

    function readDelegateResourceContract(
        bytes[20] calldata blocks,
        bytes calldata encodedTx,
        bytes32[] calldata proof,
        uint256 index
    ) external pure returns (DelegateResourceContract memory delegation) {
        (uint256 blockNumber, uint32 blockTimestamp, bytes32 txTrieRoot) = _parseFirstBlock(blocks[0]);
        TronTxInclusionVerifier.verifyTxMerkleProof(txTrieRoot, encodedTx, proof, index);
        delegation = TronTxParser.parseDelegateResourceContract(encodedTx);
        delegation.tronBlockNumber = blockNumber;
        delegation.tronBlockTimestamp = blockTimestamp;
    }

    function _parseFirstBlock(bytes calldata encodedBlock)
        private
        pure
        returns (uint256 blockNumber, uint32 blockTimestamp, bytes32 txTrieRoot)
    {
        if (encodedBlock.length != 174) revert TronTxReaderErrors.InvalidEncodedBlockLength(encodedBlock.length);

        // Sanity-check outer framing matches the assumed fixed layout.
        if (
            encodedBlock[0] != 0x0a || encodedBlock[1] != 0x69 || encodedBlock[107] != 0x12 || encodedBlock[108] != 0x41
        ) {
            revert TronTxReaderErrors.InvalidHeaderPrefix();
        }

        // timestamp (ms) at full offset 3, fixed 6-byte varint
        (uint256 tsMs,) = ProtoVarint.read(encodedBlock, 3, 3 + 6);
        uint256 tsSec = tsMs / 1000;
        if (tsSec > type(uint32).max) revert TronTxReaderErrors.TimestampOverflow();
        // forge-lint: disable-next-line(unsafe-typecast)
        blockTimestamp = uint32(tsSec);

        // txTrieRoot payload at full offset 11 (32 bytes)
        // solhint-disable-next-line no-inline-assembly
        assembly {
            txTrieRoot := calldataload(add(encodedBlock.offset, 11))
        }

        // block number at full offset 78, fixed 4-byte varint
        (blockNumber,) = ProtoVarint.read(encodedBlock, 78, 78 + 4);
    }
}

