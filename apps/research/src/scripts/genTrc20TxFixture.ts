import { writeFileSync, mkdirSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import Long from "long";
import { z } from "zod";
import { parseEnv } from "../lib/env.js";
import { log } from "../lib/logger.js";
import { createTronClients } from "@untron/tron-protocol";
import { sha256 } from "@noble/hashes/sha2.js";
import type { Hex, Address } from "viem";

import type { BlockExtention, NumberMessage } from "@untron/tron-protocol/api";
import { Transaction, Transaction_Contract_ContractType } from "@untron/tron-protocol/tron";
import { TriggerSmartContract } from "@untron/tron-protocol/core/contract/smart_contract";

// -----------------------------------------------------------------------------
// Paths & basic helpers
// -----------------------------------------------------------------------------

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

// From: apps/research/src/scripts
// To:   packages/contracts/test/evm/fixtures
const TRC20_FIXTURES_DIR = resolve(__dirname, "../../../../packages/contracts/test/evm/fixtures");

function toHex(buf: Uint8Array | Buffer): string {
  return Buffer.from(buf).toString("hex");
}

function toHex0x(buf: Uint8Array | Buffer): Hex {
  return `0x${Buffer.from(buf).toString("hex")}` as Hex;
}

function tronAddressToEvmAddress(bytes: Uint8Array | Buffer): Address | null {
  // Tron address is 21 bytes: 0x41 prefix + 20-byte EVM address
  if (bytes.length !== 21 || bytes[0] !== 0x41) return null;
  return `0x${Buffer.from(bytes.subarray(1)).toString("hex")}` as Address;
}

function evmAddressToTronHex(evm: Address): Hex {
  const hex = evm.replace(/^0x/i, "").toLowerCase();
  return `0x41${hex}` as Hex;
}

// sha256(Transaction.encode(tx)) → tron tx leaf
function computeTxLeaf(encodedTx: Uint8Array): Hex {
  const digest = sha256(encodedTx);
  return toHex0x(digest);
}

// -----------------------------------------------------------------------------
// Fixture types
// -----------------------------------------------------------------------------

type Trc20TxFixture = {
  // Index of the transaction within the block (0-based)
  index: number;
  // Tron txid from TransactionExtention.txid
  txId: Hex;
  // sha256(Transaction.encode(tx))
  txLeaf: Hex;
  // Full protobuf-encoded Transaction bytes
  encodedTx: Hex;

  // Fields that should match the Solidity Trc20Transfer struct
  tronBlockNumber: string;
  tronBlockTimestamp: string; // seconds (stringified)
  tronTokenEvm: Address;
  fromTron: Hex; // 0x41 + 20 bytes
  toTron: Hex; // 0x41 + 20 bytes
  amount: string; // decimal string
  isTransferFrom: boolean;
  success: boolean;

  // Extra debugging info
  selector: Hex;
};

type Trc20BlockFixture = {
  network: "tron-mainnet";
  blockNumber: string;
  blockId: Hex;
  blockTimestamp: string; // seconds
  txCount: number;
  trc20Txs: Trc20TxFixture[];
};

// -----------------------------------------------------------------------------
// Tron access helpers
// -----------------------------------------------------------------------------

async function fetchBlock(wallet: any, callOpts: any, num: number): Promise<BlockExtention> {
  const req: NumberMessage = { num: Long.fromNumber(num, true) };
  return await new Promise((resolve, reject) => {
    wallet.getBlockByNum2(req, callOpts.metadata, (err: any, res: BlockExtention | null) => {
      if (err || !res) return reject(err ?? new Error("Empty response from getBlockByNum2"));
      resolve(res);
    });
  });
}

// -----------------------------------------------------------------------------
// TRC-20 decoding helpers
// -----------------------------------------------------------------------------

const SELECTOR_TRANSFER = "a9059cbb"; // transfer(address,uint256)
const SELECTOR_TRANSFER_FROM = "23b872dd"; // transferFrom(address,address,uint256)

type DecodedTrc20 = {
  fromTron: Hex;
  toTron: Hex;
  tronTokenEvm: Address;
  amount: bigint;
  isTransferFrom: boolean;
  selector: Hex;
};

function splitSelectorAndArgs(data: Buffer): { selector?: Hex; args?: Buffer } {
  if (data.length === 0) return {};
  if (data.length <= 4) {
    return { selector: toHex0x(data) };
  }

  const selector = data.subarray(0, 4);
  const args = data.subarray(4);

  return {
    selector: toHex0x(selector),
    args,
  };
}

// Abi decode TRC-20 transfer/transferFrom from TriggerSmartContract
function decodeTrc20FromTrigger(trigger: TriggerSmartContract): DecodedTrc20 | null {
  const ownerTronBytes = trigger.ownerAddress as Buffer;
  const contractTronBytes = trigger.contractAddress as Buffer;
  const dataBuf = trigger.data as Buffer;

  if (!ownerTronBytes || !contractTronBytes || !dataBuf) {
    return null;
  }

  if (ownerTronBytes.length !== 21 || ownerTronBytes[0] !== 0x41) return null;
  if (contractTronBytes.length !== 21 || contractTronBytes[0] !== 0x41) return null;

  const tronTokenEvm = tronAddressToEvmAddress(contractTronBytes);
  if (!tronTokenEvm) return null;

  const ownerTronHex = toHex0x(ownerTronBytes);

  const { selector, args } = splitSelectorAndArgs(dataBuf);
  if (!selector || !args) return null;

  const selectorNo0x = selector.slice(2);

  // transfer(address,uint256)
  if (selectorNo0x === SELECTOR_TRANSFER) {
    if (args.length != 64) return null;

    const toWord = args.subarray(0, 32);
    const amountWord = args.subarray(32, 64);

    const toEvm = ("0x" + Buffer.from(toWord.subarray(12)).toString("hex")) as Address;
    const amount = BigInt("0x" + Buffer.from(amountWord).toString("hex"));

    const fromTron = ownerTronHex;
    const toTron = evmAddressToTronHex(toEvm);

    return {
      fromTron,
      toTron,
      tronTokenEvm,
      amount,
      isTransferFrom: false,
      selector,
    };
  }

  // transferFrom(address,address,uint256)
  if (selectorNo0x === SELECTOR_TRANSFER_FROM) {
    if (args.length != 96) return null;

    const fromWord = args.subarray(0, 32);
    const toWord = args.subarray(32, 64);
    const amountWord = args.subarray(64, 96);

    const fromEvm = ("0x" + Buffer.from(fromWord.subarray(12)).toString("hex")) as Address;
    const toEvm = ("0x" + Buffer.from(toWord.subarray(12)).toString("hex")) as Address;
    const amount = BigInt("0x" + Buffer.from(amountWord).toString("hex"));

    const fromTron = evmAddressToTronHex(fromEvm);
    const toTron = evmAddressToTronHex(toEvm);

    return {
      fromTron,
      toTron,
      tronTokenEvm,
      amount,
      isTransferFrom: true,
      selector,
    };
  }

  return null;
}

// Decode a single Transaction into a TRC-20 transfer(From), if applicable
function decodeTrc20FromTransaction(tx: Transaction): DecodedTrc20 | null {
  const raw = tx.rawData;
  if (!raw || !raw.contract) return null;

  // Walk all TriggerSmartContract contracts; take the first that looks like TRC-20
  for (const c of raw.contract) {
    if (!c) continue;
    if (c.type !== Transaction_Contract_ContractType.TriggerSmartContract) continue;
    const param = c.parameter;
    if (!param || !param.value) continue;

    const trigger = TriggerSmartContract.decode(param.value as Buffer);
    const decoded = decodeTrc20FromTrigger(trigger);
    if (decoded) {
      return decoded;
    }
  }

  return null;
}

// -----------------------------------------------------------------------------
// Main script
// -----------------------------------------------------------------------------

async function main() {
  const env = parseEnv(
    z.object({
      TRON_GRPC_HOST: z.string().min(1),
      TRON_API_KEY: z.string().optional(),
    })
  );

  const argv = process.argv.slice(2);

  // Support both:
  //   tsx genTrc20TxFixture.ts <blockNumber> [outPath]
  // and:
  //   tsx src/run.ts genTrc20TxFixture <blockNumber> [outPath]
  let argOffset = 0;
  if (argv.length > 0 && !/^[0-9]+$/.test(argv[0]!)) {
    argOffset = 1;
  }

  const args = argv.slice(argOffset);
  if (args.length < 1 || args.length > 2) {
    // eslint-disable-next-line no-console
    console.error(
      "Usage: tsx genTrc20TxFixture.ts <blockNumber> [outPath]\n" +
        "Or:    tsx src/run.ts genTrc20TxFixture <blockNumber> [outPath]\n" +
        "Example: tsx genTrc20TxFixture.ts 55000000 packages/contracts/test/evm/fixtures/trc20_block_55000000.json"
    );
    process.exit(1);
  }

  const blockNumber = Number(args[0]!);
  if (!Number.isInteger(blockNumber) || blockNumber < 0) {
    throw new Error("Invalid blockNumber");
  }

  const outPath = args[1]
    ? resolve(args[1])
    : resolve(TRC20_FIXTURES_DIR, `trc20_block_${blockNumber}.json`);

  const { wallet, callOpts } = createTronClients(env.TRON_GRPC_HOST, env.TRON_API_KEY, {
    insecure: true,
  });

  log.info("Generating TRC-20 tx fixture", { blockNumber, outPath });

  const block = await fetchBlock(wallet, callOpts, blockNumber);

  const header = block.blockHeader;
  if (!header || !header.rawData) {
    throw new Error(`Block ${blockNumber} missing header/rawData`);
  }

  const rawHeader = header.rawData;
  const blockId = block.blockid
    ? toHex0x(block.blockid as Buffer)
    : (("0x" + "0".repeat(64)) as Hex);
  const tsMs = BigInt(rawHeader.timestamp.toString());
  const tsSec = (tsMs / 1000n).toString();

  const txExts = ((block as any).transactions ?? []) as any[];
  const txCount = txExts.length;

  log.info("Block summary", {
    blockNumber,
    blockId,
    timestampMs: rawHeader.timestamp.toString(),
    timestampSec: tsSec,
    txCount,
  });

  const trc20Txs: Trc20TxFixture[] = [];

  let sawTransfer = false;
  let sawTransferFrom = false;

  for (let index = 0; index < txExts.length; index++) {
    const txExt = txExts[index];
    if (!txExt || !txExt.transaction) continue;

    const tx = txExt.transaction as Transaction;
    const txId = txExt.txid ? toHex0x(txExt.txid as Buffer) : (("0x" + "0".repeat(64)) as Hex);

    const encodedTx = Transaction.encode(tx).finish();
    const encodedTxHex = toHex0x(encodedTx);
    const txLeaf = computeTxLeaf(encodedTx);

    const decodedTrc20 = decodeTrc20FromTransaction(tx);
    if (!decodedTrc20) {
      continue;
    }

    // Success: align with Solidity logic:
    // - If Result.ret (code) is absent, it defaults to 0 (SUCESS) per proto3.
    // - For TriggerSmartContract, require contractRet == SUCCESS (1).
    const firstResult = tx.ret && tx.ret.length > 0 ? tx.ret[0]! : undefined;
    const statusCode = firstResult ? firstResult.ret : 0;
    const contractRet = firstResult ? firstResult.contractRet : 0;
    const success = statusCode === 0 && contractRet === 1;

    const { fromTron, toTron, tronTokenEvm, amount, isTransferFrom, selector } = decodedTrc20;

    if (!isTransferFrom) {
      sawTransfer = true;
    } else {
      sawTransferFrom = true;
    }

    const fixtureTx: Trc20TxFixture = {
      index,
      txId,
      txLeaf,
      encodedTx: encodedTxHex,
      tronBlockNumber: String(blockNumber),
      tronBlockTimestamp: tsSec,
      tronTokenEvm,
      fromTron,
      toTron,
      amount: amount.toString(10),
      isTransferFrom,
      success,
      selector,
    };

    trc20Txs.push(fixtureTx);

    log.info("Found TRC-20 tx", {
      blockNumber,
      index,
      txId,
      selector,
      isTransferFrom,
      amount: amount.toString(10),
      fromTron,
      toTron,
      tronTokenEvm,
      success,
    });
  }

  if (!sawTransfer || !sawTransferFrom) {
    throw new Error(
      `Block ${blockNumber} does not contain at least one TRC-20 transfer and one transferFrom (sawTransfer=${sawTransfer}, sawTransferFrom=${sawTransferFrom})`
    );
  }

  const fixture: Trc20BlockFixture = {
    network: "tron-mainnet",
    blockNumber: String(blockNumber),
    blockId,
    blockTimestamp: tsSec,
    txCount,
    trc20Txs,
  };

  // Ensure output dir exists
  mkdirSync(dirname(outPath), { recursive: true });
  writeFileSync(outPath, JSON.stringify(fixture, null, 2));

  log.info("Wrote TRC-20 tx fixture", { outPath, trc20Count: trc20Txs.length });
}

main().catch((err) => {
  log.error(err);
  // eslint-disable-next-line no-console
  console.error(err);
  process.exit(1);
});
