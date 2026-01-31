import { log } from "../lib/logger.js";
import { parseEnv } from "../lib/env.js";
import { z } from "zod";

async function main() {
  // Define per-script env here when needed
  const env = parseEnv(
    z.object({
      // EXAMPLE_KEY: z.string().min(1),
    })
  );
  log.info("hello");
}

main().catch((e) => {
  log.error(e);
  process.exit(1);
});
