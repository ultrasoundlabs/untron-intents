/**
 * List transactions in a Tron block (txid + contract type).
 *
 * Usage:
 *   TRON_GRPC_HOST=... pnpm research listTronBlockTxs <blockNumber>
 *   TRON_GRPC_HOST=... pnpm research listTronBlockTxs <blockNumber> --types
 */
import Long from "long";
import { z } from "zod";
import { parseEnv } from "../lib/env.js";
import { log } from "../lib/logger.js";
import { createTronClients } from "@untron/tron-protocol";
import type { BlockExtention, NumberMessage } from "@untron/tron-protocol/api";
import { Transaction, Transaction_Contract_ContractType } from "@untron/tron-protocol/tron";

async function fetchBlock(wallet: any, callOpts: any, num: number): Promise<BlockExtention> {
  const req: NumberMessage = { num: Long.fromNumber(num, true) };
  return await new Promise((resolve, reject) => {
    wallet.getBlockByNum2(req, callOpts.metadata, (err: any, res: BlockExtention | null) => {
      if (err || !res) return reject(err ?? new Error("Empty response from getBlockByNum2"));
      resolve(res);
    });
  });
}

function typeName(typeId: number): string {
  // ts-proto enums are bidirectional in JS: enum[1] => "TransferContract"
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  return (Transaction_Contract_ContractType as any)[typeId] ?? `Unknown(${typeId})`;
}

async function main() {
  // Avoid crashing when piping output (e.g. `| head`) and the consumer closes stdout early.
  process.stdout.on("error", (err: any) => {
    if (err?.code === "EPIPE") process.exit(0);
  });

  const env = parseEnv(
    z.object({
      TRON_GRPC_HOST: z.string().min(1),
      TRON_API_KEY: z.string().optional(),
    })
  );

  const rawArgs = process.argv.slice(2);
  const args = rawArgs.length > 0 && /^\d+$/.test(rawArgs[0]!) ? rawArgs : rawArgs.slice(1);
  if (args.length < 1) {
    // eslint-disable-next-line no-console
    console.error("Usage: pnpm research listTronBlockTxs <blockNumber> [--types]");
    process.exit(1);
  }

  const blockNumber = Number(args[0]!);
  if (!Number.isInteger(blockNumber) || blockNumber <= 0) throw new Error("invalid blockNumber");
  const showTypes = args.includes("--types");

  const { wallet, callOpts } = createTronClients(env.TRON_GRPC_HOST, env.TRON_API_KEY, {
    insecure: true,
  });

  const b = await fetchBlock(wallet, callOpts, blockNumber);
  const txExts = b.transactions ?? [];
  log.info("Block transactions", { blockNumber, count: txExts.length });

  for (let i = 0; i < txExts.length; i++) {
    const txExt = txExts[i]!;
    const txid = `0x${Buffer.from(txExt.txid).toString("hex")}`;
    let t = "Unknown";
    if (showTypes) {
      const tx = txExt.transaction as Transaction | undefined;
      const c0 = tx?.rawData?.contract?.[0];
      const typeId = c0?.type != null ? Number((c0.type as unknown as number) ?? -1) : -1;
      t = typeName(typeId);
    }
    // eslint-disable-next-line no-console
    console.log(
      [i.toString().padStart(4, " "), txid, showTypes ? t : ""].filter(Boolean).join(" ")
    );
  }
}

main().catch((err) => {
  log.error(err);
  process.exit(1);
});
