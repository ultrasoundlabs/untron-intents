/**
 * Generate an offline fixture for Tron tx inclusion proof + 20 encoded headers.
 *
 * Usage:
 *   pnpm research genTronTxProofFixture <blockNumber> <txId> [outPath]
 *
 * Output JSON includes:
 * - `encodedTx`, `txId`, `txLeaf`
 * - all tx leaves for the block (sha256(encodedTx))
 * - `proof`, `indexBits`, `headerTxTrieRoot`
 * - `blocks`: 20 sequential encoded headers (174 bytes each)
 */
import { writeFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import Long from "long";
import { z } from "zod";
import { sha256 } from "@noble/hashes/sha2.js";
import { parseEnv } from "../lib/env.js";
import { log } from "../lib/logger.js";
import { createTronClients } from "@untron/tron-protocol";
import type { BlockExtention, NumberMessage } from "@untron/tron-protocol/api";
import { Account, BlockHeader_raw, Transaction, Transaction_raw } from "@untron/tron-protocol/tron";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

// From: apps/research/src/scripts
// To:   crates/tron/testdata/fixtures
const TRON_CRATE_FIXTURES_DIR = resolve(__dirname, "../../../../crates/tron/testdata/fixtures");

function toHex0x(buf: Uint8Array | Buffer): string {
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

  if (rawBytes.length !== 105) {
    throw new Error(`unexpected BlockHeader_raw length: ${rawBytes.length} (expected 105)`);
  }
  if (sigBytes.length < 65) {
    throw new Error(`unexpected witnessSignature length: ${sigBytes.length} (expected >= 65)`);
  }

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
      if (!right) {
        next.push(left);
      } else {
        next.push(sha256Concat(left, right));
      }
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

async function fetchBlock(wallet: any, callOpts: any, num: number): Promise<BlockExtention> {
  const req: NumberMessage = { num: Long.fromNumber(num, true) };
  return await new Promise((resolve, reject) => {
    wallet.getBlockByNum2(req, callOpts.metadata, (err: any, res: BlockExtention | null) => {
      if (err || !res) return reject(err ?? new Error("Empty response from getBlockByNum2"));
      resolve(res);
    });
  });
}

async function getChainParameters(wallet: any, callOpts: any): Promise<any> {
  return await new Promise((resolve, reject) => {
    wallet.getChainParameters({}, callOpts.metadata, (err: any, res: any) =>
      err || !res
        ? reject(err ?? new Error("Empty response from getChainParameters"))
        : resolve(res)
    );
  });
}

async function estimateEnergy(wallet: any, callOpts: any, msg: any): Promise<any> {
  return await new Promise((resolve, reject) => {
    wallet.estimateEnergy(msg, callOpts.metadata, (err: any, res: any) =>
      err || !res ? reject(err ?? new Error("Empty response from estimateEnergy")) : resolve(res)
    );
  });
}

async function getAccountResource(wallet: any, callOpts: any, address: Buffer): Promise<any> {
  return await new Promise((resolve, reject) => {
    wallet.getAccountResource(
      Account.fromPartial({ address }),
      callOpts.metadata,
      (err: any, res: any) =>
        err || !res
          ? reject(err ?? new Error("Empty response from getAccountResource"))
          : resolve(res)
    );
  });
}

async function main() {
  const env = parseEnv(
    z.object({
      TRON_GRPC_HOST: z.string().min(1),
      TRON_API_KEY: z.string().optional(),
    })
  );

  const rawArgs = process.argv.slice(2);
  const args = rawArgs.length > 0 && /^\d+$/.test(rawArgs[0]!) ? rawArgs : rawArgs.slice(1);

  if (args.length < 2 || args.length > 3) {
    // eslint-disable-next-line no-console
    console.error(
      "Usage: pnpm research genTronTxProofFixture <blockNumber> <txId> [outPath]\n" +
        "Example: pnpm research genTronTxProofFixture 78812179 0x... crates/tron/testdata/fixtures/tx.json"
    );
    process.exit(1);
  }

  const blockNumber = Number(args[0]!);
  if (!Number.isInteger(blockNumber) || blockNumber <= 0) throw new Error("invalid blockNumber");

  const txIdHex = args[1]!.replace(/^0x/i, "").toLowerCase();
  if (!/^[0-9a-f]{64}$/.test(txIdHex)) throw new Error("invalid txId hex");

  const outPath = args[2]
    ? resolve(args[2])
    : resolve(TRON_CRATE_FIXTURES_DIR, `tron_tx_proof_${blockNumber}_${txIdHex}.json`);

  const { wallet, callOpts } = createTronClients(env.TRON_GRPC_HOST, env.TRON_API_KEY, {
    insecure: true,
  });

  log.info("Fetching tx block", { blockNumber, txId: `0x${txIdHex}` });
  const txBlock = await fetchBlock(wallet, callOpts, blockNumber);

  const headerRaw = txBlock.blockHeader?.rawData as BlockHeader_raw | undefined;
  const witnessSig = txBlock.blockHeader?.witnessSignature as Buffer | undefined;
  if (!headerRaw || !headerRaw.txTrieRoot || !witnessSig) {
    throw new Error("tx block missing header/rawData/txTrieRoot/witnessSignature");
  }

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

  if (targetIndex === -1 || !encodedTx || !txIdFromRaw) throw new Error("transaction not found");

  const { proof, indexBits, root } = merkleProofCarryUp(leaves, targetIndex);

  const headerTxTrieRoot = Buffer.from(headerRaw.txTrieRoot);
  if (toHex0x(root) !== toHex0x(headerTxTrieRoot)) {
    throw new Error(
      `txTrieRoot mismatch: computed=${toHex0x(root)} header=${toHex0x(headerTxTrieRoot)}`
    );
  }

  log.info("Fetching 19 following blocks", { from: blockNumber + 1, to: blockNumber + 19 });
  const blocks: Buffer[] = [];
  blocks.push(packEncodedHeader(headerRaw, witnessSig));
  for (let i = 1; i < 20; i++) {
    const b = await fetchBlock(wallet, callOpts, blockNumber + i);
    const raw = b.blockHeader?.rawData as BlockHeader_raw | undefined;
    const sig = b.blockHeader?.witnessSignature as Buffer | undefined;
    if (!raw || !sig) throw new Error(`block ${blockNumber + i} missing header/rawData/signature`);
    blocks.push(packEncodedHeader(raw, sig));
  }

  // Cost model reference data (for offline Rust tests).
  const chainParams = await getChainParameters(wallet, callOpts);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const txDecoded: any = Transaction.decode(encodedTx!);
  const c0: any = txDecoded.rawData.contract[0];
  const any0: any = c0.parameter;
  const { TriggerSmartContract } =
    await import("@untron/tron-protocol/core/contract/smart_contract");
  const trigger = TriggerSmartContract.decode(any0.value);

  const energyRequiredMsg = await estimateEnergy(
    wallet,
    callOpts,
    TriggerSmartContract.fromPartial({
      ownerAddress: trigger.ownerAddress,
      contractAddress: trigger.contractAddress,
      callValue: trigger.callValue,
      data: trigger.data,
      callTokenValue: trigger.callTokenValue,
      tokenId: trigger.tokenId,
    })
  );

  const acctRes = await getAccountResource(wallet, callOpts, Buffer.from(trigger.ownerAddress));

  const energyFeeRaw = (chainParams.chainParameter ?? []).find(
    (p: any) => p.key === "getEnergyFee"
  )?.value;
  const txFeeRaw = (chainParams.chainParameter ?? []).find(
    (p: any) => p.key === "getTransactionFee"
  )?.value;

  const energyFee = energyFeeRaw != null ? BigInt(energyFeeRaw.toString()) : null;
  const txFee = txFeeRaw != null ? BigInt(txFeeRaw.toString()) : null;

  const txSizeBytes = encodedTx!.length;
  const energyRequired = BigInt((energyRequiredMsg.energyRequired ?? 0).toString());
  const computedFeeLimitSun =
    (energyFee ?? 0n) * energyRequired + (txFee ?? 0n) * BigInt(txSizeBytes);

  const acctOut = {
    energyUsed: (acctRes.EnergyUsed ?? acctRes.energyUsed ?? 0).toString(),
    energyLimit: (acctRes.EnergyLimit ?? acctRes.energyLimit ?? 0).toString(),
    netUsed: (acctRes.NetUsed ?? acctRes.netUsed ?? 0).toString(),
    netLimit: (acctRes.NetLimit ?? acctRes.netLimit ?? 0).toString(),
    freeNetUsed: (acctRes.freeNetUsed ?? acctRes.FreeNetUsed ?? 0).toString(),
    freeNetLimit: (acctRes.freeNetLimit ?? acctRes.FreeNetLimit ?? 0).toString(),
  };

  const out = {
    network: "tron-mainnet",
    blockNumber: String(blockNumber),
    txId: toHex0x(decodeHex0x(txIdHex)),
    targetIndex,
    encodedTx: toHex0x(encodedTx),
    txIdFromRawData: toHex0x(txIdFromRaw),
    txLeaf: toHex0x(sha256Buf(encodedTx)),
    headerTxTrieRoot: toHex0x(headerTxTrieRoot),
    leaves: leaves.map(toHex0x),
    proof: proof.map(toHex0x),
    indexBits: indexBits.toString(10),
    root: toHex0x(root),
    blocks: blocks.map(toHex0x),
    cost: {
      energyFeeSunPerEnergy: energyFee != null ? energyFee.toString() : null,
      txFeeSunPerByte: txFee != null ? txFee.toString() : null,
      energyRequired: energyRequired.toString(),
      txSizeBytes,
      computedFeeLimitSun: computedFeeLimitSun.toString(),
      accountResource: acctOut,
    },
  };

  writeFileSync(outPath, JSON.stringify(out, null, 2));
  log.info("Wrote Tron tx proof fixture", {
    outPath,
    txCount: leaves.length,
    proofLen: proof.length,
  });
}

main().catch((err) => {
  // eslint-disable-next-line no-console
  console.error(err);
  process.exit(1);
});
