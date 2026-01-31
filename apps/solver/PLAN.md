# Solver Implementation Plan (Untron Intents)

This document is a living plan for building the Untron Intents solver with **reliability-first** priorities.
It is intentionally detailed enough to drive implementation checklists, but avoids locking us into brittle architecture choices.

## Goals

- Reliably fill Untron intents by:
  - reading intent state from the **indexer via PostgREST**,
  - claiming intents on the EVM “hub” chain using **Safe/4337 AA** (bundlers + optional paymasters),
  - executing the corresponding Tron transaction(s),
  - generating Tron inclusion proofs (blocks/tx/proof/index),
  - and submitting `proveIntentFill(...)` (and any follow-ups) on the hub chain.
- Survive restarts/crashes without double-paying or abandoning in-flight work:
  - persistence is required; replay must be idempotent.
- Prioritize safety against abuse:
  - static allow/deny lists,
  - dynamic circuit breakers (blacklists with TTL/backoff),
  - rate limits,
  - profitability checks that include both Tron + hub-chain costs.

## Non-goals (for MVP)

- Becoming a general-purpose “Tron MEV / execution engine”.
- Filling arbitrary `TRIGGER_SMART_CONTRACT` intents without strong policy controls.
- Supporting partial fills (protocol currently requires “1 intent == 1 matching Tron tx proof”).
- Building a fully decentralized solver marketplace.

## Protocol recap (what the solver fills)

The pool contract supports four intent types (see `packages/contracts/src/UntronIntents.sol`):

- `TRX_TRANSFER`: Tron `TransferContract` (native TRX transfer, no memo).
- `USDT_TRANSFER`: Tron `TriggerSmartContract` call constrained to allowed USDT-related call shapes.
- `DELEGATE_RESOURCE`: Tron `DelegateResourceContract` with `lock=true` and explicit `lockPeriod`.
- `TRIGGER_SMART_CONTRACT`: arbitrary Tron `TriggerSmartContract` call (data + callValueSun must match).

**Key invariant**: proving requires a Tron tx whose decoded fields match intent specs exactly.

**Critical operational invariant**: do not broadcast the *final* intent-matching Tron tx until the
solver’s hub-chain `claimIntent(id)` is confirmed. Otherwise another solver can claim first and
prove “our” Tron tx and get paid.

## High-level architecture (conceptual)

Keep the implementation modular, but don’t prematurely over-abstract. The solver can be a single process with internal tasks.

### Components

1) **Indexer client (PostgREST only)**
   - Reads current pool state from `api.*` views (e.g. `api.pool_open_intents`).
   - Monitors indexing lag / health (e.g. `api.health`, `api.event_appended` newest block).

2) **Persistence layer**
   - Stores solver internal state (jobs, txs, retries, blacklists, cached prices).
   - Must allow safe resumption and multi-process concurrency (leases/locks) if we run multiple solver instances.

3) **Policy engine**
   - Static policy: allowlist/denylist, selector denylist, max amounts, min deadline slack, etc.
   - Dynamic policy: per-contract circuit breaker and optional per-(contract, selector) breaker.

4) **Planner**
   - Converts an intent into an executable plan:
     - `final_tx` that must match the intent exactly.
     - optional `pre_txs` (consolidation) to move assets to the executor key.
   - Produces a cost estimate + risk score used by profitability gating.

5) **Tron executor**
   - Signs and broadcasts Tron txs for each step.
   - Waits for confirmations/finality before building proofs.

6) **Proof builder** (powered by `tron` library)
   - Fetches Tron block headers + tx bytes + Merkle proof + leaf index.
   - Outputs the exact tuple needed by `proveIntentFill(blocks, encodedTx, proof, index)`.
   - Note: proof *computation* is deterministic, but the builder depends on network reads
     (tx lookup, block fetches, head/finality checks), so this step still needs retries/backoff.

7) **Hub (EVM) submitter**
   - Sends AA operations:
     - `claimIntent(id)` (and possibly other flows later),
     - `proveIntentFill(...)`,
     - optional “maintenance” transactions (e.g. `settleIntent`, or keeper-like actions).
   - Tracks receipts and handles retries safely.

8) **Price oracle + cache**
   - TRX/USD (and potentially other inputs) from a trusted API (e.g. Coingecko) with:
     - short timeouts, TTL caching, and env override fallback.

### Suggested job model (state machine)

Each intent we choose to work on should become a persisted “job” with explicit states.
States should be monotonic (only forward), except for controlled “retry/backoff” loops.

Example states (names flexible; persistence is the point):

- `DISCOVERED`: seen in indexer and considered a candidate.
- `ACQUIRED`: solver instance has a lease/lock to act on it.
- `CLAIM_SUBMITTED`: hub-chain claim AA op submitted.
- `CLAIMED`: claim confirmed (or at least “final enough” to proceed).
- `PLAN_READY`: chosen Tron executor key and tx plan is fixed and persisted (signed bytes stored).
- `TRON_PRE_TXS_BROADCASTED`: consolidation txs broadcast (if any).
- `TRON_FINAL_TX_BROADCASTED`: final intent-matching Tron tx broadcast.
- `TRON_FINALIZED`: Tron tx finality reached (config-driven).
- `PROOF_READY`: have `blocks[20]`, `encodedTx`, `proof[]`, `index`.
- `PROVE_SUBMITTED`: hub-chain prove AA op submitted.
- `PROVED`: prove confirmed; intent should now be solved onchain.
- `DONE`: terminal state once we’ve verified settlement (or recorded that it’s pending externally).

We also need terminal failure states:
- `SKIPPED_POLICY`: permanently rejected by policy.
- `SKIPPED_UNPROFITABLE`: permanently rejected under current pricing (maybe re-evaluate later).
- `FAILED_RETRYABLE`: transient errors with next retry time.
- `FAILED_FATAL`: permanently failed (e.g. blacklisted contract, insufficient funds, etc.).

## Persistence choice (Postgres)

Using Postgres is a good fit operationally and for restart-safety:

- Recommended: **same Postgres instance as indexer**, but a separate schema (e.g. `solver`) and role.
- The solver should own its schema and run its own migrations at startup.
- Avoid coupling to indexer internals: the solver should not read indexer tables directly; it should only use PostgREST for pool state.

Design goals for persistence:

- Idempotency via uniqueness constraints:
  - one job per `intent_id` per pool contract/chain.
  - one recorded Tron tx per `(job_id, step)` with stable `txid`.
- Concurrency via leases:
  - `leased_by`, `lease_until` columns, renewed periodically.
  - If a solver dies, another can pick up after the lease expires.
- Durable artifacts:
  - signed Tron tx bytes for each step,
  - AA userOp hashes + receipts,
  - proof blob components or a pointer to how to recompute them.

## Policy & safety model

### Static policy (operator intent)

Configurable knobs (exact env surface TBD):

- Intent type enablement: which types this solver instance will fill.
- **Contract allowlist/denylist** for `TRIGGER_SMART_CONTRACT`.
  - Support both modes:
    - allowlist-only, or
    - denylist with default-allow.
- **Selector denylist** (even if contract is allowed), e.g. `approve`, `increaseAllowance`, etc.
- Amount limits:
  - max TRX (sun), max USDT, max callValueSun, max lockPeriod, etc.
- Minimum deadline slack:
  - don’t claim if `deadline - now < X`.
  - `X` must include time for: claim confirmation (AA), Tron finality (19 blocks), proof build, prove submission,
    and reasonable buffers; treat this as a first-class reliability guard (not just a “nice to have”).
- Per-type rate limits:
  - max claims per minute, max concurrent in-flight jobs, etc.

### Dynamic circuit breakers (anti-abuse + reliability)

Persisted dynamic blacklist keyed by:

- primary: `contract_address` (address-level breaker),
- optional secondary: `(contract_address, selector)` (when failures are selector-specific).

Trigger conditions:
- Tron tx fails onchain (receipt failure),
- repeated near-max fee usage / energy burn,
- proof construction failures correlated to a contract (defensive),
- mismatch between simulation and reality (if simulation is enabled).

Backoff:
- exponential TTL: 1m → 5m → 30m → 6h → 24h (configurable).
- “cooldown until” timestamp stored in DB.

## Profitability model (include both chains)

The solver should only fill when expected profit clears thresholds:

- `expected_revenue`:
  - escrow payment (in hub token) minus any expected protocol fees (depending on intent flow).
- `expected_costs`:
  - Tron: transferred principal (TRX/USDT/callValue), expected fees/energy/bandwidth, consolidation costs.
  - Hub: AA execution costs for `claim` + `prove` (+ any follow-ups).
  - Risk buffer: slippage for fee estimation, price volatility, and “unknown unknowns”.

Recommended thresholds:
- `min_profit_usd` (absolute)
- `min_profit_bps` (relative)
- optional per-type overrides

Price inputs:
- TRX/USD from Coingecko-like API with:
  - strict timeout,
  - TTL cache,
  - last-good fallback,
  - env override.
- For MVP, restrict fills to escrow tokens we can price reliably: **hub USDT + hub USDC only**.
  - Even when escrow is USDC, the claim deposit is still hub USDT, so we also need an operational USDT float guard.

## Intent-type execution notes

### TRX_TRANSFER

- Tron final tx: `TransferContract(to, amountSun)` from chosen executor key.
- Consolidation (optional): gather TRX from other configured keys into executor before final tx.
- Implementation note: we may need to construct/sign/broadcast this tx type directly using Tron protobufs
  and existing signing helpers (not all tx types may have high-level helpers yet).

### USDT_TRANSFER

- Tron final tx: a `TriggerSmartContract` call that matches the protocol’s allowed shapes.
- Consolidation (optional): gather TRC-20 USDT to executor, then execute transfer call.
- (Optional) Preflight:
  - check token balance, allowance (if relevant), and validate calldata against allowed patterns.

### DELEGATE_RESOURCE

- Tron final tx: `DelegateResourceContract(receiver, resource, balanceSun, lock=true, lockPeriod)`.
- Inventory model:
  - treat staked TRX availability as capacity.
  - `lockPeriod` is part of the intent, so profitability should account for capital lock.
- Consolidation (optional):
  - not “multi-account delegation in one tx”, but can move TRX to one delegator account, then delegate.
- Implementation note: as with TRX transfers, we may need to construct/sign/broadcast this tx type via protobufs.

### TRIGGER_SMART_CONTRACT (gated)

- Must be strict allowlist-first to avoid abusive contracts.
- Consider hard caps:
  - calldata length,
  - callValueSun,
  - maximum fee_limit, maximum energy usage (if measurable).
- Dynamic blacklist should primarily be at contract-address granularity.

## Hub-chain interactions (Safe/4337)

We should treat hub-chain operations as first-class:

- Ensure the AA account has:
  - hub-chain ETH for gas (if needed) or a paymaster configured,
  - hub-chain USDT balance and a one-time approval to `UntronIntents` for `INTENT_CLAIM_DEPOSIT`.
- Track AA submissions:
  - store userOp hash, bundler used, submitted_at, receipt status.
  - retry on transient failures with backoff.

Important:
- Never broadcast the final Tron tx before claim confirmation (prevents claim-steal + proof-steal).
- `proveIntentFill` attempts settlement automatically if the intent is already funded.
- For some flows (virtual receiver intents), funding may happen later; record “proved but not yet paid” status.

## Operational concerns

- **Indexing lag guard**: if indexer is behind hub chain head by more than `max_head_lag_blocks`, pause new claims.
- **Tron finality guard**: wait `tron_finality_blocks` before proof/prove.
- **RPC endpoint strategy**:
  - support multiple Tron gRPC endpoints with health checks and backoff.
  - allow separate endpoint sets for reads (estimation, queries) vs writes (broadcast), e.g. own nodes for reads
    and TronGrid for fastest landing, with fallback in either direction.
- **Backpressure**:
  - global “max in-flight jobs” limit,
  - per-intent-type concurrency limits,
  - per-key concurrency limits (Tron accounts shouldn’t spam conflicting transactions).
- **Graceful shutdown**:
  - stop acquiring new leases,
  - finish or checkpoint in-flight work,
  - persist enough state to resume.

## Testing strategy

### Unit tests

- Policy engine: allow/deny + selector denylist + dynamic breaker logic.
- Profitability math: ensure bounds and sensible handling of missing prices.
- Planner: ensure “plan is deterministic once persisted” (important for restart idempotency).

### Integration tests (local)

- Use Anvil + the existing E2E harness patterns to exercise:
  - reading intents from PostgREST,
  - claiming/proving via AA (can stub bundler/paymaster or run a local bundler if available).
- For Tron: start with mocked Tron client interfaces and recorded fixtures; later add real-node fixtures.
  - Note: there is a Tron private network Docker image (`tronbox/tre`) that can act like “Tron Anvil”.
    This is useful for full e2e tests (real broadcasts + receipts), but our *production* onchain reader
    assumes a mainnet-like witness set and header encoding. If the devnet doesn’t match those assumptions,
    we can still use it by:
    - using a test-only `ITronTxReader` variant for e2e, or
    - configuring the devnet to mimic mainnet constraints (witness rotation / header layout), if possible.

### End-to-end (later)

- Compose stack:
  - Postgres + PostgREST + indexer + solver
  - local EVM chain (Anvil) for pool
  - optional Tron test harness (fixtures / devnet / remote testnet)

## Milestones / checkboxes

### Phase 0: Foundations

- [ ] Decide solver DB location (same Postgres instance + `solver` schema).
- [ ] Add solver DB migrations + a migration runner.
- [ ] Define persisted job state machine and idempotency rules.
- [ ] Implement PostgREST client primitives (GET + pagination + filters).
- [ ] Implement “indexer lag” health gating.

### Phase 1: Minimal reliable loop (no Tron yet)

- [ ] Poll `api.pool_open_intents` and create local jobs (`DISCOVERED`).
- [ ] Apply static policy filters + record `SKIPPED_*` reasons.
- [ ] Acquire leases and mark `ACQUIRED`.
- [ ] Submit AA claim tx for a chosen intent; persist submission metadata.
- [ ] Confirm claim and mark `CLAIMED`.

### Phase 2: Tron execution (start with simplest)

- [ ] TRX transfer executor:
  - build/sign/broadcast tx,
  - wait confirmation/finality,
  - persist tx bytes + txid.
- [ ] Proof builder for `TransferContract` fills.
- [ ] Hub prove submission and confirmation.

### Phase 3: USDT transfer

- [ ] Implement USDT transfer execution on Tron (one allowed call shape first).
- [ ] Add optional consolidation planning for USDT with strict limits.
- [ ] Proof builder for `TriggerSmartContract` fills.
- [ ] Profitability model expanded to include TRX/USD.

### Phase 4: Resource delegation

- [ ] Implement `DelegateResourceContract` execution.
- [ ] Implement profitability model for lock-period/capital lock (simple version acceptable).
- [ ] Add per-account “capacity accounting” to avoid overcommitting staked TRX.

### Phase 5: TRIGGER_SMART_CONTRACT (strictly gated)

- [ ] Implement strict allowlist (contract + optional selector).
- [ ] Add selector denylist defaults.
- [ ] Add contract-level dynamic breaker and persistence.
- [ ] Optional: Tron simulation preflight + “simulation-success but onchain-fail” breaker escalation.

### Phase 6: Hardening

- [ ] Restart/recovery tests: kill solver mid-flight and ensure it resumes without double-send.
- [ ] Multi-instance tests: two solvers sharing DB should not double-claim/fill the same intent.
- [ ] Rate limiting and global circuit breakers.
- [ ] Better observability: structured logs + metrics for state transitions and failure causes.

## Open questions (capture here; don’t block early progress)

- Which escrow tokens do we support in MVP for profitability?
  - Decision for MVP: support **hub USDT + hub USDC** escrow tokens only (treat as ~$1 with a small risk buffer).
  - Everything else is `SKIPPED_POLICY` until we add trustworthy pricing + optional liquidation/swap strategy.
- How strict do we want `TRIGGER_SMART_CONTRACT` gating initially (address-only vs selector-aware)?
  - Decision for MVP: support both, with address-level as the primary control:
    - contract allowlist/denylist mode, plus optional per-contract selector allowlist,
    - global selector denylist (e.g. `approve`-like calls),
    - dynamic breaker primarily keyed by contract address (optional per-(contract, selector)).
- Do we want the solver to run “keeper” behaviors (settle/close/unclaim others) or only fill?
  - Decision: always “self-keep” (resume and complete our own in-flight jobs).
  - Optional behind env flag: “global keeper” behaviors that act on other users’ intents.
- How do we want to handle receiver-originated “virtual” intents in MVP?
  - Note: the indexer can correlate forwarder events into `api.forwarder_expected_receiver_intents`,
    but only if forwarder streams are configured for the relevant origin chains. MVP can ignore this
    and fill maker-funded intents first.
- How do we want to handle receiver-originated “virtual” intents in MVP?
