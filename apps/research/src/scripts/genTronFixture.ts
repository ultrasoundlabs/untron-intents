import { writeFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import Long from "long";
import { z } from "zod";
import { parseEnv } from "../lib/env.js";
import { log } from "../lib/logger.js";
import { createTronClients } from "@untron/tron-protocol";
import { BlockHeader_raw } from "@untron/tron-protocol/tron";
import type { BlockExtention, NumberMessage } from "@untron/tron-protocol/api";
import { sha256 } from "@noble/hashes/sha2.js";
import { keccak_256 } from "@noble/hashes/sha3.js";
import * as secp256k1 from "@noble/secp256k1";
import type { Address, Hex } from "viem";

// Resolve paths relative to this file so we can reliably drop fixtures
// into the contracts package, regardless of the current working directory.
const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

// From: apps/research/src/scripts
// To:   packages/contracts/test/evm/fixtures
const TRON_FIXTURES_DIR = resolve(__dirname, "../../../../packages/contracts/test/evm/fixtures");

function toHex(bytes: Uint8Array | Buffer): string {
  return Buffer.from(bytes).toString("hex");
}

function toHex0x(bytes: Uint8Array | Buffer): Hex {
  return `0x${Buffer.from(bytes).toString("hex")}` as Hex;
}

function tronWitnessAddressToEvmAddress(bytes: Uint8Array | Buffer): Address | null {
  // Tron witness address is 21 bytes: 0x41 prefix + 20-byte EVM address
  if (bytes.length !== 21 || bytes[0] !== 0x41) return null;
  return `0x${Buffer.from(bytes.subarray(1)).toString("hex")}` as Address;
}

function publicKey64FromUncompressed(pub: Uint8Array): Uint8Array | null {
  if (pub.length === 65 && pub[0] === 0x04) return pub.subarray(1);
  if (pub.length === 64) return pub;
  return null;
}

function recoverUncompressedPublicKey(
  hash32: Uint8Array,
  witnessSignature: Buffer
): Uint8Array | null {
  if (!witnessSignature || witnessSignature.length < 65) return null;
  const r = witnessSignature.subarray(0, 32);
  const s = witnessSignature.subarray(32, 64);
  let recovery = Number(witnessSignature[64]! & 0xff);
  if (recovery >= 27) recovery -= 27; // normalize eth-style v (27/28) -> 0/1
  if (recovery < 0 || recovery > 3) return null;
  const sig65 = new Uint8Array(65);
  sig65[0] = recovery;
  sig65.set(r, 1);
  sig65.set(s, 33);
  try {
    const pub = secp256k1.recoverPublicKey(sig65, hash32, { prehash: false });
    if (!pub || pub.length === 0) return null;
    if (pub.length === 65) return pub; // uncompressed 0x04 || X || Y
    if (pub.length === 33) {
      const hex = Buffer.from(pub).toString("hex");
      return secp256k1.Point.fromHex(hex).toBytes(false);
    }
    return null;
  } catch {
    return null;
  }
}

function evmAddressFromUncompressed(pub: Uint8Array): Address {
  const pub64 = publicKey64FromUncompressed(pub);
  if (!pub64) {
    throw new Error("Invalid uncompressed public key length");
  }
  const hash = keccak_256(pub64);
  return `0x${Buffer.from(hash.subarray(12)).toString("hex")}` as Address;
}

function isStrictlySortedLex(addresses: Address[]): boolean {
  for (let i = 1; i < addresses.length; i++) {
    if (addresses[i - 1]!.toLowerCase() >= addresses[i]!.toLowerCase()) return false;
  }
  return true;
}

// Compute Tron-style blockId and related reference values for testing:
// - blockHash: sha256(BlockHeader_raw)
// - blockId:   uint64(blockNumber) || sha256(BlockHeader_raw)[8:]
function computeBlockId(raw: BlockHeader_raw): {
  blockId: Hex;
  blockNumber: bigint;
  blockHash: Hex;
  rawHeaderBytes: Hex;
} {
  const rawBytes = BlockHeader_raw.encode(raw).finish();
  const digest = sha256(rawBytes); // Uint8Array, 32 bytes

  const digestHex = toHex(digest);
  const digestBig = BigInt("0x" + digestHex);

  const numberBig = BigInt(raw.number.toString()); // Long (uint64) -> string -> bigint

  // For Tron blockId, the upper 8 bytes are the block number (uint64),
  // and the lower 24 bytes are the tail of sha256(BlockHeader_raw).
  const tailMask = (1n << 192n) - 1n;
  const tail = digestBig & tailMask;

  const blockIdBig = (numberBig << 192n) | tail;
  const blockIdHex = blockIdBig.toString(16).padStart(64, "0");

  return {
    blockId: ("0x" + blockIdHex) as Hex,
    blockNumber: numberBig,
    blockHash: toHex0x(digest),
    rawHeaderBytes: toHex0x(rawBytes),
  };
}

type Fixture = {
  network: "tron-mainnet";
  startBlock: string;
  endBlock: string;
  startingBlockId: Hex;
  startingBlockTxTrieRoot: Hex;
  // Timestamp (seconds) for the starting anchor block (parent of the first block in the range)
  startingBlockTimestamp: string;
  endingBlockId: Hex;
  endingBlockTxTrieRoot: Hex;
  // Timestamp (seconds) for the ending block in the range
  endingBlockTimestamp: string;
  // Super Representatives (SR) – Tron witness owner accounts that appear in BlockHeader_raw.witnessAddress.
  srs: Address[]; // always length 27, SR owner accounts
  compressedTronBlockMetadata: Hex;
  compressedSignatures: Hex;
  blockNumbers: string[];
  blockIds: Hex[];
  // Extra reference data for Layer 0 (encoding / hashing) tests.
  // These are per-block arrays aligned with blockNumbers/blockIds.
  blockHashes: Hex[]; // sha256(BlockHeader_raw)
  blockHeaderRawBytes: Hex[]; // raw BlockHeader_raw.encode(raw) bytes
  witnessEvmAddresses: Address[]; // per-block recovered signing key
  witnessIndices: number[]; // witness index written into compressedTronBlockMetadata
  witnessSignatures: Hex[]; // 65-byte [r|s|v] signatures as hex
  // Per-SR mapping from SR owner account (srs[i]) to its delegated signing key (if any).
  // These are the EVM addresses of the actual ECDSA keys used to sign blocks.
  witnessDelegatees: Address[]; // length 27
};

async function fetchBlock(wallet: any, callOpts: any, num: number): Promise<BlockExtention> {
  const req: NumberMessage = { num: Long.fromNumber(num, true) };
  return await new Promise((resolve, reject) => {
    wallet.getBlockByNum2(req, callOpts.metadata, (err: any, res: BlockExtention | null) => {
      if (err || !res) return reject(err ?? new Error("Empty response from getBlockByNum2"));
      resolve(res);
    });
  });
}

async function main() {
  const env = parseEnv(
    z.object({
      TRON_GRPC_HOST: z.string().min(1),
      TRON_API_KEY: z.string().optional(),
    })
  );

  const argv = process.argv.slice(2);

  // Support both direct invocation:
  //   tsx genTronFixture.ts <startBlock> <endBlock> [outPath]
  // and runner style:
  //   tsx src/run.ts genTronFixture <startBlock> <endBlock> [outPath]
  let argOffset = 0;
  if (argv.length > 0 && !/^[0-9]+$/.test(argv[0]!)) {
    // First arg is script name/path, not a block number.
    argOffset = 1;
  }

  const args = argv.slice(argOffset);
  if (args.length < 2 || args.length > 3) {
    // eslint-disable-next-line no-console
    console.error(
      "Usage: tsx genTronFixture.ts <startBlock> <endBlock> [outPath]\n" +
        "Or: tsx src/run.ts genTronFixture <startBlock> <endBlock> [outPath]\n" +
        "Example: tsx genTronFixture.ts 55000000 55000099 test/fixtures/tron_55000000_55000099.json"
    );
    process.exit(1);
  }

  const startBlock = Number(args[0]!);
  const endBlock = Number(args[1]!);
  if (!Number.isInteger(startBlock) || !Number.isInteger(endBlock) || startBlock > endBlock) {
    throw new Error("Invalid startBlock/endBlock");
  }

  const outPath = args[2]
    ? resolve(args[2])
    : resolve(TRON_FIXTURES_DIR, `tron_${startBlock}_${endBlock}.json`);

  const { wallet, callOpts } = createTronClients(env.TRON_GRPC_HOST, env.TRON_API_KEY, {
    insecure: true,
  });

  log.info("Generating Tron fixture", { startBlock, endBlock, outPath });

  const numBlocks = endBlock - startBlock + 1;
  const blocks: BlockExtention[] = [];

  // Fetch the parent of the first block so we can record its txTrieRoot for the starting anchor.
  const parentBlockNumber = startBlock - 1;
  if (parentBlockNumber < 0) {
    throw new Error("startBlock must be > 0 to fetch parent block");
  }
  const parentBlock = await fetchBlock(wallet, callOpts, parentBlockNumber);
  const parentRaw = parentBlock.blockHeader?.rawData as BlockHeader_raw | undefined;
  if (!parentRaw || !parentRaw.txTrieRoot) {
    throw new Error(`Parent block ${parentBlockNumber} missing header/rawData/txTrieRoot`);
  }

  for (let n = startBlock; n <= endBlock; n++) {
    const block = await fetchBlock(wallet, callOpts, n);
    if (!block.blockHeader || !block.blockHeader.rawData) {
      throw new Error(`Block ${n} missing header/rawData`);
    }
    blocks.push(block);
  }

  const firstRaw = blocks[0]!.blockHeader!.rawData!;
  // Tron header's parentHash field is the parent blockId (32 bytes).
  // We'll use this as startingBlockId in the fixture.
  const startingBlockId = toHex0x(firstRaw.parentHash);
  // Timestamp for the starting anchor (parent block) – convert from ms to seconds.
  const parentTimestampMs = BigInt(parentRaw.timestamp.toString());
  const parentTimestampSec = (parentTimestampMs / 1000n).toString();

  // We canonicalize SR indexing by SR owner address (witnessAddress), not by delegatee/signer.
  // This makes `witnessIndex` stable across schedules/JSON permutations.
  const ownerToDelegatee = new Map<Address, Address>();

  // Pre-allocate buffers
  const TRON_BLOCK_METADATA_SIZE = 69; // 32 + 32 + 4 + 1
  const SIGNATURE_SIZE = 65;

  const metadataBuf = Buffer.alloc(numBlocks * TRON_BLOCK_METADATA_SIZE);
  const sigsBuf = Buffer.alloc(numBlocks * SIGNATURE_SIZE);

  const blockNumbers: string[] = [];
  const blockIds: Hex[] = [];
  const blockHashes: Hex[] = [];
  const blockHeaderRawBytes: Hex[] = [];
  const witnessEvmAddresses: Address[] = [];
  const witnessIndices: number[] = [];
  const witnessSignatures: Hex[] = [];
  const witnessOwnersByBlock: Address[] = [];

  let metaOffset = 0;
  let sigOffset = 0;

  for (let i = 0; i < blocks.length; i++) {
    const header = blocks[i]!.blockHeader!;
    const raw = header.rawData!;

    const { blockId, blockNumber, blockHash, rawHeaderBytes } = computeBlockId(
      raw as BlockHeader_raw
    );
    blockNumbers.push(blockNumber.toString());
    blockIds.push(blockId);
    blockHashes.push(blockHash);
    blockHeaderRawBytes.push(rawHeaderBytes);

    // Owner (Tron witness account) and signer (actual ECDSA key) may differ due to delegation.
    // - Owner EVM address comes from raw.witnessAddress (0x41 prefix + 20-byte EVM).
    // - Signer EVM address comes from recovering the public key from (sha256(raw), witnessSignature).
    const tronWitness = raw.witnessAddress;
    if (!tronWitness) {
      throw new Error(`Block ${blockNumber} missing witnessAddress`);
    }
    const ownerEvm = tronWitnessAddressToEvmAddress(tronWitness);
    if (!ownerEvm) {
      throw new Error(`Block ${blockNumber} has invalid witnessAddress bytes`);
    }

    const witnessSig = header.witnessSignature as Buffer | undefined;
    if (!witnessSig || witnessSig.length < SIGNATURE_SIZE) {
      throw new Error(`Block ${blockNumber} missing or invalid witnessSignature`);
    }

    // Recover signing public key from sha256(BlockHeader_raw).
    const rawBytes = BlockHeader_raw.encode(raw as BlockHeader_raw).finish();
    const digest = sha256(rawBytes);
    const pubUncompressed = recoverUncompressedPublicKey(
      digest,
      witnessSig.subarray(0, SIGNATURE_SIZE)
    );
    if (!pubUncompressed) {
      throw new Error(`Failed to recover signing public key for block ${blockNumber}`);
    }
    const signerEvm = evmAddressFromUncompressed(pubUncompressed);

    // Record per-owner delegatee mapping.
    const existing = ownerToDelegatee.get(ownerEvm);
    if (existing && existing !== signerEvm) {
      throw new Error(
        `Owner ${ownerEvm} has conflicting delegatees: ${existing} vs ${signerEvm} at block ${blockNumber}`
      );
    }
    ownerToDelegatee.set(ownerEvm, signerEvm);

    // CompressedTronBlockMetadata layout per block:
    // [0..31]  parentHash (bytes32)  -> raw.parentHash
    // [32..63] txTrieRoot (bytes32)  -> raw.txTrieRoot
    // [64..67] timestamp (uint32, big-endian seconds)
    // [68]     witnessAddressIndex (uint8)
    const parentHash = raw.parentHash;
    const txTrieRoot = raw.txTrieRoot;

    if (!parentHash || !txTrieRoot) {
      throw new Error(`Block ${blockNumber} missing parentHash or txTrieRoot`);
    }

    parentHash.copy(metadataBuf, metaOffset);
    metaOffset += 32;

    txTrieRoot.copy(metadataBuf, metaOffset);
    metaOffset += 32;

    const tsMs = BigInt(raw.timestamp.toString()); // ms
    const tsSec = tsMs / 1000n;
    if (tsSec > BigInt(0xffffffff)) {
      throw new Error(`Timestamp seconds overflow uint32 for block ${blockNumber}`);
    }
    metadataBuf.writeUInt32BE(Number(tsSec), metaOffset);
    metaOffset += 4;

    // Placeholder witnessIndex (patched after deriving the canonical SR ordering).
    metadataBuf.writeUInt8(0, metaOffset);
    metaOffset += 1;
    witnessEvmAddresses.push(signerEvm);
    witnessOwnersByBlock.push(ownerEvm);

    // Signatures: Tron stores [r(32) | s(32) | v(1)].
    witnessSig.copy(sigsBuf, sigOffset, 0, SIGNATURE_SIZE);
    sigOffset += SIGNATURE_SIZE;
    witnessSignatures.push(toHex0x(witnessSig.subarray(0, SIGNATURE_SIZE)));

    log.info("Generated block", {
      blockNumber,
      blockId,
      blockHash,
      rawHeaderBytes,
      witnessEvmAddress: signerEvm,
      witnessOwnerEvmAddress: ownerEvm,
    });
  }

  if (metaOffset !== metadataBuf.length) {
    throw new Error("Metadata buffer offset mismatch");
  }
  if (sigOffset !== sigsBuf.length) {
    throw new Error("Signature buffer offset mismatch");
  }

  // Build SR owners (srs) and witness delegatees arrays in canonical lexicographic order by SR owner address.
  const zeroAddress = "0x0000000000000000000000000000000000000000" as Address;
  const srsFixed = [...ownerToDelegatee.keys()].sort((a, b) =>
    a.toLowerCase().localeCompare(b.toLowerCase())
  );

  if (srsFixed.length !== 27) {
    throw new Error(`Expected exactly 27 unique SR owners, got ${srsFixed.length}`);
  }
  if (!isStrictlySortedLex(srsFixed)) {
    throw new Error("SR owners must be strictly increasing (lexicographic)");
  }
  if (srsFixed.some((a) => a === zeroAddress)) {
    throw new Error("SR owners must not contain the zero address");
  }

  const witnessDelegateesFixed = srsFixed.map((sr) => ownerToDelegatee.get(sr) ?? zeroAddress);
  if (witnessDelegateesFixed.some((a) => a === zeroAddress)) {
    throw new Error("Missing witness delegatee for at least one SR owner");
  }

  // Patch witnessIndex bytes in metadata and fill witnessIndices using the canonical SR ordering.
  if (witnessOwnersByBlock.length !== numBlocks) {
    throw new Error("witnessOwnersByBlock length mismatch");
  }
  const indexByOwner = new Map<Address, number>(srsFixed.map((sr, i) => [sr, i]));
  for (let i = 0; i < witnessOwnersByBlock.length; i++) {
    const owner = witnessOwnersByBlock[i]!;
    const idx = indexByOwner.get(owner);
    if (idx == null) throw new Error(`Block witness owner not in SR set: ${owner}`);
    witnessIndices.push(idx);
    metadataBuf.writeUInt8(idx, i * TRON_BLOCK_METADATA_SIZE + 68);
  }

  const endingBlockId = blockIds[blockIds.length - 1]!;
  const lastRaw = blocks[blocks.length - 1]!.blockHeader!.rawData as BlockHeader_raw;
  if (!lastRaw.txTrieRoot) {
    throw new Error("Last block missing txTrieRoot");
  }

  const startingBlockTxTrieRoot = toHex0x(parentRaw.txTrieRoot);
  const endingBlockTxTrieRoot = toHex0x(lastRaw.txTrieRoot);
  // Timestamp for the ending block – convert from ms to seconds.
  const endingTimestampMs = BigInt(lastRaw.timestamp.toString());
  const endingTimestampSec = (endingTimestampMs / 1000n).toString();

  const fixture: Fixture = {
    network: "tron-mainnet",
    startBlock: String(startBlock),
    endBlock: String(endBlock),
    startingBlockId,
    startingBlockTxTrieRoot,
    startingBlockTimestamp: parentTimestampSec,
    endingBlockId,
    endingBlockTxTrieRoot,
    endingBlockTimestamp: endingTimestampSec,
    srs: srsFixed,
    compressedTronBlockMetadata: toHex0x(metadataBuf),
    compressedSignatures: toHex0x(sigsBuf),
    blockNumbers,
    blockIds,
    blockHashes,
    blockHeaderRawBytes,
    witnessEvmAddresses,
    witnessIndices,
    witnessSignatures,
    witnessDelegatees: witnessDelegateesFixed,
  };

  writeFileSync(outPath, JSON.stringify(fixture, null, 2));
  log.info("Wrote Tron fixture", { outPath });
}

main().catch((err) => {
  // eslint-disable-next-line no-console
  console.error(err);
  process.exit(1);
});
