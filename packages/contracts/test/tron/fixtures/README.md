# Tron fixtures

These are offline JSON fixtures generated from real Tron mainnet data, intended for `StatefulTronTxReader` tests.

Generate a fixture:

`TRON_GRPC_HOST=... TRON_API_KEY=... pnpm research genTronTxReaderFixture <blockNumber> 0x<txid>`

Run the Solidity fixture test:

`TRON_TX_READER_FIXTURE=packages/contracts/test/tron/fixtures/<file>.json forge test --root packages/contracts -m test_fixture_decodesRealTx_ifFixtureProvided`

