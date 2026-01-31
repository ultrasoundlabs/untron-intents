# Repository Guidelines

## Project Structure

- `packages/contracts/`: Solidity contracts (Foundry), tests, scripts, and generated ABIs/TS types.
- `packages/tron-protocol/`: TypeScript package + protobuf codegen for Tron protocol types.
- `apps/indexer/`, `apps/solver/`, `apps/e2e/`: Rust workspace members (services + end-to-end tests).
- `crates/tron/`: Shared Rust Tron client/utilities.
- `crates/bindings/`: Rust bindings generated from the contracts’ ABIs (do not hand-edit).
- `infra/`: Local stack and ops config (Docker Compose, env examples, OpenAPI sidecar).

## Setup

- Install deps: `pnpm install` (Node 20 in CI; repo pins `pnpm@10.17.0`).
- Initialize submodules (contracts pull deps via git submodules): `git submodule update --init --recursive`.

## Build, Test, and Development Commands

- `pnpm build`: Build all workspace packages.
- `pnpm test`: Run all workspace tests.
- `pnpm verify`: CI-equivalent suite (format checks, contracts tests, prod build, codegen, typecheck).
- Contracts (from repo root):
  - `pnpm --filter @untron/intents-contracts test` (or `forge test --root packages/contracts`)
  - `pnpm codegen` (wagmi types; updates `packages/contracts/abi/generated.ts`)
- Rust:
  - `cargo build -p indexer` / `cargo test -p e2e`
  - `pnpm --filter @untron/intents-indexer dev` (runs `cargo run -p indexer`)

## Coding Style & Naming

- TypeScript: Prettier (2 spaces, `printWidth=100`); run `pnpm format:ts`.
- Solidity: `forge fmt` (`pnpm format:sol` / `pnpm fmt:sol` for check-only).
- Rust: `cargo fmt`; prefer `cargo clippy -p indexer -- -D warnings` before PRs.
- Generated files: don’t edit `packages/contracts/abi/generated.ts` or `crates/bindings/*` manually; regenerate via `pnpm codegen` / `pnpm contracts:bind:rust`.

## Testing Guidelines

- Solidity tests live in `packages/contracts/test/` and run via `forge test`.
- Rust tests use `cargo test`; `apps/e2e/tests/` uses `testcontainers` and may require Docker.
- Changes should be test-driven:
  - Any new solver/indexer/protocol behavior should come with unit tests (fast, deterministic) and, where it crosses process boundaries, e2e coverage in `apps/e2e/tests/`.
  - Prefer designs that are easy to test: small modules with pure functions, explicit inputs/outputs, and dependency injection for networked components.

## Commit & Pull Request Guidelines

- Use Conventional Commits: `feat(scope): ...`, `fix(scope): ...`, `chore: ...`, `refactor(scope): ...`.
- Husky hooks run on commit/push; keep your branch green with `pnpm verify`.
- PRs: describe intent + risks, link issues, include relevant logs/screenshots for infra/e2e changes, and call out any regenerated artifacts.
