# Research scripts

This is a small scratchpad for Tron-related scripts used to generate reproducible offline fixtures and debug Tron protobuf parsing.

Fetch full on-chain details for a transaction (raw tx + receipt/status + return data):

```
TRON_GRPC_HOST=... TRON_API_KEY=... pnpm research fetchTronTx 0x<txid>
TRON_GRPC_HOST=... TRON_API_KEY=... pnpm research fetchTronTx 0x<txid> --summary
```

## Generating fixtures for StatefulTronTxReader

Generate a single JSON fixture (headers + tx + proof + SR set) for a real Tron tx:

```
TRON_GRPC_HOST=... TRON_API_KEY=... pnpm research genTronTxReaderFixture <blockNumber> 0x<txid>
```

Then run the Solidity test against it:

```
TRON_TX_READER_FIXTURE=packages/contracts/test/tron/fixtures/<file>.json forge test --root packages/contracts -m test_fixture_decodesRealTx_ifFixtureProvided
```
