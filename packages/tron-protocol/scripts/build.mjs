import { existsSync } from "node:fs";
import { spawnSync } from "node:child_process";

if (!existsSync("src/gen")) {
  console.log("Skipping build: no generated sources found in src/gen.");
  process.exit(0);
}

const res = spawnSync("pnpm", ["exec", "tsc", "-p", "tsconfig.build.json"], {
  stdio: "inherit",
});

process.exit(res.status ?? 1);
