/**
 * Generate an offline fixture suitable for testing StatefulTronTxReader against a real Tron tx.
 *
 * Usage:
 *   pnpm research genTronTxReaderFixture <blockNumber> <txId> [outPath]
 *
 * The fixture contains:
 * - `blocks`: 20 packed encoded headers (174 bytes each) starting at `blockNumber`
 * - `encodedTx`: protobuf-encoded Transaction bytes for `txId`
 * - `proof` + `indexBits`: SHA-256 Merkle inclusion proof against `txTrieRoot`
 * - `srs` + `witnessDelegatees`: canonical SR owner accounts + delegatee signing keys for this epoch
 * - `expected`: decoded contract params for TransferContract or TriggerSmartContract
 */
import { writeFileSync } from "node:fs";
import { dirname, isAbsolute, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import Long from "long";
import { z } from "zod";
import { sha256 } from "@noble/hashes/sha2.js";
import { keccak_256 } from "@noble/hashes/sha3.js";
import * as secp256k1 from "@noble/secp256k1";
import { parseEnv } from "../lib/env.js";
import { log } from "../lib/logger.js";
import { createTronClients } from "@untron/tron-protocol";
import type { BlockExtention, NumberMessage } from "@untron/tron-protocol/api";
import {
  BlockHeader_raw,
  Transaction,
  Transaction_raw,
  Transaction_Contract_ContractType,
} from "@untron/tron-protocol/tron";
import { TriggerSmartContract } from "@untron/tron-protocol/core/contract/smart_contract";
import {
  DelegateResourceContract,
  TransferContract,
} from "@untron/tron-protocol/core/contract/balance_contract";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const REPO_ROOT = resolve(__dirname, "../../../../");

// From: apps/research/src/scripts
// To:   packages/contracts/test/tron/fixtures
const CONTRACTS_TRON_FIXTURES_DIR = resolve(
  __dirname,
  "../../../../packages/contracts/test/tron/fixtures"
);

type Hex0x = `0x${string}`;

function toHex0x(buf: Uint8Array | Buffer): Hex0x {
  return `0x${Buffer.from(buf).toString("hex")}`;
}

function decodeHex0x(hex: string): Buffer {
  const cleaned = hex.replace(/^0x/i, "");
  return Buffer.from(cleaned, "hex");
}

function sha256Buf(bytes: Uint8Array | Buffer): Buffer {
  return Buffer.from(sha256(bytes));
}

function sha256Concat(a: Buffer, b: Buffer): Buffer {
  return sha256Buf(Buffer.concat([a, b]));
}

function packEncodedHeader(raw: BlockHeader_raw, witnessSignature: Uint8Array | Buffer): Buffer {
  const rawBytes = Buffer.from(BlockHeader_raw.encode(raw).finish());
  const sigBytes = Buffer.from(witnessSignature);

  if (rawBytes.length !== 105)
    throw new Error(`unexpected BlockHeader_raw length: ${rawBytes.length} (expected 105)`);
  if (sigBytes.length < 65)
    throw new Error(`unexpected witnessSignature length: ${sigBytes.length} (expected >= 65)`);

  const out = Buffer.concat([
    Buffer.from([0x0a, 0x69]),
    rawBytes,
    Buffer.from([0x12, 0x41]),
    sigBytes.subarray(0, 65),
  ]);
  if (out.length !== 174) throw new Error(`unexpected encoded header length: ${out.length}`);
  return out;
}

// Tron txTrieRoot uses a "carry-up" rule: odd last node is promoted unchanged (no self-duplication).
function merkleProofCarryUp(
  leaves: Buffer[],
  leafIndex: number
): { proof: Buffer[]; indexBits: bigint; root: Buffer } {
  if (leaves.length === 0) throw new Error("empty merkle tree");
  if (leafIndex < 0 || leafIndex >= leaves.length) throw new Error("leafIndex out of bounds");

  let idx = leafIndex;
  let level = leaves.slice();
  const proof: Buffer[] = [];
  let indexBits = 0n;
  let bit = 0n;

  while (level.length > 1) {
    const hasNoSibling = (level.length & 1) === 1 && idx === level.length - 1;
    if (!hasNoSibling) {
      const isRight = (idx & 1) === 1;
      if (isRight) indexBits |= 1n << bit;
      const sibling = level[isRight ? idx - 1 : idx + 1]!;
      proof.push(sibling);
      bit += 1n;
    }

    const next: Buffer[] = [];
    for (let j = 0; j < level.length; j += 2) {
      const left = level[j]!;
      const right = level[j + 1];
      if (!right) next.push(left);
      else next.push(sha256Concat(left, right));
    }

    idx = Math.floor(idx / 2);
    level = next;
  }

  return { proof, indexBits, root: level[0]! };
}

function computeTxIdFromRawData(tx: Transaction): Buffer {
  if (!tx.rawData) throw new Error("missing tx.rawData");
  const rawBytes = Buffer.from(Transaction_raw.encode(tx.rawData).finish());
  return sha256Buf(rawBytes);
}

function evmAddressFromUncompressed(pub: Uint8Array): Hex0x {
  // input may be 65-byte (0x04 | X | Y) or 64-byte (X | Y)
  if (pub.length === 65 && pub[0] === 0x04) pub = pub.subarray(1);
  if (pub.length !== 64) throw new Error(`unexpected pubkey length: ${pub.length}`);
  const hash = keccak_256(pub);
  return `0x${Buffer.from(hash.subarray(12)).toString("hex")}`;
}

function recoverUncompressedPublicKey(hash32: Uint8Array, sig65: Buffer): Uint8Array {
  const r = sig65.subarray(0, 32);
  const s = sig65.subarray(32, 64);
  let recovery = Number(sig65[64]! & 0xff);
  if (recovery >= 27) recovery -= 27;
  if (recovery < 0 || recovery > 3) throw new Error(`invalid recovery: ${recovery}`);

  const inSig = new Uint8Array(65);
  inSig[0] = recovery;
  inSig.set(r, 1);
  inSig.set(s, 33);

  const pub = secp256k1.recoverPublicKey(inSig, hash32, { prehash: false });
  if (pub.length === 65) return pub;
  if (pub.length === 33)
    return secp256k1.Point.fromHex(Buffer.from(pub).toString("hex")).toBytes(false);
  throw new Error(`unexpected recovered pubkey length: ${pub.length}`);
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function fetchBlock(wallet: any, callOpts: any, num: number): Promise<BlockExtention> {
  const req: NumberMessage = { num: Long.fromNumber(num, true) };

  let lastErr: unknown = null;
  for (let attempt = 1; attempt <= 5; attempt++) {
    try {
      const res = await new Promise<BlockExtention>((resolve, reject) => {
        wallet.getBlockByNum2(req, callOpts.metadata, (err: any, out: BlockExtention | null) => {
          if (err || !out) return reject(err ?? new Error("Empty response from getBlockByNum2"));
          resolve(out);
        });
      });
      // Sometimes providers return an incomplete block extension; treat that as retryable.
      const raw = res.blockHeader?.rawData as BlockHeader_raw | undefined;
      const sig = res.blockHeader?.witnessSignature as Uint8Array | Buffer | undefined;
      if (!raw || !sig || (sig as Uint8Array).length < 65) {
        throw new Error("Incomplete block extension (missing rawData/signature)");
      }
      return res;
    } catch (err) {
      lastErr = err;
      if (attempt < 5) {
        await sleep(250 * attempt);
        continue;
      }
    }
  }

  throw lastErr instanceof Error ? lastErr : new Error(String(lastErr));
}

function toBytes32Hex(value: bigint): Hex0x {
  if (value < 0n) throw new Error("expected non-negative bigint");
  const hex = value.toString(16).padStart(64, "0");
  return `0x${hex}`;
}

function tron21ToBytes21Hex(bytes: Uint8Array | Buffer): Hex0x {
  const b = Buffer.from(bytes);
  if (b.length !== 21) throw new Error(`expected 21-byte tron address, got ${b.length}`);
  if (b[0] !== 0x41) throw new Error("expected tron address 0x41 prefix");
  return toHex0x(b);
}

async function deriveSrSetAndDelegatees(opts: {
  wallet: any;
  callOpts: any;
  startBlock: number;
  mustIncludeOwners: Set<string>;
  maxScanBlocks: number;
}): Promise<{ srs: Hex0x[]; witnessDelegatees: Hex0x[] }> {
  const ownerToDelegatee = new Map<string, string>();

  for (let i = 0; i < opts.maxScanBlocks; i++) {
    const n = opts.startBlock + i;
    const b = await fetchBlock(opts.wallet, opts.callOpts, n);
    const raw = b.blockHeader?.rawData as BlockHeader_raw | undefined;
    const sig = b.blockHeader?.witnessSignature as Buffer | undefined;
    if (!raw || !raw.witnessAddress || !sig || sig.length < 65) continue;

    const ownerTron = Buffer.from(raw.witnessAddress);
    if (ownerTron.length !== 21 || ownerTron[0] !== 0x41) continue;
    const ownerEvm = `0x${ownerTron.subarray(1).toString("hex")}`;

    const rawBytes = BlockHeader_raw.encode(raw).finish();
    const digest = sha256(rawBytes);
    const pub = recoverUncompressedPublicKey(digest, sig.subarray(0, 65));
    const signerEvm = evmAddressFromUncompressed(pub);

    const prev = ownerToDelegatee.get(ownerEvm);
    if (prev && prev.toLowerCase() !== signerEvm.toLowerCase()) {
      throw new Error(`conflicting delegatees for owner ${ownerEvm}: ${prev} vs ${signerEvm}`);
    }
    ownerToDelegatee.set(ownerEvm, signerEvm);

    const hasAll =
      opts.mustIncludeOwners.size === 0 ||
      [...opts.mustIncludeOwners].every((o) => ownerToDelegatee.has(o));
    if (ownerToDelegatee.size === 27 && hasAll) break;
  }

  if (ownerToDelegatee.size !== 27) {
    throw new Error(
      `failed to derive full SR owner set (expected 27, got ${ownerToDelegatee.size})`
    );
  }
  for (const o of opts.mustIncludeOwners) {
    if (!ownerToDelegatee.has(o)) throw new Error(`missing required witness owner in SR set: ${o}`);
  }

  const srs = [...ownerToDelegatee.keys()].sort((a, b) =>
    a.toLowerCase().localeCompare(b.toLowerCase())
  ) as Hex0x[];
  const witnessDelegatees = srs.map((sr) => ownerToDelegatee.get(sr)!) as Hex0x[];

  return { srs, witnessDelegatees };
}

async function main() {
  const env = parseEnv(
    z.object({
      TRON_GRPC_HOST: z.string().min(1),
      TRON_API_KEY: z.string().optional(),
      TRON_FIXTURE_SR_SCAN_BLOCKS: z.string().optional(),
    })
  );

  const rawArgs = process.argv.slice(2);
  const args = rawArgs.length > 0 && /^\d+$/.test(rawArgs[0]!) ? rawArgs : rawArgs.slice(1);

  if (args.length < 2 || args.length > 3) {
    // eslint-disable-next-line no-console
    console.error(
      "Usage: pnpm research genTronTxReaderFixture <blockNumber> <txId> [outPath]\n" +
        "Example: pnpm research genTronTxReaderFixture 78812179 0x... packages/contracts/test/tron/fixtures/tx.json"
    );
    process.exit(1);
  }

  const blockNumber = Number(args[0]!);
  if (!Number.isInteger(blockNumber) || blockNumber <= 0) throw new Error("invalid blockNumber");

  const txIdHex = args[1]!.replace(/^0x/i, "").toLowerCase();
  if (!/^[0-9a-f]{64}$/.test(txIdHex)) throw new Error("invalid txId hex");

  const outPath = args[2]
    ? isAbsolute(args[2])
      ? args[2]
      : resolve(REPO_ROOT, args[2])
    : resolve(CONTRACTS_TRON_FIXTURES_DIR, `tron_reader_${blockNumber}_${txIdHex}.json`);

  const { wallet, callOpts } = createTronClients(env.TRON_GRPC_HOST, env.TRON_API_KEY, {
    insecure: true,
  });

  log.info("Fetching tx block", { blockNumber, txId: `0x${txIdHex}` });
  const txBlock = await fetchBlock(wallet, callOpts, blockNumber);

  const headerRaw = txBlock.blockHeader?.rawData as BlockHeader_raw | undefined;
  const witnessSig = txBlock.blockHeader?.witnessSignature as Buffer | undefined;
  if (!headerRaw || !headerRaw.txTrieRoot || !witnessSig)
    throw new Error("block missing header/rawData/txTrieRoot/witnessSignature");

  const txExts = txBlock.transactions ?? [];
  if (txExts.length === 0) throw new Error("block has no transactions");

  const leaves: Buffer[] = [];
  let targetIndex = -1;
  let encodedTx: Buffer | undefined;
  let txIdFromRaw: Buffer | undefined;

  for (let i = 0; i < txExts.length; i++) {
    const txExt = txExts[i]!;
    const tx = txExt.transaction as Transaction | undefined;
    if (!tx) throw new Error(`missing tx at index ${i}`);

    const enc = Buffer.from(Transaction.encode(tx).finish());
    leaves.push(sha256Buf(enc));

    const txid = Buffer.from(txExt.txid).toString("hex").toLowerCase();
    if (txid === txIdHex) {
      targetIndex = i;
      encodedTx = enc;
      txIdFromRaw = computeTxIdFromRawData(tx);
    }
  }

  if (targetIndex === -1 || !encodedTx || !txIdFromRaw)
    throw new Error("transaction not found in block");

  const { proof, indexBits, root } = merkleProofCarryUp(leaves, targetIndex);

  const headerTxTrieRoot = Buffer.from(headerRaw.txTrieRoot);
  if (toHex0x(root) !== toHex0x(headerTxTrieRoot)) {
    throw new Error(
      `txTrieRoot mismatch: computed=${toHex0x(root)} header=${toHex0x(headerTxTrieRoot)}`
    );
  }

  log.info("Fetching 19 following blocks for finality", {
    from: blockNumber + 1,
    to: blockNumber + 19,
  });
  const blocks: Buffer[] = [];
  blocks.push(packEncodedHeader(headerRaw, witnessSig));
  for (let i = 1; i < 20; i++) {
    const b = await fetchBlock(wallet, callOpts, blockNumber + i);
    const raw = b.blockHeader?.rawData as BlockHeader_raw | undefined;
    const sig = b.blockHeader?.witnessSignature as Buffer | undefined;
    if (!raw || !sig) throw new Error(`block ${blockNumber + i} missing header/rawData/signature`);
    blocks.push(packEncodedHeader(raw, sig));
  }

  // Build the SR owner + delegatee arrays (canonical by owner address lexicographic sort).
  const mustIncludeOwners = new Set<string>();
  for (const blockEnc of blocks) {
    // witness address lives in BlockHeader_raw bytes at offset 82 (witnessAddress field is 21 bytes),
    // but we already have the decoded raw objects only for the first block here. Re-fetching would be wasteful,
    // so just derive "must include" from the first 20 blocks by reading them again (cheap enough for offline fixture gen).
    // We'll scan SRs starting from `blockNumber` anyway and enforce it contains these owners.
    void blockEnc;
  }
  for (let i = 0; i < 20; i++) {
    const b = await fetchBlock(wallet, callOpts, blockNumber + i);
    const raw = b.blockHeader?.rawData as BlockHeader_raw | undefined;
    if (!raw?.witnessAddress) throw new Error(`block ${blockNumber + i} missing witnessAddress`);
    const ownerTron = Buffer.from(raw.witnessAddress);
    if (ownerTron.length !== 21 || ownerTron[0] !== 0x41)
      throw new Error(`block ${blockNumber + i} invalid witnessAddress`);
    mustIncludeOwners.add(`0x${ownerTron.subarray(1).toString("hex")}`);
  }

  const maxScanBlocks = Number(env.TRON_FIXTURE_SR_SCAN_BLOCKS ?? "120");
  if (!Number.isInteger(maxScanBlocks) || maxScanBlocks <= 0)
    throw new Error("invalid TRON_FIXTURE_SR_SCAN_BLOCKS");

  const { srs, witnessDelegatees } = await deriveSrSetAndDelegatees({
    wallet,
    callOpts,
    startBlock: blockNumber,
    mustIncludeOwners,
    maxScanBlocks,
  });

  // Decode the tx's contract parameters for expected fields.
  const txDecoded = Transaction.decode(encodedTx);
  const c0 = txDecoded.rawData?.contract?.[0];
  if (!c0) throw new Error("tx missing rawData.contract[0]");
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const any0: any = c0.parameter;
  const typeId = Number((c0.type as unknown as number) ?? -1);
  const anyValue = any0?.value as Uint8Array | Buffer | undefined;
  if (!anyValue) throw new Error("tx contract parameter missing Any.value");

  let expected: any;
  if (typeId === Transaction_Contract_ContractType.TransferContract || typeId === 1) {
    const t = TransferContract.decode(anyValue);
    expected = {
      contractType: "TransferContract",
      senderTron: tron21ToBytes21Hex(t.ownerAddress),
      toTron: tron21ToBytes21Hex(t.toAddress),
      amountSun: toBytes32Hex(BigInt(t.amount.toString())),
    };
  } else if (typeId === Transaction_Contract_ContractType.TriggerSmartContract || typeId === 31) {
    const t = TriggerSmartContract.decode(anyValue);
    expected = {
      contractType: "TriggerSmartContract",
      senderTron: tron21ToBytes21Hex(t.ownerAddress),
      toTron: tron21ToBytes21Hex(t.contractAddress),
      callValueSun: toBytes32Hex(BigInt(t.callValue.toString())),
      data: toHex0x(t.data),
    };
  } else if (
    typeId === Transaction_Contract_ContractType.DelegateResourceContract ||
    typeId === 57
  ) {
    const t = DelegateResourceContract.decode(anyValue);
    expected = {
      contractType: "DelegateResourceContract",
      ownerTron: tron21ToBytes21Hex(t.ownerAddress),
      receiverTron: tron21ToBytes21Hex(t.receiverAddress),
      resource: Number(t.resource),
      balanceSun: toBytes32Hex(BigInt(t.balance.toString())),
      lock: t.lock,
      lockPeriod: toBytes32Hex(BigInt(t.lockPeriod.toString())),
    };
  } else {
    throw new Error(`unsupported contract type id: ${typeId}`);
  }

  const out = {
    network: "tron-mainnet",
    blockNumber,
    txId: toHex0x(decodeHex0x(txIdHex)),
    txIdFromRawData: toHex0x(txIdFromRaw),
    targetIndex,
    encodedTx: toHex0x(encodedTx),
    txLeaf: toHex0x(sha256Buf(encodedTx)),
    headerTxTrieRoot: toHex0x(headerTxTrieRoot),
    proof: proof.map(toHex0x),
    indexBits: Number(indexBits),
    blocks: blocks.map(toHex0x),
    srs,
    witnessDelegatees,
    expected,
  };

  writeFileSync(outPath, JSON.stringify(out, null, 2));
  log.info("Wrote TronTxReader fixture", {
    outPath,
    proofLen: proof.length,
    contractType: expected.contractType,
  });
}

main().catch((err) => {
  // eslint-disable-next-line no-console
  console.error(err);
  process.exit(1);
});
