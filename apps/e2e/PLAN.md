# E2E Test Plan (Gaps / Next Coverage)

This file is a prioritized checklist of important end-to-end behaviors that are **not** currently
covered by `apps/e2e/tests/*.rs`, but we expect to have high confidence in for production.

Conventions:
- Priority: **P0** = critical safety/reliability, **P1** = important, **P2** = nice-to-have.
- “Done when” bullets should be turned into concrete assertions (DB rows, onchain state, logs).
- Prefer adding a new `apps/e2e/tests/<name>.rs` per behavior; add shared helpers in `apps/e2e/src/*`.

## P0 — Protocol safety + money-moving correctness

- [x] **TriggerSmartContract intent (mock Tron) is fillable only when policy allows**
  - [x] Add test `apps/e2e/tests/solver_trigger_policy_mock.rs`
  - [x] Create a `TRIGGER_SMART_CONTRACT` intent with calldata selector present.
  - [x] Configure `SOLVER_TRIGGER_CONTRACT_ALLOWLIST_CSV` to include the target; assert it fills.
  - [x] Configure `SOLVER_TRIGGER_SELECTOR_DENYLIST_CSV` to deny the selector; assert it is skipped.
  - [x] Assert skip reason is persisted (e.g. `solver.intent_skips`) and job does not advance past `ready`.
  - Done when: we have both “fills” and “rejects” cases, and the failure case never submits a hub-chain claim.

- [x] **TriggerSmartContract circuit breaker trips on repeated onchain failures**
  - [x] Add test `apps/e2e/tests/solver_trigger_breaker.rs`
  - [x] Use a deliberately failing trigger call (e.g., contract that always reverts).
  - [x] Assert `solver.circuit_breakers` is updated with `(contract, selector)` and becomes active.
  - [x] Create a second intent with same `(contract, selector)` and assert it is skipped without claiming.
  - Done when: breaker activation + suppression is deterministic and persisted across solver restart.

- [x] **USDT_TRANSFER intent on real Tron gRPC**
  - [x] Add test `apps/e2e/tests/solver_usdt_tron_grpc.rs`
  - [x] Run `tronbox/tre` (as in existing Tron gRPC tests).
  - [x] Ensure the solver has TRC20 balance on Tron (stub token returns large `balanceOf` without SSTORE).
  - [x] Create `USDT_TRANSFER` intent and assert:
    - [x] intent becomes `solved/funded/settled` via indexer projections
    - [x] the Tron tx is a `TriggerSmartContract` to Tron USDT with `transfer(to, amount)` calldata
  - Done when: the test validates both hub settlement *and* Tron-side tx fields match intent specs.
  - Note: this uses a TRC20-like stub token (no storage writes) so it does not assert receiver balance changes.

- [x] **Multi-key Tron inventory + consolidation plan is correct and restart-safe**
  - [x] Add test `apps/e2e/tests/solver_tron_consolidation.rs`
  - [x] Configure `TRON_PRIVATE_KEYS_HEX_CSV` with ≥2 keys, and set `SOLVER_CONSOLIDATION_*` caps.
  - [x] Create TRX transfer intent that requires consolidation (no single key has enough funds).
  - [x] Assert `solver.tron_signed_txs` contains ordered `pre:*` steps + `final`.
  - [x] Kill the solver after some `pre:*` txs are broadcast; restart; ensure it resumes idempotently.
  - Done when: no duplicate broadcasts and the final tx is never broadcast before hub claim is confirmed.

## P0 — Safe4337 correctness (AA nonces/receipts/failure modes)

- [x] **Safe4337: nonce-floor / “AA25 invalid account nonce” recovery**
  - [x] Add test `apps/e2e/tests/solver_safe4337_nonce_recovery.rs`
  - [x] Force a stale prepared userop scenario (sleep/backoff or inject extra userop externally).
  - [x] Assert solver deletes stale prepared ops and re-prepares with a valid nonce.
  - Done when: the job completes without manual intervention and receipts are persisted.

- [x] **Safe4337: “AlreadyClaimed” and other hub reverts are fatal and stop retries**
  - [x] Add test `apps/e2e/tests/solver_safe4337_already_claimed.rs`
  - [x] Create intent; claim it from a different address; let solver attempt.
  - [x] Assert solver records `failed_fatal` and does not keep re-submitting userops.
  - Done when: DB state stabilizes (no growing attempts) and job is terminal.

## P1 — Operational reliability + guardrails

- [x] **Profitability gating and pricing fallbacks**
  - [x] Add test `apps/e2e/tests/solver_profitability.rs`
  - [x] Set `SOLVER_MIN_PROFIT_USD` high; create small-escrow intents; assert they are skipped.
  - [x] Configure `SOLVER_REQUIRE_PRICED_ESCROW=true` and a non-allowed token; assert skip reason.
  - [x] Simulate pricing outage (invalid URL / timeouts) and assert solver falls back to configured costs.
  - Done when: “skip” paths are asserted without any hub-chain claim submission.

- [x] **Rate limiting + global pause**
  - [x] Add test `apps/e2e/tests/solver_rate_limit_and_pause.rs`
  - [x] Configure `SOLVER_RATE_LIMIT_CLAIMS_PER_MINUTE_*` low; create N intents; assert claims are throttled.
  - [x] Trigger global pause (or set via DB) and assert ticks do not claim while paused (`apps/e2e/tests/solver_global_pause.rs`).
  - Done when: claim submission rate is bounded and pause is enforced across restarts.

- [x] **Indexer lag guard blocks claiming**
  - [x] Add test `apps/e2e/tests/solver_indexer_lag_guard.rs`
  - [x] Artificially advance hub head (produce blocks) without indexing them, then start solver.
  - [x] Assert solver logs “indexer lag too high” and does not claim (via absence of `solver.jobs`).
  - Done when: the solver resumes claiming once lag is below threshold (or after indexer catches up).

- [x] **PostgREST outage / flakiness handling**
  - [x] Add test `apps/e2e/tests/solver_postgrest_outage.rs`
  - [x] Kill PostgREST during run; keep solver running; restart PostgREST.
  - [x] Assert solver recovers and continues (no permanent fatal, no duplicate fills).
  - Done when: jobs still reach `done` and errors remain retryable.

## P1 — Tron error handling / fee mechanics

- [x] **Tron node busy / transient errors are retried with backoff**
  - [x] Add test `apps/e2e/tests/solver_tron_retry_backoff.rs`
  - [x] Inject failures (in-process gRPC TCP proxy).
  - [x] Assert retryable errors bump `attempts` and set `next_retry_at` into the future.
  - Done when: transient failures do not produce `failed_fatal`.

- [x] **Energy rental providers (if enabled) are exercised**
  - [x] Add test `apps/e2e/tests/solver_tron_energy_rental.rs`
  - [x] Configure `TRON_ENERGY_RENTAL_APIS_JSON` to a local stub and assert solver attempts rental.
  - Done when: we assert the HTTP call shape and that the solver proceeds with expected fee limits.

- [x] **DelegateResource resell via rental APIs (provider tx owner + fallback/freeze)**
  - [x] Add test `apps/e2e/tests/solver_tron_delegate_resell.rs`
  - [x] Configure `TRON_DELEGATE_RESOURCE_RESELL_ENABLED=true` and multiple providers (first fails).
  - [x] Assert the proved Tron tx owner is the provider (not the solver).
  - [x] Assert the failing provider is frozen and the solver falls back to the next provider.
  - Done when: the job reaches `done` and request/response are persisted for postmortems.

## P2 — Proof/security fidelity + adversarial scenarios

- [x] **Proof verification with signature validation (avoid “no-sig reader” shortcuts)**
  - [x] Add test `apps/e2e/tests/solver_tron_proof_sig_verified.rs`
  - [x] Use a signature-checking reader (`TestTronTxReaderSigAllowlist`) and assert prove succeeds only with valid tx bytes.
  - [x] Done when: the test fails if tx bytes are mutated (and signatures must be recoverable onchain).

- [x] **Competing solvers and “don’t broadcast final Tron tx until claim confirmed”**
  - [x] Add test `apps/e2e/tests/solver_competition_race.rs`
  - [x] Run two solver instances with different hub keys; ensure only claimant broadcasts final tx.
  - Done when: non-claimant never produces a matching final Tron tx that could be stolen for proof.
