use super::grpc::TronGrpc;
use super::protocol::{BlockExtention, BlockHeader};
use alloy::primitives::{FixedBytes, U256};
use anyhow::{Context, Result};
use prost::Message;
use serde::Serialize;
use sha2::{Digest, Sha256};

pub struct TronTxProofBundle {
    pub blocks: [Vec<u8>; 20],
    pub encoded_tx: Vec<u8>,
    pub proof: Vec<FixedBytes<32>>,
    pub index: U256,
}

pub struct TronTxProofBuilder {
    /// Must be 19 for the hub's `bytes[20]` proof format.
    pub finality_blocks: u64,
}

#[derive(Debug, Serialize, Clone)]
struct TxTrieRootMismatchTxDump {
    index: usize,
    txid_ext: String,
    txid_from_raw_data: Option<String>,
    encoded_tx: String,
    encoded_tx_len: usize,
    tx_leaf: String,
}

#[derive(Debug, Serialize)]
struct TxTrieRootMismatchDump {
    tron_block_number: u64,
    txid: String,
    tx_index: usize,
    tx_count: usize,
    header_tx_trie_root: String,
    computed_tx_trie_root: String,
    computed_tx_trie_root_duplicate_last: String,
    index_bits: String,
    proof: Vec<String>,
    encoded_tx: String,
    encoded_tx_len: usize,
    leaves: Vec<String>,
    transactions: Vec<TxTrieRootMismatchTxDump>,
    block_header_raw_data: Option<String>,
    witness_signature: Option<String>,
}

impl TronTxProofBuilder {
    pub fn new(finality_blocks: u64) -> Self {
        Self { finality_blocks }
    }

    pub async fn build(&self, grpc: &mut TronGrpc, txid: [u8; 32]) -> Result<TronTxProofBundle> {
        if self.finality_blocks != 19 {
            anyhow::bail!(
                "unsupported Tron finality_blocks: expected 19, got {}",
                self.finality_blocks
            );
        }

        let tx_info = grpc
            .get_transaction_info_by_id(txid)
            .await
            .context("get tx info")?;
        let tron_block_number =
            u64::try_from(tx_info.block_number).context("Tron tx blockNumber out of range")?;

        let head = tron_head_block(grpc).await?;
        if head < tron_block_number + self.finality_blocks {
            anyhow::bail!(
                "tx not finalized: head={}, tx_block={}, need >= {}",
                head,
                tron_block_number,
                tron_block_number + self.finality_blocks
            );
        }

        // Fetch the tx block (with tx list) first, preserving canonical tx bytes for txTrieRoot.
        let (tx_block, tx_bytes) = grpc
            .get_block_by_num2_raw_txs(i64::try_from(tron_block_number)?)
            .await
            .context("get tx block (raw tx bytes)")?;

        let header_raw = tx_block
            .block_header
            .as_ref()
            .and_then(|h| h.raw_data.as_ref())
            .context("missing block_header.raw_data")?;
        let header_tx_trie_root = header_raw.tx_trie_root.clone();

        let details = compute_proof_from_block_ext(&tx_block, &tx_bytes, tron_block_number, txid)
            .context("tx proof")?;

        if details.computed_root.as_slice() != header_tx_trie_root.as_slice() {
            anyhow::bail!(
                "computed txTrieRoot mismatch\n{}",
                serde_json::to_string_pretty(&details.dump(header_raw, &tx_block.block_header))
                    .unwrap_or_else(|e| format!("{{\"error\":\"failed to serialize dump: {e}\"}}"))
            );
        }

        // Fetch 19 blocks after, for the hub's stateful Tron reader.
        let mut blocks: [Vec<u8>; 20] = std::array::from_fn(|_| Vec::new());
        blocks[0] = encode_block_header(&tx_block.block_header)?;
        for (i, block) in blocks.iter_mut().enumerate().skip(1) {
            let num = tron_block_number + (i as u64);
            let b = grpc
                .get_block_by_num2(i64::try_from(num)?)
                .await
                .with_context(|| format!("get block {num}"))?;
            *block = encode_block_header(&b.block_header)?;
        }

        Ok(TronTxProofBundle {
            blocks,
            encoded_tx: details.encoded_tx,
            proof: details.proof,
            index: details.index_bits,
        })
    }
}

struct ProofDetails {
    tron_block_number: u64,
    txid: [u8; 32],
    tx_index: usize,
    tx_count: usize,
    encoded_tx: Vec<u8>,
    proof: Vec<FixedBytes<32>>,
    index_bits: U256,
    computed_root: FixedBytes<32>,
    leaves: Vec<FixedBytes<32>>,
    tx_dumps: Vec<TxTrieRootMismatchTxDump>,
}

impl ProofDetails {
    fn dump(
        &self,
        header_raw: &crate::protocol::block_header::Raw,
        header: &Option<BlockHeader>,
    ) -> TxTrieRootMismatchDump {
        let header_tx_trie_root = header_raw.tx_trie_root.clone();
        let computed_root_dup_last = merkle_root_sha256_duplicate_last(&self.leaves);
        let header_raw_hex = Some(format!(
            "0x{}",
            hex::encode(header_raw.encode_to_vec().as_slice())
        ));
        let witness_sig_hex = header
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h.witness_signature.as_slice())));

        TxTrieRootMismatchDump {
            tron_block_number: self.tron_block_number,
            txid: format!("0x{}", hex::encode(self.txid)),
            tx_index: self.tx_index,
            tx_count: self.tx_count,
            header_tx_trie_root: format!("0x{}", hex::encode(&header_tx_trie_root)),
            computed_tx_trie_root: format!("0x{}", hex::encode(self.computed_root)),
            computed_tx_trie_root_duplicate_last: format!(
                "0x{}",
                hex::encode(computed_root_dup_last)
            ),
            index_bits: self.index_bits.to_string(),
            proof: self
                .proof
                .iter()
                .map(|h| format!("0x{}", hex::encode(h)))
                .collect(),
            encoded_tx: format!("0x{}", hex::encode(&self.encoded_tx)),
            encoded_tx_len: self.encoded_tx.len(),
            leaves: self
                .leaves
                .iter()
                .map(|h| format!("0x{}", hex::encode(h)))
                .collect(),
            transactions: self.tx_dumps.clone(),
            block_header_raw_data: header_raw_hex,
            witness_signature: witness_sig_hex,
        }
    }
}

fn compute_proof_from_block_ext(
    b: &BlockExtention,
    tx_bytes: &[Vec<u8>],
    tron_block_number: u64,
    txid: [u8; 32],
) -> Result<ProofDetails> {
    if tx_bytes.len() != b.transactions.len() {
        anyhow::bail!(
            "tx_bytes length mismatch: tx_bytes={} b.transactions={}",
            tx_bytes.len(),
            b.transactions.len()
        );
    }
    let mut leaves: Vec<FixedBytes<32>> = Vec::with_capacity(b.transactions.len());
    let mut tx_dumps: Vec<TxTrieRootMismatchTxDump> = Vec::with_capacity(b.transactions.len());
    let mut tx_index = None;
    let mut encoded_tx = None;

    for (idx, txe) in b.transactions.iter().enumerate() {
        let enc = tx_bytes[idx].clone();
        let tx = crate::protocol::Transaction::decode(enc.as_slice()).ok();
        let leaf = sha256_bytes32(&enc);
        leaves.push(leaf);

        let txid_ext = format!("0x{}", hex::encode(txe.txid.as_slice()));
        let txid_from_raw_data = tx.as_ref().and_then(|tx| tx.raw_data.as_ref()).map(|raw| {
            let raw_bytes = raw.encode_to_vec();
            format!("0x{}", hex::encode(sha256_bytes32(&raw_bytes)))
        });

        tx_dumps.push(TxTrieRootMismatchTxDump {
            index: idx,
            txid_ext,
            txid_from_raw_data,
            encoded_tx: format!("0x{}", hex::encode(&enc)),
            encoded_tx_len: enc.len(),
            tx_leaf: format!("0x{}", hex::encode(leaf)),
        });

        if txe.txid.as_slice() == txid {
            tx_index = Some(idx);
            encoded_tx = Some(enc);
        }
    }

    let tx_index = tx_index.context("tx not found in block tx list")?;
    let encoded_tx = encoded_tx.context("missing encoded tx bytes after tx match")?;

    let (proof, index_bits, computed_root) = merkle_proof_sha256(&leaves, tx_index)?;
    Ok(ProofDetails {
        tron_block_number,
        txid,
        tx_index,
        tx_count: leaves.len(),
        encoded_tx,
        proof,
        index_bits,
        computed_root,
        leaves,
        tx_dumps,
    })
}

async fn tron_head_block(grpc: &mut TronGrpc) -> Result<u64> {
    let b = grpc.get_now_block2().await.context("get now block")?;
    let raw = b
        .block_header
        .as_ref()
        .and_then(|h| h.raw_data.as_ref())
        .context("missing now block header.raw_data")?;
    u64::try_from(raw.number).context("now block number out of range")
}

fn encode_block_header(h: &Option<BlockHeader>) -> Result<Vec<u8>> {
    let h = h.as_ref().context("missing block_header")?;
    encode_block_header_stateful(h)
}

fn sha256_bytes32(bytes: &[u8]) -> FixedBytes<32> {
    let digest = Sha256::digest(bytes);
    FixedBytes::from_slice(&digest)
}

/// Encode a Tron `BlockHeader` in the exact fixed format expected by `StatefulTronTxReader`.
///
/// The onchain verifier expects:
/// - total length 174 bytes,
/// - `raw_data` length 105 bytes (0x69),
/// - `witness_signature` length 65 bytes (0x41),
/// - and specific field ordering with current-era varint lengths:
///   - timestamp (ms): 6 bytes varint
///   - block number: 4 bytes varint
///
/// If any of these assumptions are violated (future-proofing / different Tron network),
/// we fail fast so the relayer doesn't submit transactions that are guaranteed to revert.
fn encode_block_header_stateful(h: &BlockHeader) -> Result<Vec<u8>> {
    let raw = h
        .raw_data
        .as_ref()
        .context("missing block_header.raw_data")?;

    let ts_ms = u64::try_from(raw.timestamp).context("header timestamp out of range")?;
    let number = u64::try_from(raw.number).context("header number out of range")?;
    let version = u64::try_from(raw.version).context("header version out of range")?;

    let tx_trie_root = raw.tx_trie_root.as_slice();
    if tx_trie_root.len() != 32 {
        anyhow::bail!("unexpected txTrieRoot length: {}", tx_trie_root.len());
    }
    let parent_hash = raw.parent_hash.as_slice();
    if parent_hash.len() != 32 {
        anyhow::bail!("unexpected parentHash length: {}", parent_hash.len());
    }

    let witness_addr = raw.witness_address.as_slice();
    if witness_addr.len() != 21 {
        anyhow::bail!("unexpected witness_address length: {}", witness_addr.len());
    }

    let sig = h.witness_signature.as_slice();
    if sig.len() != 65 {
        anyhow::bail!("unexpected witness_signature length: {}", sig.len());
    }

    // Build the raw_data message (105 bytes).
    let ts_var = encode_varint_fixed(ts_ms, 6)?;
    let num_var = encode_varint_fixed(number, 4)?;
    let ver_var = encode_varint_fixed(version, 1)?;

    let mut raw_data = Vec::with_capacity(105);
    raw_data.push(0x08); // field 1 (timestamp), wire 0
    raw_data.extend_from_slice(&ts_var);
    raw_data.push(0x12); // field 2 (txTrieRoot), wire 2
    raw_data.push(0x20); // length 32
    raw_data.extend_from_slice(tx_trie_root);
    raw_data.push(0x1a); // field 3 (parentHash), wire 2
    raw_data.push(0x20); // length 32
    raw_data.extend_from_slice(parent_hash);
    raw_data.push(0x38); // field 7 (number), wire 0
    raw_data.extend_from_slice(&num_var);
    raw_data.push(0x4a); // field 9 (witness_address), wire 2
    raw_data.push(0x15); // length 21
    raw_data.extend_from_slice(witness_addr);
    raw_data.push(0x50); // field 10 (version), wire 0
    raw_data.extend_from_slice(&ver_var);

    if raw_data.len() != 105 {
        anyhow::bail!("unexpected raw_data length: {}", raw_data.len());
    }

    // Wrap in BlockHeader framing.
    let mut out = Vec::with_capacity(174);
    out.push(0x0a); // field 1 (raw_data), wire 2
    out.push(0x69); // length 105
    out.extend_from_slice(&raw_data);
    out.push(0x12); // field 2 (witness_signature), wire 2
    out.push(0x41); // length 65
    out.extend_from_slice(sig);

    if out.len() != 174 {
        anyhow::bail!("unexpected encoded header length: {}", out.len());
    }
    Ok(out)
}

fn encode_varint_fixed(mut v: u64, expected_len: usize) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    while v >= 0x80 {
        out.push(((v as u8) & 0x7f) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);

    if out.len() < expected_len {
        // Protobuf varints are typically encoded in their shortest form, but decoders accept
        // non-canonical encodings with additional zero-value continuation groups. We use this
        // to produce fixed-size encodings needed by the hub's packed block header format.
        //
        // Example: 1 => [0x01] (canonical) can be padded to length 4 as:
        // [0x81, 0x80, 0x80, 0x00], which still decodes to 1.
        let mut padded = out;
        let last = padded.len().saturating_sub(1);
        padded[last] |= 0x80;
        while padded.len() < expected_len - 1 {
            padded.push(0x80);
        }
        padded.push(0x00);
        out = padded;
    }

    if out.len() != expected_len {
        anyhow::bail!(
            "unexpected varint length: got {}, expected {}",
            out.len(),
            expected_len
        );
    }
    Ok(out)
}

fn merkle_proof_sha256(
    leaves: &[FixedBytes<32>],
    leaf_index: usize,
) -> Result<(Vec<FixedBytes<32>>, U256, FixedBytes<32>)> {
    if leaves.is_empty() {
        anyhow::bail!("empty merkle tree");
    }
    if leaf_index >= leaves.len() {
        anyhow::bail!("leaf index out of range");
    }

    let mut idx = leaf_index;
    let mut level = leaves.to_vec();
    let mut proof = Vec::new();
    let mut index_bits = U256::ZERO;
    let mut bit = 0u32;

    while level.len() > 1 {
        // Tron txTrieRoot uses a "carry-up" Merkle tree:
        // when a level has an odd count, the final node is promoted unchanged to the next level
        // (it is NOT duplicated and hashed with itself).
        //
        // That means a path can "skip" a level with no sibling/hash step.
        let has_no_sibling = (level.len() & 1) == 1 && idx == level.len() - 1;
        if !has_no_sibling {
            let is_right = (idx & 1) == 1;
            if is_right {
                index_bits |= U256::from(1u64) << bit;
            }

            let sibling_idx = if is_right { idx - 1 } else { idx + 1 };
            let sibling = level[sibling_idx];
            proof.push(sibling);
            bit += 1;
        }

        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        for j in (0..level.len()).step_by(2) {
            let left = level[j];
            let right = if j + 1 < level.len() {
                level[j + 1]
            } else {
                // Promote last node when odd.
                next.push(left);
                break;
            };
            next.push(sha256_concat(left, right));
        }

        idx /= 2;
        level = next;
    }

    Ok((proof, index_bits, level[0]))
}

fn sha256_concat(a: FixedBytes<32>, b: FixedBytes<32>) -> FixedBytes<32> {
    let mut hasher = Sha256::new();
    hasher.update(a.as_slice());
    hasher.update(b.as_slice());
    let digest = hasher.finalize();
    FixedBytes::from_slice(&digest)
}

fn merkle_root_sha256_duplicate_last(leaves: &[FixedBytes<32>]) -> FixedBytes<32> {
    if leaves.is_empty() {
        return FixedBytes::from([0u8; 32]);
    }

    let mut level = leaves.to_vec();
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        for j in (0..level.len()).step_by(2) {
            let left = level[j];
            let right = level.get(j + 1).copied().unwrap_or(left);
            next.push(sha256_concat(left, right));
        }
        level = next;
    }
    level[0]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address::TronAddress;
    use crate::grpc::extract_block_extention_transaction_bytes;
    use crate::protocol::BlockHeader;
    use k256::ecdsa::signature::DigestVerifier;
    use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
    use prost::Message;
    use serde::Deserialize;
    use std::path::PathBuf;

    fn workspace_path(rel: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
    }

    fn decode_hex0x(s: &str) -> Vec<u8> {
        let s = s.strip_prefix("0x").unwrap_or(s);
        hex::decode(s).expect("valid hex")
    }

    #[derive(Deserialize)]
    struct TronHeadersSampleFixture {
        #[allow(dead_code)]
        network: String,
        #[serde(rename = "indices")]
        #[allow(dead_code)]
        indices: Vec<usize>,
        #[serde(rename = "blockNumbers")]
        #[allow(dead_code)]
        block_numbers: Vec<String>,
        #[serde(rename = "blockHeaderRawBytes")]
        block_header_raw_bytes: Vec<String>,
        #[serde(rename = "witnessSignatures")]
        witness_signatures: Vec<String>,
    }

    fn load_tron_headers_sample_fixture() -> TronHeadersSampleFixture {
        let path = workspace_path("testdata/fixtures/tron_headers_78000000_78000099.sample.json");
        let json = std::fs::read_to_string(path).expect("read tron fixture json");
        serde_json::from_str(&json).expect("parse tron fixture json")
    }

    #[derive(Deserialize)]
    struct Trc20TxSampleFixture {
        #[serde(rename = "txId")]
        tx_id: String,
        #[serde(rename = "txLeaf")]
        tx_leaf: String,
        #[serde(rename = "encodedTx")]
        encoded_tx: String,
        #[serde(rename = "tronTokenEvm")]
        tron_token_evm: String,
        #[serde(rename = "fromTron")]
        from_tron: String,
        #[serde(rename = "toTron")]
        to_tron: String,
        #[serde(rename = "amount")]
        amount: String,
        #[serde(rename = "isTransferFrom")]
        is_transfer_from: bool,
        #[serde(rename = "success")]
        success: bool,
        #[serde(rename = "selector")]
        selector: String,
    }

    #[derive(Deserialize)]
    struct Trc20TxSampleFile {
        #[allow(dead_code)]
        network: String,
        #[allow(dead_code)]
        #[serde(rename = "blockNumber")]
        block_number: String,
        #[serde(rename = "tx")]
        tx: Trc20TxSampleFixture,
    }

    fn load_trc20_sample_fixture() -> Trc20TxSampleFile {
        let path = workspace_path("testdata/fixtures/trc20_tx_78115149.sample.json");
        let json = std::fs::read_to_string(path).expect("read trc20 fixture json");
        serde_json::from_str(&json).expect("parse trc20 fixture json")
    }

    #[test]
    fn encode_varint_fixed_pads_short_values() {
        fn decode_varint(mut buf: &[u8]) -> u64 {
            let mut out = 0u64;
            let mut shift = 0u32;
            loop {
                let b = buf[0];
                buf = &buf[1..];
                out |= u64::from(b & 0x7f) << shift;
                if b & 0x80 == 0 {
                    return out;
                }
                shift += 7;
            }
        }

        let enc = encode_varint_fixed(0, 6).expect("encode");
        assert_eq!(enc.len(), 6);
        assert_eq!(decode_varint(&enc), 0);

        let enc = encode_varint_fixed(1, 4).expect("encode");
        assert_eq!(enc.len(), 4);
        assert_eq!(decode_varint(&enc), 1);

        let enc = encode_varint_fixed(300, 5).expect("encode");
        assert_eq!(enc.len(), 5);
        assert_eq!(decode_varint(&enc), 300);
    }

    #[test]
    fn encode_varint_fixed_errors_if_value_needs_more_bytes() {
        let err = encode_varint_fixed(300, 1).unwrap_err().to_string();
        assert!(err.contains("unexpected varint length"));
    }

    #[test]
    fn merkle_proof_roundtrips_root_with_index_bits() {
        let leaves = vec![
            sha256_bytes32(b"a"),
            sha256_bytes32(b"b"),
            sha256_bytes32(b"c"),
        ];

        for leaf_index in 0..leaves.len() {
            let (proof, index_bits, root) = merkle_proof_sha256(&leaves, leaf_index).unwrap();
            let expected_len = match leaf_index {
                2 => 1,
                _ => 2,
            };
            assert_eq!(proof.len(), expected_len);

            let mut cur = leaves[leaf_index];
            for (bit, sib) in proof.into_iter().enumerate() {
                let is_right = ((index_bits >> bit) & U256::from(1u64)) == U256::from(1u64);
                cur = if is_right {
                    sha256_concat(sib, cur)
                } else {
                    sha256_concat(cur, sib)
                };
            }

            assert_eq!(cur, root);
        }
    }

    #[test]
    fn merkle_proof_carry_up_skips_unpaired_levels() {
        let leaves = vec![
            sha256_bytes32(b"a"),
            sha256_bytes32(b"b"),
            sha256_bytes32(b"c"),
            sha256_bytes32(b"d"),
            sha256_bytes32(b"e"),
        ];

        // The last leaf in an odd-sized tree is promoted twice (5 -> 3 -> 2),
        // so it only has a single hashing step at the final level (2 -> 1).
        let (proof, index_bits, root) = merkle_proof_sha256(&leaves, 4).unwrap();
        assert_eq!(proof.len(), 1);

        let mut cur = leaves[4];
        for (bit, sib) in proof.into_iter().enumerate() {
            let is_right = ((index_bits >> bit) & U256::from(1u64)) == U256::from(1u64);
            cur = if is_right {
                sha256_concat(sib, cur)
            } else {
                sha256_concat(cur, sib)
            };
        }
        assert_eq!(cur, root);
    }

    #[test]
    fn encode_block_header_stateful_has_fixed_layout() {
        let ts_ms: u64 = 1u64 << 35;
        let number: u64 = 1u64 << 21;

        let mut tx_trie_root = vec![0u8; 32];
        tx_trie_root[0] = 1;
        let mut parent_hash = vec![0u8; 32];
        parent_hash[0] = 2;

        let mut witness_address = vec![0u8; 21];
        witness_address[0] = TronAddress::MAINNET_PREFIX;
        witness_address[1..].copy_from_slice(&[0x33u8; 20]);

        let raw = crate::protocol::block_header::Raw {
            timestamp: i64::try_from(ts_ms).unwrap(),
            tx_trie_root,
            parent_hash,
            number: i64::try_from(number).unwrap(),
            witness_id: 0,
            witness_address,
            version: 1,
            account_state_root: Vec::new(),
        };

        let header = BlockHeader {
            raw_data: Some(raw),
            witness_signature: vec![0x44u8; 65],
        };

        let out = encode_block_header_stateful(&header).unwrap();
        assert_eq!(out.len(), 174);
        assert_eq!(out[0], 0x0a);
        assert_eq!(out[1], 0x69);
        assert_eq!(out[107], 0x12);
        assert_eq!(out[108], 0x41);
        assert_eq!(&out[109..], vec![0x44u8; 65].as_slice());
    }

    #[test]
    fn encode_block_header_stateful_matches_real_mainnet_fixture_bytes() {
        let fixture = load_tron_headers_sample_fixture();
        assert_eq!(
            fixture.block_header_raw_bytes.len(),
            fixture.witness_signatures.len()
        );
        assert!(!fixture.block_header_raw_bytes.is_empty());

        for i in 0..fixture.block_header_raw_bytes.len() {
            let raw_bytes = decode_hex0x(&fixture.block_header_raw_bytes[i]);
            let sig_bytes = decode_hex0x(&fixture.witness_signatures[i]);
            assert_eq!(sig_bytes.len(), 65, "fixture signature len mismatch at {i}");

            // These fixtures are what Solidity uses in `StatefulTronTxReader` tests:
            // `abi.encodePacked(0x0a69, raw, 0x1241, sig)`.
            let raw = crate::protocol::block_header::Raw::decode(raw_bytes.as_slice())
                .expect("decode BlockHeader_raw");
            let header = BlockHeader {
                raw_data: Some(raw),
                witness_signature: sig_bytes.clone(),
            };

            let encoded =
                encode_block_header_stateful(&header).expect("encode_block_header_stateful");

            let mut expected = Vec::with_capacity(174);
            expected.extend_from_slice(&[0x0a, 0x69]);
            expected.extend_from_slice(&raw_bytes);
            expected.extend_from_slice(&[0x12, 0x41]);
            expected.extend_from_slice(&sig_bytes);

            assert_eq!(
                encoded, expected,
                "encoded header mismatch at fixture index {i}"
            );
        }
    }

    #[test]
    fn tx_id_and_leaf_match_real_mainnet_fixture() {
        let fixture = load_trc20_sample_fixture();
        let tx0 = fixture.tx;

        let encoded_tx_bytes = decode_hex0x(&tx0.encoded_tx);
        let expected_leaf = FixedBytes::<32>::from_slice(&decode_hex0x(&tx0.tx_leaf));
        let expected_txid = FixedBytes::<32>::from_slice(&decode_hex0x(&tx0.tx_id));

        // txLeaf = sha256(encodedTxBytes)
        let leaf = sha256_bytes32(&encoded_tx_bytes);
        assert_eq!(leaf, expected_leaf, "txLeaf mismatch");

        // txId = sha256(Transaction.raw_data bytes)
        let tx = crate::protocol::Transaction::decode(encoded_tx_bytes.as_slice())
            .expect("decode Transaction");
        // Ensure our prost encoding matches the fixture bytes (cross-language stability check).
        assert_eq!(
            tx.encode_to_vec(),
            encoded_tx_bytes,
            "Transaction encoding mismatch"
        );

        let raw = tx.raw_data.expect("fixture tx has raw_data");
        let raw_bytes = raw.encode_to_vec();
        let txid = sha256_bytes32(&raw_bytes);
        assert_eq!(txid, expected_txid, "txId mismatch");
    }

    #[test]
    fn trigger_smart_contract_elements_match_trc20_fixture() {
        let fixture = load_trc20_sample_fixture();
        let tx0 = fixture.tx;

        let encoded_tx_bytes = decode_hex0x(&tx0.encoded_tx);
        let tx = crate::protocol::Transaction::decode(encoded_tx_bytes.as_slice())
            .expect("decode Transaction");

        let ret0 = tx.ret.first().cloned().unwrap_or_default();
        let success = ret0.ret == 0 && ret0.contract_ret == 1;
        assert_eq!(success, tx0.success, "success mismatch");

        let raw = tx.raw_data.expect("fixture tx has raw_data");
        assert_eq!(raw.contract.len(), 1, "expected exactly 1 contract");
        let contract0 = raw.contract.first().expect("contract[0]");

        // TriggerSmartContract = 31
        assert_eq!(contract0.r#type, 31, "unexpected contract type");

        let any = contract0
            .parameter
            .as_ref()
            .expect("missing contract parameter");
        let trigger = crate::protocol::TriggerSmartContract::decode(any.value.as_slice())
            .expect("decode TriggerSmartContract");

        let expected_owner = decode_hex0x(&tx0.from_tron);
        assert_eq!(
            trigger.owner_address, expected_owner,
            "owner_address mismatch"
        );

        let token_evm = decode_hex0x(&tx0.tron_token_evm);
        assert_eq!(token_evm.len(), 20, "tronTokenEvm must be 20 bytes");
        let mut expected_contract = vec![0x41u8];
        expected_contract.extend_from_slice(&token_evm);
        assert_eq!(
            trigger.contract_address, expected_contract,
            "contract_address mismatch"
        );

        let expected_selector = decode_hex0x(&tx0.selector);
        assert_eq!(expected_selector.len(), 4, "selector must be 4 bytes");
        assert!(
            trigger.data.len() >= 4,
            "expected TriggerSmartContract.data to have at least 4 bytes"
        );
        assert_eq!(
            &trigger.data[..4],
            expected_selector.as_slice(),
            "selector mismatch"
        );

        // Basic sanity: the fixture "toTron" appears in calldata for transfer(address,uint256).
        let expected_to = decode_hex0x(&tx0.to_tron);
        assert_eq!(expected_to.len(), 21, "toTron must be 21 bytes");
        let to20 = &expected_to[1..];
        assert!(
            trigger.data.windows(20).any(|w| w == to20),
            "toTron (20-byte) not found in calldata"
        );

        // The amount is ABI-encoded as uint256 big-endian; ensure it appears as a 32-byte word.
        let amount: u128 = tx0.amount.parse().expect("amount fits u128");
        let mut amount_word = [0u8; 32];
        amount_word[16..].copy_from_slice(&amount.to_be_bytes());
        assert!(
            trigger.data.windows(32).any(|w| w == amount_word),
            "amount word not found in calldata"
        );

        assert!(
            !tx0.is_transfer_from,
            "fixture expects transfer (not transferFrom)"
        );
    }

    #[test]
    fn tx_signature_verifies_over_raw_data_not_full_transaction_bytes() {
        let fixture = load_trc20_sample_fixture();
        let tx0 = fixture.tx;

        let encoded_tx_bytes = decode_hex0x(&tx0.encoded_tx);
        let tx = crate::protocol::Transaction::decode(encoded_tx_bytes.as_slice())
            .expect("decode Transaction");

        let sig_bytes = tx.signature.first().expect("fixture tx has signature[0]");
        assert_eq!(sig_bytes.len(), 65, "expected 65-byte tx signature");

        let sig = Signature::try_from(&sig_bytes[..64]).expect("parse r||s signature");
        let mut v = sig_bytes[64];
        // Normalize 27/28 -> 0/1 if needed.
        if v >= 27 {
            v -= 27;
        }
        let recid = RecoveryId::try_from(v).expect("valid recovery id");

        let raw = tx.raw_data.expect("fixture tx has raw_data");
        let raw_bytes = raw.encode_to_vec();
        let digest = Sha256::new_with_prefix(&raw_bytes);

        let vk = VerifyingKey::recover_from_digest(digest.clone(), &sig, recid)
            .expect("recover verifying key from signature");
        vk.verify_digest(digest, &sig)
            .expect("signature verifies over sha256(raw_data)");

        // Safety check: signature should not verify over the full Transaction bytes.
        let wrong = vk.verify_digest(Sha256::new_with_prefix(&encoded_tx_bytes), &sig);
        assert!(
            wrong.is_err(),
            "signature unexpectedly verified over full tx bytes"
        );
    }

    #[derive(Deserialize)]
    struct TronTxProofCostFixture {
        #[serde(rename = "energyFeeSunPerEnergy")]
        energy_fee_sun_per_energy: Option<String>,
        #[serde(rename = "txFeeSunPerByte")]
        tx_fee_sun_per_byte: Option<String>,
        #[serde(rename = "energyRequired")]
        energy_required: Option<String>,
        #[serde(rename = "txSizeBytes")]
        tx_size_bytes: Option<u64>,
        #[serde(rename = "computedFeeLimitSun")]
        computed_fee_limit_sun: Option<String>,
        #[serde(rename = "accountResource")]
        #[allow(dead_code)]
        account_resource: Option<serde_json::Value>,
    }

    #[derive(Deserialize)]
    struct TronTxProofFixture {
        #[allow(dead_code)]
        network: String,
        #[serde(rename = "blockNumber")]
        #[allow(dead_code)]
        block_number: String,
        #[serde(rename = "txId")]
        tx_id: String,
        #[serde(rename = "targetIndex")]
        target_index: usize,
        #[serde(rename = "encodedTx")]
        encoded_tx: String,
        #[serde(rename = "txLeaf")]
        tx_leaf: String,
        #[serde(rename = "headerTxTrieRoot")]
        header_tx_trie_root: String,
        #[serde(rename = "leaves")]
        leaves: Vec<String>,
        #[serde(rename = "proof")]
        proof: Vec<String>,
        #[serde(rename = "indexBits")]
        index_bits: String,
        #[serde(rename = "blocks")]
        blocks: Vec<String>,
        #[serde(default)]
        #[serde(rename = "cost")]
        cost: Option<TronTxProofCostFixture>,
    }

    #[derive(Deserialize)]
    struct TronBlockExtentionRawFixture {
        #[serde(rename = "blockNumber")]
        block_number: String,
        #[serde(rename = "txId")]
        tx_id: String,
        #[serde(rename = "blockExtention")]
        block_extention: String,
        #[serde(rename = "expectedTxIndex")]
        expected_tx_index: Option<usize>,
    }

    #[test]
    fn tron_blockext_raw_fixture_tx_trie_root_matches_header_if_present() {
        let default_path = workspace_path(
            "testdata/fixtures/tron_blockext_raw_79072107_860981c986ef3c0313916283483e6e0f8614686b2c011c31d68d42e8efff6601.json",
        );
        let path = std::env::var("TRON_BLOCKEXT_RAW_FIXTURE")
            .ok()
            .map(PathBuf::from)
            .unwrap_or(default_path);

        if !path.exists() {
            eprintln!(
                "skipping: set TRON_BLOCKEXT_RAW_FIXTURE to a generated fixture path (missing {})",
                path.display()
            );
            return;
        }

        let json = std::fs::read_to_string(&path).expect("read blockext raw fixture json");
        let fixture: TronBlockExtentionRawFixture =
            serde_json::from_str(&json).expect("parse blockext raw fixture json");

        let block_number: u64 = fixture.block_number.parse().expect("blockNumber decimal");
        assert_eq!(block_number, 79072107, "unexpected fixture blockNumber");

        let block_bytes = decode_hex0x(&fixture.block_extention);
        let block = crate::protocol::BlockExtention::decode(block_bytes.as_slice())
            .expect("decode BlockExtention");

        let header_raw = block
            .block_header
            .as_ref()
            .and_then(|h| h.raw_data.as_ref())
            .expect("missing block_header.raw_data");
        let header_root = FixedBytes::<32>::from_slice(header_raw.tx_trie_root.as_slice());

        let tx_bytes =
            extract_block_extention_transaction_bytes(&block_bytes).expect("extract tx bytes");
        assert_eq!(
            tx_bytes.len(),
            block.transactions.len(),
            "tx byte count mismatch"
        );

        let txid = FixedBytes::<32>::from_slice(&decode_hex0x(&fixture.tx_id));
        let mut tx_index = None;
        for (i, txe) in block.transactions.iter().enumerate() {
            if txe.txid.as_slice() == txid.as_slice() {
                tx_index = Some(i);
                break;
            }
        }
        let tx_index = tx_index.expect("fixture txId not found in block.transactions");
        if let Some(expected) = fixture.expected_tx_index {
            assert_eq!(tx_index, expected, "unexpected tx index");
        }

        let leaves = tx_bytes
            .iter()
            .map(|b| sha256_bytes32(b))
            .collect::<Vec<_>>();
        let (_, _, root) = merkle_proof_sha256(&leaves, tx_index).expect("compute root");
        assert_eq!(
            root, header_root,
            "computed root != header txTrieRoot for fixture"
        );

        // Regression: the original incident was caused by a *different* tx within the same block
        // containing a high-tag unknown field inside Transaction.Result (e.g. 1002). If we were
        // to decode+re-encode that tx with `prost`, its bytes would change and the block root
        // would no longer match the header.
        //
        // From incident logs: tx index 187 carried the extra field and txCount was 619.
        assert_eq!(tx_bytes.len(), 619, "unexpected txCount for fixture block");
        let problematic_idx = 187usize;
        assert!(
            tx_bytes[problematic_idx]
                .windows(2)
                .any(|w| w == [0xd2, 0x3e]),
            "expected tx[187] bytes to contain field 1002 tag (0xd2 0x3e)"
        );

        let decoded_problem =
            crate::protocol::Transaction::decode(tx_bytes[problematic_idx].as_slice())
                .expect("decode problematic Transaction");
        let reencoded_problem = decoded_problem.encode_to_vec();
        assert_ne!(
            reencoded_problem, tx_bytes[problematic_idx],
            "expected problematic tx to change on decode->encode (unknown field drop)"
        );

        let mut reencoded_txs = tx_bytes.clone();
        reencoded_txs[problematic_idx] = reencoded_problem;
        let reencoded_leaves = reencoded_txs
            .iter()
            .map(|b| sha256_bytes32(b))
            .collect::<Vec<_>>();
        let (_, _, reencoded_root) =
            merkle_proof_sha256(&reencoded_leaves, tx_index).expect("compute root (re-encoded)");
        assert_ne!(
            reencoded_root, header_root,
            "expected txTrieRoot mismatch if a tx is decode->encoded with prost"
        );
    }

    #[test]
    fn tron_tx_proof_fixture_end_to_end_if_present() {
        let default_path = workspace_path(
            "testdata/fixtures/tron_tx_proof_78812179_1d649769f0ecf78bd6812226d067144bca18b4d01fb34cfdb260fd51cc3072db.json",
        );

        let path = std::env::var("TRON_TX_PROOF_FIXTURE")
            .ok()
            .map(PathBuf::from)
            .unwrap_or(default_path);

        if !path.exists() {
            eprintln!(
                "skipping: set TRON_TX_PROOF_FIXTURE to a generated fixture path (missing {})",
                path.display()
            );
            return;
        }

        let json = std::fs::read_to_string(&path).expect("read tx proof fixture json");
        let fixture: TronTxProofFixture =
            serde_json::from_str(&json).expect("parse tx proof fixture json");

        // Basic tx invariants.
        let encoded_tx_bytes = decode_hex0x(&fixture.encoded_tx);
        let expected_leaf = FixedBytes::<32>::from_slice(&decode_hex0x(&fixture.tx_leaf));
        let leaf = sha256_bytes32(&encoded_tx_bytes);
        assert_eq!(leaf, expected_leaf, "txLeaf mismatch");

        let tx = crate::protocol::Transaction::decode(encoded_tx_bytes.as_slice())
            .expect("decode Transaction");
        let raw = tx.raw_data.expect("fixture tx has raw_data");
        let raw_bytes = raw.encode_to_vec();
        let txid = sha256_bytes32(&raw_bytes);
        let expected_txid = FixedBytes::<32>::from_slice(&decode_hex0x(&fixture.tx_id));
        assert_eq!(txid, expected_txid, "txId mismatch");

        // Merkle proof invariants (recompute from leaves).
        let leaves = fixture
            .leaves
            .iter()
            .map(|h| FixedBytes::<32>::from_slice(&decode_hex0x(h)))
            .collect::<Vec<_>>();
        assert_eq!(
            leaves.get(fixture.target_index).copied(),
            Some(expected_leaf),
            "leaf at targetIndex mismatch"
        );

        let (proof, index_bits, root) = merkle_proof_sha256(&leaves, fixture.target_index).unwrap();
        let expected_root =
            FixedBytes::<32>::from_slice(&decode_hex0x(&fixture.header_tx_trie_root));
        assert_eq!(root, expected_root, "computed root != header txTrieRoot");

        let expected_proof = fixture
            .proof
            .iter()
            .map(|h| FixedBytes::<32>::from_slice(&decode_hex0x(h)))
            .collect::<Vec<_>>();
        assert_eq!(proof, expected_proof, "proof mismatch");

        let expected_index_bits_u256 =
            U256::from_str_radix(&fixture.index_bits, 10).expect("parse indexBits as decimal U256");
        assert_eq!(index_bits, expected_index_bits_u256, "indexBits mismatch");

        if let Some(cost) = fixture.cost {
            let energy_fee: u64 = cost
                .energy_fee_sun_per_energy
                .as_deref()
                .expect("fixture.cost.energyFeeSunPerEnergy missing")
                .parse()
                .expect("energyFeeSunPerEnergy decimal");
            let tx_fee: u64 = cost
                .tx_fee_sun_per_byte
                .as_deref()
                .expect("fixture.cost.txFeeSunPerByte missing")
                .parse()
                .expect("txFeeSunPerByte decimal");
            let energy_required: u64 = cost
                .energy_required
                .as_deref()
                .expect("fixture.cost.energyRequired missing")
                .parse()
                .expect("energyRequired decimal");
            let fees = crate::resources::ChainFees {
                energy_fee_sun_per_energy: energy_fee,
                tx_fee_sun_per_byte: tx_fee,
            };
            let tx_size = cost.tx_size_bytes.unwrap_or(encoded_tx_bytes.len() as u64);
            assert_eq!(
                tx_size,
                u64::try_from(encoded_tx_bytes.len()).unwrap(),
                "fixture txSizeBytes mismatch"
            );
            let computed = crate::resources::quote_fee_limit_sun(energy_required, tx_size, fees);
            if let Some(s) = cost.computed_fee_limit_sun {
                let expected_from_fixture: u64 = s.parse().expect("computedFeeLimitSun decimal");
                assert_eq!(
                    computed, expected_from_fixture,
                    "computedFeeLimitSun mismatch"
                );
            }
        }

        // Header encoding invariants: each block should roundtrip through our stateful encoder.
        assert_eq!(fixture.blocks.len(), 20, "expected 20 blocks");
        for (i, b) in fixture.blocks.iter().enumerate() {
            let block_bytes = decode_hex0x(b);
            assert_eq!(
                block_bytes.len(),
                174,
                "encoded block length mismatch at {i}"
            );
            assert_eq!(&block_bytes[..2], &[0x0a, 0x69], "prefix mismatch at {i}");
            assert_eq!(
                &block_bytes[107..109],
                &[0x12, 0x41],
                "sig framing mismatch at {i}"
            );

            let raw_bytes = &block_bytes[2..107];
            let sig_bytes = block_bytes[109..].to_vec();

            let raw = crate::protocol::block_header::Raw::decode(raw_bytes)
                .expect("decode BlockHeader_raw");
            let header = BlockHeader {
                raw_data: Some(raw),
                witness_signature: sig_bytes,
            };
            let re_encoded =
                encode_block_header_stateful(&header).expect("encode_block_header_stateful");
            assert_eq!(
                re_encoded, block_bytes,
                "block header encoding mismatch at {i}"
            );
        }
    }

    #[test]
    fn transaction_decode_encode_drops_unknown_fields() {
        // Regression: some nodes include high-tag fields (e.g. 1002) inside `Transaction.Result`.
        // `prost` drops unknown fields on decode, so decode->encode changes bytes.
        //
        // Our txTrieRoot code must therefore hash the *canonical tx bytes* extracted from the
        // outer response, not `tx.encode_to_vec()`.
        let tx_bytes = decode_hex0x(
            "0a93010a028b58220806e328e403f663c74098dfcfeeb9335a750839126f0a35747970652e676f6f676c65617069732e636f6d2f70726f746f636f6c2e44656c65676174655265736f75726365436f6e747261637412360a15416651c0e2ff4bb6306dedee3432178bc3440f6dd7100118c0ccf3c7332215414656aefbd8de0f2b0fa3fb7a4724079add283ce42803708790cceeb9331241f04bc4c12a3323d37ad1a57ab6eab4e0e258dbf34733d73ea7fec3d1a49e98f632d848c19fa36b4b0cf2de320f6f5adcd90696fb747c47b0a8d23363b6f6de50012a021801d23e0cc83e81f6a50cd03ed4b6a20f",
        );
        assert!(
            tx_bytes.windows(2).any(|w| w == [0xd2, 0x3e]),
            "expected tx bytes to contain field 1002 tag (0xd2 0x3e)"
        );

        let decoded =
            crate::protocol::Transaction::decode(tx_bytes.as_slice()).expect("decode Transaction");
        let reencoded = decoded.encode_to_vec();
        assert_ne!(
            reencoded, tx_bytes,
            "prost unexpectedly preserved unknown fields during Transaction decode->encode"
        );
        assert!(
            !reencoded.windows(2).any(|w| w == [0xd2, 0x3e]),
            "re-encoded tx unexpectedly still contains field 1002 tag"
        );
    }

    #[test]
    fn compute_proof_uses_canonical_tx_bytes_without_reencoding() {
        let tx_bytes = decode_hex0x(
            "0a93010a028b58220806e328e403f663c74098dfcfeeb9335a750839126f0a35747970652e676f6f676c65617069732e636f6d2f70726f746f636f6c2e44656c65676174655265736f75726365436f6e747261637412360a15416651c0e2ff4bb6306dedee3432178bc3440f6dd7100118c0ccf3c7332215414656aefbd8de0f2b0fa3fb7a4724079add283ce42803708790cceeb9331241f04bc4c12a3323d37ad1a57ab6eab4e0e258dbf34733d73ea7fec3d1a49e98f632d848c19fa36b4b0cf2de320f6f5adcd90696fb747c47b0a8d23363b6f6de50012a021801d23e0cc83e81f6a50cd03ed4b6a20f",
        );
        let txid = [0u8; 32];

        let block = crate::protocol::BlockExtention {
            transactions: vec![crate::protocol::TransactionExtention {
                transaction: None,
                txid: txid.to_vec(),
                ..Default::default()
            }],
            ..Default::default()
        };

        let details =
            super::compute_proof_from_block_ext(&block, std::slice::from_ref(&tx_bytes), 1, txid)
                .unwrap();

        assert_eq!(details.tx_index, 0);
        assert_eq!(details.tx_count, 1);
        assert_eq!(details.encoded_tx, tx_bytes);
        assert!(
            details.encoded_tx.windows(2).any(|w| w == [0xd2, 0x3e]),
            "expected proof encoded_tx to contain field 1002 tag"
        );
    }
}
