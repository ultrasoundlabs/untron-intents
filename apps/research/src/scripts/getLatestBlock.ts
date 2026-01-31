import { createPublicClient, http } from "viem";
import { mainnet } from "viem/chains";
import { parseEnv } from "../lib/env.js";
import { log } from "../lib/logger.js";
import { z } from "zod";

async function main() {
  const env = parseEnv(
    z.object({
      EVM_RPC_URL: z.url(),
    })
  );
  const client = createPublicClient({
    chain: mainnet,
    transport: http(env.EVM_RPC_URL),
  });

  const block = await client.getBlock();
  log.info("Latest block", {
    number: block.number,
    hash: block.hash,
    txs: BigInt(block.transactions.length),
  });
}

main().catch((err) => {
  log.error(err);
  process.exit(1);
});
