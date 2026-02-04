// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {
    ITronTxReader,
    TriggerSmartContract,
    TransferContract,
    DelegateResourceContract
} from "../external/interfaces/ITronTxReader.sol";

import {ProtoVarint, ProtoTruncated} from "../utils/ProtoVarint.sol";
import {TronTxInclusionVerifier} from "../utils/TronTxInclusionVerifier.sol";
import {TronTxParser} from "../utils/TronTxParser.sol";
import {TronTxReaderErrors} from "../TronTxReaderErrors.sol";

/// @title TestTronTxReaderSig
/// @notice Test-only Tron tx reader that verifies inclusion proofs and witness signatures, but
///         does not require a full 27-SR set (useful for private Tron networks like tronbox/tre).
/// @dev This contract intentionally does *not* implement SR uniqueness / rotation checks. It is a
///      middle-ground between `TestTronTxReaderNoSig` and the production `StatefulTronTxReader`.
contract TestTronTxReaderSig is ITronTxReader {
    bytes20 public immutable expectedWitnessAddress;
    bytes20 public immutable expectedWitnessDelegatee;

    constructor(bytes20 _expectedWitnessAddress, bytes20 _expectedWitnessDelegatee) {
        expectedWitnessAddress = _expectedWitnessAddress;
        expectedWitnessDelegatee = _expectedWitnessDelegatee;
    }

    function readTriggerSmartContract(
        bytes[20] calldata blocks,
        bytes calldata encodedTx,
        bytes32[] calldata proof,
        uint256 index
    ) external view returns (TriggerSmartContract memory callData) {
        (uint256 blockNumber, uint32 blockTimestamp, bytes32 txTrieRoot) = _verifyBlocks(blocks);
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
    ) external view returns (TransferContract memory transfer) {
        (uint256 blockNumber, uint32 blockTimestamp, bytes32 txTrieRoot) = _verifyBlocks(blocks);
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
    ) external view returns (DelegateResourceContract memory delegation) {
        (uint256 blockNumber, uint32 blockTimestamp, bytes32 txTrieRoot) = _verifyBlocks(blocks);
        TronTxInclusionVerifier.verifyTxMerkleProof(txTrieRoot, encodedTx, proof, index);
        delegation = TronTxParser.parseDelegateResourceContract(encodedTx);
        delegation.tronBlockNumber = blockNumber;
        delegation.tronBlockTimestamp = blockTimestamp;
    }

    function _verifyBlocks(bytes[20] calldata blocks)
        private
        view
        returns (uint256 blockNumber, uint32 blockTimestamp, bytes32 txTrieRoot)
    {
        bytes32 prevBlockId;

        for (uint256 i = 0; i < blocks.length; ++i) {
            (uint32 ts, bytes32 root, bytes32 parentHash, uint256 n, bytes20 witnessAddress) =
                _parseTronBlock(blocks[i]);

            if (i == 0) {
                blockNumber = n;
                blockTimestamp = ts;
                txTrieRoot = root;
            }

            if (prevBlockId != bytes32(0) && prevBlockId != parentHash) {
                revert TronTxReaderErrors.InvalidBlockSequence();
            }
            if (expectedWitnessAddress != bytes20(0) && witnessAddress != expectedWitnessAddress) {
                revert TronTxReaderErrors.UnknownSr(witnessAddress);
            }

            // Tron witnesses sign the `BlockHeader.raw_data` message bytes (not the full header including signature).
            bytes32 blockHash = _hashTronBlockRawData(blocks[i]);
            bytes20 signer = _ecrecoverFromBlock(blockHash, blocks[i]);
            if (signer != expectedWitnessDelegatee) revert TronTxReaderErrors.InvalidWitnessSignature();

            prevBlockId = _makeBlockId(n, blockHash);
        }
    }

    function _hashTronBlockRawData(bytes calldata encodedBlock) private pure returns (bytes32 digest) {
        // Outer encoding is: 0x0a 0x69 || raw_data (105 bytes) || 0x12 0x41 || signature (65 bytes).
        digest = sha256(encodedBlock[2:107]);
    }

    function _ecrecoverFromBlock(bytes32 digest, bytes calldata encodedBlock) private view returns (bytes20 signer) {
        // encodedBlock layout is fixed by `_parseTronBlock`: signature at offsets 109..173 (65 bytes).
        bytes32 r;
        bytes32 s;
        uint8 v;

        // solhint-disable-next-line no-inline-assembly
        assembly {
            let base := encodedBlock.offset
            r := calldataload(add(base, 109))
            s := calldataload(add(base, 141))
            v := byte(0, calldataload(add(base, 173)))
        }

        // Tron stores signatures as [r|s|v], with v commonly in {0,1} and sometimes {27,28}.
        // EVM ecrecover expects v in {27,28}.
        if (v == 0 || v == 1) v += 27;
        if (v != 27 && v != 28) revert TronTxReaderErrors.InvalidWitnessSignature();

        // solhint-disable-next-line no-inline-assembly
        assembly ("memory-safe") {
            let ptr := mload(0x40)
            mstore(ptr, digest)
            mstore(add(ptr, 0x20), v)
            mstore(add(ptr, 0x40), r)
            mstore(add(ptr, 0x60), s)

            if iszero(staticcall(gas(), 1, ptr, 0x80, ptr, 0x20)) {
                mstore(0x00, 0)
                revert(0x00, 0x00)
            }

            let a := mload(ptr)
            signer := shl(96, a)
        }
    }

    function _parseTronBlock(bytes calldata encodedBlock)
        private
        pure
        returns (
            uint32 blockTimestamp, // seconds
            bytes32 txTrieRoot,
            bytes32 parentHash,
            uint256 number,
            bytes20 witnessAddress // EVM-style 20 bytes (drops 0x41 prefix)
        )
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
        // parentHash payload at full offset 45 (32 bytes)
        // solhint-disable-next-line no-inline-assembly
        assembly {
            let base := encodedBlock.offset
            txTrieRoot := calldataload(add(base, 11))
            parentHash := calldataload(add(base, 45))
        }

        // block number at full offset 78, fixed 4-byte varint
        (number,) = ProtoVarint.read(encodedBlock, 78, 78 + 4);

        // witnessAddress payload at full offset 84 (21 bytes): 0x41 + 20 bytes
        uint8 prefix = uint8(encodedBlock[84]);
        if (prefix != 0x41) revert TronTxReaderErrors.InvalidWitnessAddressPrefix(prefix);

        bytes32 tmp;
        // solhint-disable-next-line no-inline-assembly
        assembly {
            tmp := calldataload(add(encodedBlock.offset, 85))
        }
        // forge-lint: disable-next-line(unsafe-typecast)
        witnessAddress = bytes20(tmp);
    }

    function _makeBlockId(uint256 blockNumber, bytes32 blockHash) private pure returns (bytes32 out) {
        if (blockNumber > type(uint64).max) revert ProtoTruncated();
        // solhint-disable-next-line no-inline-assembly
        assembly {
            out := 
            // keep the low 192 bits (last 24 bytes) of sha256(BlockHeader_raw)
            or(
                shl(192, blockNumber),
                and(blockHash, 0x0000000000000000ffffffffffffffffffffffffffffffffffffffffffffffff)
            )
        }
    }
}
