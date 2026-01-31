// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

/// @title TronSha256MerkleVerifier
/// @notice Library for verifying Merkle proofs for SHA256 Merkle trees used in Tron blockchain.
/// @author Ultrasound Labs
library TronSha256MerkleVerifier {
    /// @notice Verifies a Merkle proof for a SHA256 Merkle tree used in Tron blockchain.
    /// @param root   Merkle root (from block header txTrieRoot)
    /// @param leaf   Leaf hash = sha256(encodedTxBytes)
    /// @param proof  Sibling hashes from leaf level upwards
    /// @param index  Bitfield: bit i == 0 => path node was left child at level i,
    ///                                   1 => path node was right child at level i
    /// @return success Whether the proof is valid.
    function verify(bytes32 root, bytes32 leaf, bytes32[] memory proof, uint256 index) internal pure returns (bool) {
        bytes32 computed = leaf;

        for (uint256 i = 0; i < proof.length; ++i) {
            bytes32 sibling = proof[i];

            // Check bit i of index
            if ((index & (uint256(1) << i)) == 0) {
                // current node was LEFT child: parent = sha256(current || sibling)
                computed = sha256(abi.encodePacked(computed, sibling));
            } else {
                // current node was RIGHT child: parent = sha256(sibling || current)
                computed = sha256(abi.encodePacked(sibling, computed));
            }
        }

        return computed == root;
    }
}
