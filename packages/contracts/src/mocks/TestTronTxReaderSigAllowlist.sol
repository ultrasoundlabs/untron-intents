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

/// @title TestTronTxReaderSigAllowlist
/// @notice Test-only Tron tx reader that verifies inclusion proofs and witness signatures against
///         a configured allowlist of valid signer EVM addresses.
/// @dev This is intended for private Tron networks where the SR owner->delegatee mapping is not
///      easily available, but we still want cryptographic signature validation (vs no-sig readers).
contract TestTronTxReaderSigAllowlist is ITronTxReader {
    mapping(bytes20 => bool) public isAllowedSigner;

    constructor(bytes20[] memory allowedSigners) {
        for (uint256 i = 0; i < allowedSigners.length; ++i) {
            isAllowedSigner[allowedSigners[i]] = true;
        }
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
        for (uint256 i = 0; i < blocks.length; ++i) {
            (uint32 ts, bytes32 root,, uint256 n,) = _parseTronBlock(blocks[i]);

            if (i == 0) {
                blockNumber = n;
                blockTimestamp = ts;
                txTrieRoot = root;
            }

            bytes32 blockHash = _hashTronBlockRawData(blocks[i]);
            bytes20 signer = _ecrecoverFromBlock(blockHash, blocks[i]);
            if (!isAllowedSigner[signer]) revert TronTxReaderErrors.InvalidWitnessSignature();
        }
    }

    function _hashTronBlockRawData(bytes calldata encodedBlock) private pure returns (bytes32 digest) {
        digest = sha256(encodedBlock[2:107]);
    }

    function _ecrecoverFromBlock(bytes32 digest, bytes calldata encodedBlock) private view returns (bytes20 signer) {
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
        returns (uint32 blockTimestamp, bytes32 txTrieRoot, bytes32 parentHash, uint256 number, bytes20 witnessAddress)
    {
        if (encodedBlock.length != 174) revert TronTxReaderErrors.InvalidEncodedBlockLength(encodedBlock.length);

        if (
            encodedBlock[0] != 0x0a || encodedBlock[1] != 0x69 || encodedBlock[107] != 0x12 || encodedBlock[108] != 0x41
        ) {
            revert TronTxReaderErrors.InvalidHeaderPrefix();
        }

        (uint256 tsMs,) = ProtoVarint.read(encodedBlock, 3, 3 + 6);
        uint256 tsSec = tsMs / 1000;
        if (tsSec > type(uint32).max) revert TronTxReaderErrors.TimestampOverflow();
        // forge-lint: disable-next-line(unsafe-typecast)
        blockTimestamp = uint32(tsSec);

        assembly {
            let base := encodedBlock.offset
            txTrieRoot := calldataload(add(base, 11))
            parentHash := calldataload(add(base, 45))
        }

        (number,) = ProtoVarint.read(encodedBlock, 78, 78 + 4);

        uint8 prefix = uint8(encodedBlock[84]);
        if (prefix != 0x41) revert TronTxReaderErrors.InvalidWitnessAddressPrefix(prefix);

        bytes32 tmp;
        assembly {
            tmp := calldataload(add(encodedBlock.offset, 85))
        }
        // forge-lint: disable-next-line(unsafe-typecast)
        witnessAddress = bytes20(tmp);
    }

    // Note: This test reader intentionally does not verify the 20-block sequence using `parentHash`,
    // because some private Tron networks do not populate the header's parent link with the canonical
    // blockId semantics expected on mainnet.
}
