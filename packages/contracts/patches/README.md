# Patches

This directory contains small, reviewable patches applied to third-party vendored dependencies under `packages/contracts/lib/`.

## safe-modules (4337)

- Patch: `safe-modules-4337-solc-pragma.patch`
- Why: upstream pins `pragma solidity 0.8.23;` exactly, which forces Foundry to compile with `solc==0.8.23`. Our repo uses `solc=0.8.28` and `evm_version=cancun`, and Foundry cannot (reliably) compile those sources with an exact `0.8.23` requirement in this configuration.
- What: relaxes the pragma to `pragma solidity ^0.8.23;` for the two 4337 contracts so they can be compiled with our configured compiler.

Apply with:

- `pnpm -C packages/contracts apply-patches`

