# @untron/tron-protocol

Strongly-typed TRON gRPC clients for Node/TypeScript, generated once with ts-proto and committed so consumers do not need protoc.

## Upstream

- Upstream protos: TRON protocol (api/ and core/ directories)
- Upstream commit (pinned): <SHA_GOES_HERE>
- Vendored into: `protos/`

## Install

```bash
pnpm add @untron/tron-protocol
```

## Usage

```ts
import { createTronClients } from '@untron/tron-protocol';

const host = process.env.TRON_GRPC_HOST!; // e.g. grpc.trongrid.io:50051
const apiKey = process.env.TRON_API_KEY;  // optional, if your provider requires it

const { wallet, callOpts } = createTronClients(host, apiKey);

wallet.getNowBlock({}, callOpts, (err, res) => {
  if (err) {
    console.error(err);
  } else {
    console.log(res?.blockHeader?.rawData?.number);
  }
});
```

## Helper

- `createTronClients(host, apiKey?)` sets up TLS credentials and attaches metadata header `TRON-PRO-API-KEY` when provided. Returns `{ wallet, solidity, callOpts }`.

## Codegen

- Dev deps: `ts-proto`, `protobufjs`, `long`
- Inputs: vendored protos under `protos/` (start with `api/` and required imports)
- Output: TypeScript in `src/gen/` (committed)
- Google protos: resolved via `google-proto-files` npm package (no manual vendoring)

Commands:

```bash
pnpm --filter @untron/tron-protocol run codegen
pnpm --filter @untron/tron-protocol run build
```

Determinism check (CI and prepublish):

```bash
pnpm --filter @untron/tron-protocol run codegen && pnpm --filter @untron/tron-protocol run -s check-dirty
```

## Updating protos

1. Choose upstream commit and update `README.md` with `Upstream commit` SHA.
2. Replace contents of `protos/` with the minimal required TRON protos (at least `api/` and their imports from `core/`).
3. Run `pnpm run codegen` and commit changes under `src/gen/`.
4. Bump package version (SemVer):
   - minor for additive proto changes
   - major for breaking changes

## Ports & endpoints

- Fullnode gRPC typically: `:50051`
- Solidity (validated) gRPC typically: provider’s solidity endpoint/port

## Smoke test (CI)

Set these secrets in CI to enable the smoke test:
- `TRON_GRPC_HOST`: e.g. `grpc.trongrid.io:50051`
- `TRON_API_KEY`: API key header if required by your provider

The smoke test calls `Wallet.getNowBlock` and asserts a block is returned.

## Licensing

- This package is MIT.
- Vendored TRON `.proto` files are subject to their upstream licenses.
- Include upstream LICENSE/NOTICE files in this package when vendoring protos and ensure compliance.


