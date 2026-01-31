// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {TronSha256MerkleVerifier} from "./TronSha256MerkleVerifier.sol";
import {TronTxReaderErrors} from "../TronTxReaderErrors.sol";

/// @title TronTxInclusionVerifier
/// @notice Verifies Tron tx inclusion proofs against a block header txTrieRoot.
/// @author Ultrasound Labs
library TronTxInclusionVerifier {
    function verifyTxMerkleProof(bytes32 txTrieRoot, bytes calldata encodedTx, bytes32[] calldata proof, uint256 index)
        internal
        pure
    {
        if (!TronSha256MerkleVerifier.verify(txTrieRoot, sha256(encodedTx), proof, index)) {
            revert TronTxReaderErrors.InvalidTxMerkleProof();
        }
    }
}
