import { z } from "zod";
import { parseEnv } from "../lib/env.js";
import { log } from "../lib/logger.js";
import { createTronClients } from "@untron/tron-protocol";
import type {
  BlockExtention,
  EmptyMessage,
  NumberMessage,
  WitnessList,
} from "@untron/tron-protocol/api";
import { BlockHeader_raw, Account } from "@untron/tron-protocol/tron";
import Long from "long";
import { sha256 } from "@noble/hashes/sha2.js";
import { keccak_256 } from "@noble/hashes/sha3.js";
import * as secp256k1 from "@noble/secp256k1";
import { writeFile } from "node:fs/promises";

type TronWallet = ReturnType<typeof createTronClients>["wallet"];
type TronMetadata = ReturnType<typeof createTronClients>["callOpts"]["metadata"];

type WitnessDelegateMaps = {
  // Tron address (21-byte) hex of delegated key -> Tron address hex of witness owner
  delegateToWitness: Map<string, string>;
  // Tron address hex of witness owner -> Tron address hex of a delegated key (first key, if any)
  witnessToDelegate: Map<string, string>;
};

function toLong(value: bigint | number | string): Long {
  return Long.fromValue(typeof value === "bigint" ? value.toString() : value);
}

async function getNowBlock(
  wallet: ReturnType<typeof createTronClients>["wallet"],
  metadata: ReturnType<typeof createTronClients>["callOpts"]["metadata"]
): Promise<BlockExtention> {
  return await new Promise((resolve, reject) => {
    wallet.getNowBlock2({} as EmptyMessage, metadata, (err, res) => {
      if (err) return reject(err);
      resolve(res);
    });
  });
}

async function getBlockByNumber(
  wallet: ReturnType<typeof createTronClients>["wallet"],
  metadata: ReturnType<typeof createTronClients>["callOpts"]["metadata"],
  blockNumber: bigint
): Promise<BlockExtention> {
  const req: NumberMessage = { num: toLong(blockNumber) };
  return await new Promise((resolve, reject) => {
    wallet.getBlockByNum2(req, metadata, (err, res) => {
      if (err) return reject(err);
      resolve(res);
    });
  });
}

function encodeBlockHeaderRaw(
  raw?: { [K in keyof BlockHeader_raw]?: BlockHeader_raw[K] } | null
): Uint8Array {
  if (!raw) return new Uint8Array();
  return BlockHeader_raw.encode(raw as BlockHeader_raw).finish();
}

function tronAddressFromPublicKey(pub: Uint8Array): Buffer | null {
  // Accept uncompressed 65-byte (0x04 | X | Y) or raw 64-byte (X | Y) public keys.
  if (pub.length === 65 && pub[0] === 0x04) {
    pub = pub.subarray(1);
  } else if (pub.length !== 64) {
    return null;
  }

  const hash = keccak_256(pub); // 32 bytes
  if (hash.length !== 32) return null;

  // Tron address is 0x41 prefix + last 20 bytes of keccak256(pubkey)
  const tronAddr = Buffer.alloc(21);
  tronAddr[0] = 0x41;
  tronAddr.set(hash.subarray(12), 1);
  return tronAddr;
}

async function getWitnessDelegateMaps(
  wallet: TronWallet,
  metadata: TronMetadata
): Promise<WitnessDelegateMaps> {
  const emptyReq: EmptyMessage = {};

  const witnessList = await new Promise<WitnessList>((resolve, reject) => {
    wallet.listWitnesses(emptyReq, metadata, (err, res) => {
      if (err || !res) return reject(err ?? new Error("Empty response from listWitnesses"));
      resolve(res);
    });
  });

  const delegateToWitness = new Map<string, string>();
  const witnessToDelegate = new Map<string, string>();

  for (const w of witnessList.witnesses) {
    const witnessAddrBuf = w.address as Buffer | undefined;
    if (!witnessAddrBuf || witnessAddrBuf.length === 0) continue;

    const witnessHex = witnessAddrBuf.toString("hex");

    const accountReq = Account.create({
      address: witnessAddrBuf,
    });

    try {
      const account = await new Promise<Account>((resolve, reject) => {
        wallet.getAccount(accountReq, metadata, (err, res) => {
          if (err || !res) return reject(err ?? new Error("Empty response from getAccount"));
          resolve(res);
        });
      });

      const perm = account.witnessPermission;
      if (!perm) continue;

      for (const key of perm.keys) {
        const keyAddrBuf = key.address as Buffer | undefined;
        if (!keyAddrBuf || keyAddrBuf.length === 0) continue;

        const keyHex = keyAddrBuf.toString("hex");
        delegateToWitness.set(keyHex, witnessHex);
        if (!witnessToDelegate.has(witnessHex)) {
          witnessToDelegate.set(witnessHex, keyHex);
        }
      }
    } catch (err) {
      log.warn("failed to load witness account for delegate map", { witness: witnessHex, err });
    }
  }

  return { delegateToWitness, witnessToDelegate };
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
      try {
        const hex = Buffer.from(pub).toString("hex");
        return secp256k1.Point.fromHex(hex).toBytes(false);
      } catch (e) {
        log.warn("failed to decompress pubkey", { e });
        return null;
      }
    }
    log.warn("unexpected recovered pubkey length", { len: pub.length });
    return null;
  } catch (err) {
    log.warn("failed to recover public key", { err, recovery });
    return null;
  }
}

function publicKey64FromUncompressed(pub: Uint8Array): Uint8Array | null {
  if (pub.length === 65 && pub[0] === 0x04) return pub.subarray(1);
  if (pub.length === 64) return pub;
  return null;
}

async function main() {
  const env = parseEnv(
    z.object({
      TRON_GRPC_HOST: z.string().min(1),
      TRON_API_KEY: z.string().optional(),
      SRSCAN_EPOCHS: z.string().optional(),
    })
  );

  const { wallet, callOpts } = createTronClients(env.TRON_GRPC_HOST, env.TRON_API_KEY, {
    insecure: true,
  });

  log.info("loading witness delegate permissions from Tron...");
  const delegateMaps = await getWitnessDelegateMaps(wallet, callOpts.metadata);
  log.info(
    `loaded ${BigInt(delegateMaps.delegateToWitness.size).toString()} delegated keys across ${BigInt(
      delegateMaps.witnessToDelegate.size
    ).toString()} witnesses`
  );

  // args: depth (number) and optional flags like --verbose/-v
  const args = process.argv.slice(3);
  const verbose =
    args.includes("--verbose") || args.includes("-v") || process.env.SRSCAN_VERBOSE === "1";
  const depthToken = args.find((a) => /^\d+$/.test(a));
  const depthArg = depthToken ?? env.SRSCAN_EPOCHS ?? "5";
  const depth = BigInt(depthArg);
  if (depth <= 0n) {
    log.error("Depth must be > 0");
    process.exit(1);
  }

  const nowBlock = await getNowBlock(wallet, callOpts.metadata);
  const latestNumber = BigInt(nowBlock.blockHeader?.rawData?.number?.toString() ?? "0");

  const srCounts = new Map<string, bigint>();

  const totals = {
    fetchedBlocks: 0n,
    grpcErrors: 0n,
    missingRaw: 0n,
    missingSig: 0n,
    shortSig: 0n,
    recoverOk: 0n,
    recoverFail: 0n,
  } as const;
  // mutable copy
  const counters: Record<keyof typeof totals, bigint> = { ...totals } as any;

  const startNs = process.hrtime.bigint();
  for (let epoch = 1n; epoch < depth; epoch++) {
    const epochBlock = latestNumber - epoch * 7200n;
    const epochEndBlock = epochBlock + 26n;
    let eFetched = 0n,
      eGrpc = 0n,
      eMissingRaw = 0n,
      eMissingSig = 0n,
      eShortSig = 0n,
      eRecoverOk = 0n,
      eRecoverFail = 0n;

    log.info(
      `epoch ${epoch.toString()}: scanning blocks ${epochBlock.toString()}..${epochEndBlock.toString()}`
    );
    for (let blockOffset = 0n; blockOffset < 27n; blockOffset++) {
      const target = epochBlock + blockOffset;
      if (verbose) {
        log.debug(`block ${target.toString()}`);
      }
      try {
        const block = await getBlockByNumber(wallet, callOpts.metadata, target);
        const header = block.blockHeader;
        const raw = header?.rawData ?? null;
        const sig = header?.witnessSignature ?? Buffer.alloc(0);
        counters.fetchedBlocks++;
        eFetched++;
        if (!raw) {
          counters.missingRaw++;
          eMissingRaw++;
          if (verbose) log.debug("skip: missing raw header", { block: target.toString() });
          continue;
        }
        if (!sig || sig.length === 0) {
          counters.missingSig++;
          eMissingSig++;
          if (verbose) log.debug("skip: missing signature", { block: target.toString() });
          continue;
        }
        if (sig.length < 65) {
          counters.shortSig++;
          eShortSig++;
          if (verbose)
            log.debug("skip: short signature", { block: target.toString(), len: sig.length });
          continue;
        }

        const rawBytes = encodeBlockHeaderRaw(raw);
        const digest = sha256(rawBytes);
        const pubUncompressed = recoverUncompressedPublicKey(digest, sig);
        const pub64 = pubUncompressed ? publicKey64FromUncompressed(pubUncompressed) : null;
        if (!pub64 || !pubUncompressed) {
          counters.recoverFail++;
          eRecoverFail++;
          if (verbose) log.debug("recover failed", { block: target.toString() });
          continue;
        }

        const tronAddrBuf = tronAddressFromPublicKey(pubUncompressed);
        if (!tronAddrBuf) {
          counters.recoverFail++;
          eRecoverFail++;
          if (verbose)
            log.debug("recover failed: could not derive Tron address", {
              block: target.toString(),
            });
          continue;
        }

        const tronHex = tronAddrBuf.toString("hex");
        const witnessHex = delegateMaps.delegateToWitness.get(tronHex);
        if (!witnessHex) {
          if (verbose)
            log.debug("skip: signer not in witness delegate map", {
              block: target.toString(),
              tronAddress: tronHex,
            });
          continue;
        }

        counters.recoverOk++;
        eRecoverOk++;
        srCounts.set(witnessHex, (srCounts.get(witnessHex) ?? 0n) + 1n);
      } catch (err) {
        counters.grpcErrors++;
        eGrpc++;
        log.warn("failed to fetch/process block", { block: target.toString(), err });
      }
    }

    log.info(
      `epoch ${epoch.toString()} done: fetched=${eFetched.toString()} grpcErrors=${eGrpc.toString()} missingRaw=${eMissingRaw.toString()} missingSig=${eMissingSig.toString()} shortSig=${eShortSig.toString()} recoverOk=${eRecoverOk.toString()} recoverFail=${eRecoverFail.toString()} uniqueSRs=${BigInt(srCounts.size).toString()}`
    );
  }

  const elapsedNs = process.hrtime.bigint() - startNs;
  const elapsedSec = elapsedNs / 1_000_000_000n;
  const uniqueCount = BigInt(srCounts.size);

  // compute epochs scanned and a fixed-point rate (3 decimal places) using BigInt only
  const scannedEpochs = depth - 1n;
  const scale = 1000n;
  const perSecScaled =
    elapsedSec === 0n ? scannedEpochs * scale : (scannedEpochs * scale) / elapsedSec;
  const whole = perSecScaled / scale;
  const frac = perSecScaled % scale;
  const fracStr = frac.toString().padStart(3, "0");

  const keys = Array.from(srCounts.keys()).join("\n");
  if (verbose) {
    log.info(keys);
  }
  log.info(`total ${uniqueCount.toString()} unique SRs`);
  log.info(
    `scraped in ${elapsedSec.toString()} sec, avg ${whole.toString()}.${fracStr} epochs/s (epochs=${scannedEpochs.toString()})`
  );
  log.info(
    `totals: fetched=${counters.fetchedBlocks.toString()} grpcErrors=${counters.grpcErrors.toString()} missingRaw=${counters.missingRaw.toString()} missingSig=${counters.missingSig.toString()} shortSig=${counters.shortSig.toString()} recoverOk=${counters.recoverOk.toString()} recoverFail=${counters.recoverFail.toString()}`
  );
  await writeFile("srs.txt", keys, { encoding: "utf8" });
  log.info("saved in srs.txt");
}

main().catch((err) => {
  log.error(err);
  process.exit(1);
});
