import { z } from "zod";
import Long from "long";
import { createHash } from "node:crypto";
import { createTronClients } from "@untron/tron-protocol";
import type { BytesMessage } from "@untron/tron-protocol/api";
import {
  Transaction_raw,
  Transaction,
  TransactionInfo,
  transactionInfo_codeToJSON,
  transaction_Result_codeToJSON,
  transaction_Result_contractResultToJSON,
} from "@untron/tron-protocol/tron";
import { parseEnv } from "../lib/env.js";
import { log } from "../lib/logger.js";
import { summarizeError } from "../lib/sanitize.js";

type Source = "wallet" | "solidity";

type Args = {
  txIdHex: string;
  sources: Source[];
  json: boolean;
  includeUnknownFields: boolean;
};

type FetchErrors = Partial<{
  getTransactionById: ReturnType<typeof summarizeError>;
  getTransactionInfoById: ReturnType<typeof summarizeError>;
}>;

function usageAndExit(): never {
  // eslint-disable-next-line no-console
  console.error(
    [
      "Usage:",
      "  pnpm research fetchTronTx <txid> [--wallet|--solidity] [--summary] [--include-unknown]",
      "",
      "Env:",
      "  TRON_GRPC_HOST=...         e.g. grpc.trongrid.io:50051",
      "  TRON_API_KEY=...          optional (TRON-PRO-API-KEY header)",
      "",
      "Notes:",
      "  - <txid> can be 0x-prefixed or raw hex (32 bytes).",
      "  - Default sources: both wallet + solidity (if available).",
      "  - Default output: JSON (use --summary for a compact view).",
    ].join("\n")
  );
  process.exit(1);
}

function parseTxIdHex(txId: string): string {
  const cleaned = txId.startsWith("0x") ? txId.slice(2) : txId;
  if (!/^[0-9a-fA-F]{64}$/.test(cleaned)) {
    throw new Error("Invalid txid: expected 32-byte hex (optionally 0x-prefixed)");
  }
  return cleaned.toLowerCase();
}

function parseArgs(argv: string[]): Args {
  const runnerIdx = argv.indexOf("fetchTronTx");
  const normalizedArgv = runnerIdx >= 0 ? argv.slice(runnerIdx + 1) : argv;

  const positionals: string[] = [];
  const flags = new Set<string>();
  for (const a of normalizedArgv) {
    if (a.startsWith("-")) flags.add(a);
    else positionals.push(a);
  }

  if (flags.has("-h") || flags.has("--help")) usageAndExit();

  const [txId] = positionals;
  if (!txId) usageAndExit();

  const sources: Source[] = [];
  if (flags.has("--wallet")) sources.push("wallet");
  if (flags.has("--solidity")) sources.push("solidity");
  if (sources.length === 0) sources.push("wallet", "solidity");

  return {
    txIdHex: parseTxIdHex(txId),
    sources,
    json: !flags.has("--summary"),
    includeUnknownFields: flags.has("--include-unknown"),
  };
}

function toHex0x(bytes: Uint8Array | Buffer): string {
  return `0x${Buffer.from(bytes).toString("hex")}`;
}

function isPlainObject(x: unknown): x is Record<string, unknown> {
  return typeof x === "object" && x !== null && !Array.isArray(x);
}

function serializeForJson(
  value: unknown,
  opts: { includeUnknownFields: boolean },
  depth = 20
): unknown {
  if (depth <= 0) return "[max-depth]";
  if (value == null) return value;

  if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
    return value;
  }
  if (typeof value === "bigint") return value.toString();

  if (Long.isLong(value)) return value.toString();

  if (typeof Buffer !== "undefined" && Buffer.isBuffer(value)) return toHex0x(value);
  if (value instanceof Uint8Array) return toHex0x(value);

  if (Array.isArray(value)) {
    return value.map((v) => serializeForJson(v, opts, depth - 1));
  }

  if (!isPlainObject(value)) return String(value);

  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(value)) {
    if (!opts.includeUnknownFields && k === "_unknownFields") continue;
    out[k] = serializeForJson(v, opts, depth - 1);
  }
  return out;
}

function bytesToPrintableMessage(bytes: Buffer): { utf8?: string; hex: string } {
  const hex = toHex0x(bytes);
  const utf8 = bytes.toString("utf8");
  // Avoid returning mojibake / control chars; keep it conservative.
  const printable =
    utf8.length > 0 &&
    !/[^\x09\x0a\x0d\x20-\x7e]/.test(utf8) && // allow \t \n \r + printable ASCII
    !utf8.includes("\u0000");
  return printable ? { utf8, hex } : { hex };
}

function deriveExecutionStatus(tx?: Transaction, info?: TransactionInfo) {
  const txRet0 = tx?.ret?.[0];
  const txRet = txRet0 ? transaction_Result_codeToJSON(txRet0.ret) : undefined;
  const txContractRet = txRet0
    ? transaction_Result_contractResultToJSON(txRet0.contractRet)
    : undefined;

  const infoResult = info ? transactionInfo_codeToJSON(info.result) : undefined;
  const receiptResult = info?.receipt
    ? transaction_Result_contractResultToJSON(info.receipt.result)
    : undefined;

  return {
    txRet,
    txContractRet,
    infoResult,
    receiptResult,
  };
}

function computeTxIdHexFromRawData(tx?: Transaction): string | undefined {
  const raw = tx?.rawData;
  if (!raw) return undefined;
  const rawDataBytes = Transaction_raw.encode(raw).finish();
  return createHash("sha256").update(rawDataBytes).digest("hex");
}

async function main() {
  const env = parseEnv(
    z.object({
      TRON_GRPC_HOST: z.string().min(1),
      TRON_API_KEY: z.string().optional(),
    })
  );

  const args = parseArgs(process.argv.slice(2));

  const { wallet, solidity, callOpts } = createTronClients(env.TRON_GRPC_HOST, env.TRON_API_KEY, {
    insecure: true,
  });

  const request: BytesMessage = { value: Buffer.from(args.txIdHex, "hex") };

  async function fetchFrom(source: Source) {
    const client = source === "wallet" ? wallet : solidity;

    const errors: FetchErrors = {};

    const [transaction, transactionInfo] = await Promise.all([
      new Promise<Transaction | undefined>((resolve) => {
        client.getTransactionById(request, callOpts.metadata, (err, res) => {
          if (err) errors.getTransactionById = summarizeError(err);
          resolve(res);
        });
      }),
      new Promise<TransactionInfo | undefined>((resolve) => {
        client.getTransactionInfoById(request, callOpts.metadata, (err, res) => {
          if (err) errors.getTransactionInfoById = summarizeError(err);
          resolve(res);
        });
      }),
    ]);

    return {
      transaction,
      transactionInfo,
      errors: Object.keys(errors).length ? errors : undefined,
    };
  }

  const fetched = await Promise.all(
    args.sources.map(async (s) => [s, await fetchFrom(s)] as const)
  );

  const bySource: Record<
    Source,
    { transaction?: Transaction; transactionInfo?: TransactionInfo; errors?: FetchErrors }
  > = {
    wallet: {},
    solidity: {},
  };

  for (const [s, data] of fetched) bySource[s] = data;

  const bestTx = bySource.solidity.transaction ?? bySource.wallet.transaction;
  const bestInfo = bySource.solidity.transactionInfo ?? bySource.wallet.transactionInfo;

  if (!bestTx && !bestInfo) {
    throw new Error(
      `No transaction data returned for txId=0x${args.txIdHex}. Check TRON_GRPC_HOST/TRON_API_KEY, or try --wallet/--solidity.`
    );
  }

  const computedTxIdHex = computeTxIdHexFromRawData(bestTx);
  const status = deriveExecutionStatus(bestTx, bestInfo);
  const resMessage = bestInfo?.resMessage
    ? bytesToPrintableMessage(bestInfo.resMessage)
    : undefined;
  const contractResult = bestInfo?.contractResult?.length
    ? bestInfo.contractResult.map((b) => toHex0x(b))
    : undefined;

  const summary = {
    txId: `0x${args.txIdHex}`,
    computedTxId: computedTxIdHex ? `0x${computedTxIdHex}` : undefined,
    txIdMatches: computedTxIdHex ? computedTxIdHex === args.txIdHex : undefined,
    blockNumber: bestInfo?.blockNumber?.toString(),
    status,
    resMessage,
    contractResult,
  };

  if (!args.json) {
    log.info("TRON transaction summary", summary);
    return;
  }

  const out = {
    summary,
    sources: {
      wallet: {
        errors: bySource.wallet.errors,
        transaction: serializeForJson(bySource.wallet.transaction, {
          includeUnknownFields: args.includeUnknownFields,
        }),
        transactionInfo: serializeForJson(bySource.wallet.transactionInfo, {
          includeUnknownFields: args.includeUnknownFields,
        }),
      },
      solidity: {
        errors: bySource.solidity.errors,
        transaction: serializeForJson(bySource.solidity.transaction, {
          includeUnknownFields: args.includeUnknownFields,
        }),
        transactionInfo: serializeForJson(bySource.solidity.transactionInfo, {
          includeUnknownFields: args.includeUnknownFields,
        }),
      },
    },
  };

  // eslint-disable-next-line no-console
  console.log(JSON.stringify(out, null, 2));
}

main().catch((err) => {
  log.error(err);
  process.exit(1);
});
