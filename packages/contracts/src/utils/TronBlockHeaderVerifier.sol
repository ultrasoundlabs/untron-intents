// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {ProtoVarint, ProtoTruncated} from "./ProtoVarint.sol";
import {TronTxReaderErrors} from "../TronTxReaderErrors.sol";

/// @title TronBlockHeaderVerifier
/// @notice Verifies Tron block header signatures and simple "finality" over a 20-block window.
/// @author Ultrasound Labs
library TronBlockHeaderVerifier {
    function verifyFirstBlockFinality(
        bytes[20] calldata blocks,
        mapping(bytes20 => uint8) storage srIndexPlus1,
        bytes20[27] storage witnessDelegatees
    ) internal view returns (uint256 blockNumber, uint32 blockTimestamp, bytes32 txTrieRoot) {
        bytes32 prevBlockId;
        uint32 seen; // uses low 27 bits

        for (uint256 i = 0; i < blocks.length; ++i) {
            (bytes32 nextPrevBlockId, uint32 nextSeen, uint256 n, uint32 ts, bytes32 root) =
                _verifyBlock(blocks[i], prevBlockId, seen, srIndexPlus1, witnessDelegatees);
            prevBlockId = nextPrevBlockId;
            seen = nextSeen;
            if (i == 0) {
                blockNumber = n;
                blockTimestamp = ts;
                txTrieRoot = root;
            }
        }
    }

    function _verifyBlock(
        bytes calldata block_,
        bytes32 prevBlockId,
        uint32 seen,
        mapping(bytes20 => uint8) storage srIndexPlus1,
        bytes20[27] storage witnessDelegatees
    )
        private
        view
        returns (bytes32 nextBlockId, uint32 nextSeen, uint256 blockNumber, uint32 blockTimestamp, bytes32 txTrieRoot)
    {
        bytes32 parentHash; // the name follows Tron's semantics but is misleading; it's prev block ID
        bytes20 witnessAddress;

        (blockTimestamp, txTrieRoot, parentHash, blockNumber, witnessAddress) = _parseTronBlock(block_);

        if (prevBlockId != bytes32(0) && prevBlockId != parentHash) revert TronTxReaderErrors.InvalidBlockSequence();

        // Tron witnesses sign the `BlockHeader.raw_data` message bytes (not the full header including signature).
        bytes32 blockHash = _hashTronBlockRawData(block_);
        bytes20 signer = _ecrecoverFromBlock(blockHash, block_);

        uint8 idxPlus1 = srIndexPlus1[witnessAddress];
        if (idxPlus1 == 0) revert TronTxReaderErrors.UnknownSr(witnessAddress);
        uint8 idx = idxPlus1 - 1; // 0..26

        uint32 bit = uint32(1) << idx;
        if (seen & bit != 0) revert TronTxReaderErrors.DuplicateSr(witnessAddress);
        nextSeen = seen | bit;

        if (witnessDelegatees[idx] != signer) revert TronTxReaderErrors.InvalidWitnessSignature();

        nextBlockId = _makeBlockId(blockNumber, blockHash);
    }

    function _hashTronBlockRawData(bytes calldata encodedBlock) private pure returns (bytes32 digest) {
        // Outer encoding is: 0x0a 0x69 || raw_data (105 bytes) || 0x12 0x41 || signature (65 bytes).
        // `_parseTronBlock` enforces the framing and total length.
        digest = sha256(encodedBlock[2:107]);
    }

    function _ecrecoverFromBlock(bytes32 digest, bytes calldata encodedBlock) private view returns (bytes20 signer) {
        // encodedBlock layout is fixed by `_parseTronBlock`:
        // signature at offsets 109..173 (65 bytes).
        bytes32 r;
        bytes32 s;
        uint8 v;

        // solhint-disable-next-line no-inline-assembly
        assembly {
            let base := encodedBlock.offset
            // signature starts at base + 109
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

            // [ptr..ptr+0x1f] hash
            // [ptr+0x20..ptr+0x3f] v (as uint256)
            // [ptr+0x40..ptr+0x5f] r
            // [ptr+0x60..ptr+0x7f] s
            mstore(ptr, digest)
            mstore(add(ptr, 0x20), v)
            mstore(add(ptr, 0x40), r)
            mstore(add(ptr, 0x60), s)

            // staticcall(gas, 0x01, in=ptr, insz=0x80, out=ptr, outsz=0x20)
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
