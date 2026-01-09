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
      exclude: [
        // Start from wagmi defaults, but keep IERC20's ABI.
        ...foundryDefaultExcludes.filter((x) => x !== "IERC20.sol/**"),

        // Extra exclusions for this repo.
        "**/*.dbg.json",
        "auth/**", // e.g. solady's auth/Ownable
        "interfaces/**", // e.g. placeholder interfaces with empty ABI
        "**/IMulticall3.sol/**",
      ],
      // forge: { build: true } // default; Wagmi can run forge build for you
    }),
  ],
});
