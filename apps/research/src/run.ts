import { readdir } from "node:fs/promises";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { log } from "./lib/logger.js";
import dotenv from "dotenv";
import { summarizeError } from "./lib/sanitize.js";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const scriptsDir = path.resolve(__dirname, "scripts");
const rootDir = path.resolve(__dirname, "..");

function loadEnvFiles(scriptName?: string) {
  const candidates = [
    path.join(rootDir, ".env"),
    path.join(rootDir, ".env.local"),
    scriptName ? path.join(rootDir, `.env.${scriptName}`) : undefined,
    scriptName ? path.join(rootDir, `.env.${scriptName}.local`) : undefined,
  ].filter(Boolean) as string[];

  for (const p of candidates) {
    if (fs.existsSync(p)) {
      dotenv.config({ path: p });
    }
  }
}

async function listScripts(): Promise<string[]> {
  const files = await readdir(scriptsDir);
  return files
    .filter((f) => f.endsWith(".ts") && f !== "_template.ts")
    .map((f) => f.replace(/\.ts$/, ""))
    .sort();
}

async function main() {
  const [scriptName] = process.argv.slice(2);

  if (!scriptName || scriptName === "list" || scriptName === "--list") {
    const names = await listScripts();
    if (names.length === 0) {
      log.info("No scripts found in", scriptsDir);
      return;
    }
    log.info("Available scripts:", names);
    return;
  }

  const targetTs = path.join(scriptsDir, `${scriptName}.ts`);

  try {
    loadEnvFiles(scriptName);
    const moduleUrl = pathToFileURL(targetTs).href;
    await import(moduleUrl);
  } catch (err) {
    const summary = summarizeError(err);
    // Avoid dumping huge nested error objects (e.g., calldata blobs) into the terminal by default.
    if (process.env.RESEARCH_DEBUG_ERRORS === "1") {
      log.error(`Failed to run script '${scriptName}'`, err);
    } else {
      log.error(`Failed to run script '${scriptName}': ${summary.message}`, summary.data);
    }
    const names = await listScripts();
    log.info("Try one of:", names);
    process.exit(1);
  }
}

main();
