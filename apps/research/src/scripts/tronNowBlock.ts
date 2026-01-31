import { z } from "zod";
import { parseEnv } from "../lib/env.js";
import { log } from "../lib/logger.js";
import { createTronClients } from "@untron/tron-protocol";
import type { BlockExtention, EmptyMessage } from "@untron/tron-protocol/api";

async function main() {
  const env = parseEnv(
    z.object({
      TRON_GRPC_HOST: z.string().min(1),
      TRON_API_KEY: z.string().optional(),
    })
  );

  const { wallet, callOpts } = createTronClients(env.TRON_GRPC_HOST, env.TRON_API_KEY, {
    insecure: true,
  });

  const block: BlockExtention = await new Promise((resolve, reject) => {
    wallet.getNowBlock2({} as EmptyMessage, callOpts.metadata, (err, res) => {
      if (err) return reject(err);
      resolve(res);
    });
  });

  const numberStr = block.blockHeader?.rawData?.number?.toString();

  log.info("TRON NowBlock", {
    number: numberStr,
  });
}

main().catch((err) => {
  log.error(err);
  process.exit(1);
});
