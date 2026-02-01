import { defineConfig } from "@wagmi/cli";
import { foundry, foundryDefaultExcludes } from "@wagmi/cli/plugins";

// Determine artifacts directory based on FOUNDRY_PROFILE env variable
// const profile = process.env.FOUNDRY_PROFILE ?? "dev";
const artifactsPath = `out`;

export default defineConfig({
  out: "packages/contracts/abi/generated.ts",
  plugins: [
    foundry({
      project: "packages/contracts", // <— your Foundry project root
      artifacts: artifactsPath,
      // Only generate TypeScript bindings for our own contracts under `src/`.
      // This avoids duplicate names coming from dependencies (e.g. IERC20/Ownable).
      include: ["src/**/*.json"],
      exclude: [
        // Start from wagmi defaults.
        ...foundryDefaultExcludes,

        // Extra exclusions for this repo.
        "**/*.dbg.json",
        "auth/**", // helper-only auth contracts (no need for TS bindings)
        "interfaces/**", // placeholder interfaces with empty ABI
        "**/IMulticall3.sol/**",
      ],
      // forge: { build: true } // default; Wagmi can run forge build for you
    }),
  ],
});
