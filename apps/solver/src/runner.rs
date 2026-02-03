use crate::{
    config::{AppConfig, HubTxMode, TronMode},
    db::SolverDb,
    db::{HubUserOpKind, SolverJob, TronProofRow, TronTxCostsRow},
    hub_cost::estimate_hub_cost_usd_from_userops,
    indexer::{IndexerClient, PoolOpenIntentRow},
    metrics::SolverTelemetry,
    policy::{BreakerQuery, PolicyEngine},
    pricing::Pricing,
    tron_backend::TronBackend,
    tron_backend::TronExecution,
    types::{IntentType, parse_b256, parse_hex_bytes},
};
use crate::{hub::HubClient, hub::TronProof};
use alloy::primitives::{B256, U256};
use alloy::rpc::types::eth::erc4337::PackedUserOperation;
use alloy::sol_types::SolCall;
use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

const INTENT_CLAIM_DEPOSIT: u64 = 1_000_000;
const LEASE_FOR_SECS: u64 = 30;

pub struct Solver {
    cfg: AppConfig,
    telemetry: SolverTelemetry,
    db: SolverDb,
    indexer: IndexerClient,
    hub: Arc<HubClient>,
    tron: TronBackend,
    pricing: Pricing,
    policy: PolicyEngine,
    instance_id: String,
    hub_userop_submit_sem: Arc<Semaphore>,
}

#[derive(Clone)]
struct JobCtx {
    cfg: AppConfig,
    db: SolverDb,
    hub: Arc<HubClient>,
    tron: TronBackend,
    instance_id: String,
    hub_userop_submit_sem: Arc<Semaphore>,
}

impl Solver {
    pub async fn new(cfg: AppConfig, telemetry: SolverTelemetry) -> Result<Self> {
        let db = SolverDb::connect(&cfg.db_url, 10).await?;
        db.migrate().await?;

        let hub = match cfg.hub.tx_mode {
            HubTxMode::Eoa => {
                HubClient::new_eoa(
                    &cfg.hub.rpc_url,
                    cfg.hub.chain_id,
                    cfg.hub.pool,
                    cfg.hub.signer_private_key,
                    telemetry.clone(),
                )
                .await?
            }
            HubTxMode::Safe4337 => {
                let entrypoint = cfg
                    .hub
                    .entrypoint
                    .context("missing HUB_ENTRYPOINT_ADDRESS")?;
                let module = cfg
                    .hub
                    .safe_4337_module
                    .context("missing HUB_SAFE_4337_MODULE_ADDRESS")?;
                let paymasters = cfg
                    .hub
                    .paymasters
                    .iter()
                    .map(|pm| aa::paymaster::PaymasterService {
                        url: pm.url.clone(),
                        context: pm.context.clone(),
                    })
                    .collect::<Vec<_>>();

                HubClient::new_safe4337(
                    &cfg.hub.rpc_url,
                    cfg.hub.chain_id,
                    cfg.hub.pool,
                    entrypoint,
                    cfg.hub.safe,
                    module,
                    cfg.hub.safe_deployment.clone(),
                    cfg.hub.bundler_urls.clone(),
                    paymasters,
                    cfg.hub.signer_private_key,
                    telemetry.clone(),
                )
                .await?
            }
        };
        let hub = Arc::new(hub);

        // For Safe4337 mode: on restart, the bundler may have pending userops that are not yet
        // reflected in EntryPoint.getNonce(). Seed a local nonce floor from our persisted
        // submitted userops to avoid AA25 invalid nonce loops.
        if cfg.hub.tx_mode == HubTxMode::Safe4337 {
            if let Some(floor) = db
                .hub_userop_nonce_floor_for_sender(hub.solver_address())
                .await?
            {
                hub.safe4337_set_nonce_floor(floor).await?;
            }
        }

        let indexer = IndexerClient::new(
            cfg.indexer.base_url.clone(),
            cfg.indexer.timeout,
            telemetry.clone(),
        );

        let tron = TronBackend::new(cfg.tron.clone(), cfg.jobs.clone(), telemetry.clone());
        let pricing = Pricing::new(cfg.pricing.clone());
        let policy = PolicyEngine::new(cfg.policy.clone());

        Ok(Self {
            instance_id: cfg.instance_id.clone(),
            cfg,
            telemetry,
            db,
            indexer,
            hub,
            tron,
            pricing,
            policy,
            hub_userop_submit_sem: Arc::new(Semaphore::new(1)),
        })
    }

    pub async fn run(mut self, shutdown: CancellationToken) -> Result<()> {
        let mut interval = tokio::time::interval(self.cfg.jobs.tick_interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = shutdown.cancelled() => {
                    tracing::info!("shutdown");
                    return Ok(());
                }
                _ = interval.tick() => {}
            }

            let started = Instant::now();
            let res = self.tick().await;
            match res {
                Ok(()) => self
                    .telemetry
                    .job_ok("tick", started.elapsed().as_millis() as u64),
                Err(err) => {
                    self.telemetry
                        .job_err("tick", started.elapsed().as_millis() as u64);
                    tracing::warn!(err = %err, "tick failed");
                }
            }
        }
    }

    async fn tick(&mut self) -> Result<()> {
        self.indexer.health().await?;

        // Indexer lag guard: do not claim if we're too far behind head.
        match self.indexer.latest_indexed_pool_block_number().await {
            Ok(Some(indexed)) => {
                let head = self.hub.hub_block_number().await?;
                let lag = head.saturating_sub(indexed);
                if lag > self.cfg.indexer.max_head_lag_blocks {
                    tracing::warn!(
                        head,
                        indexed,
                        lag,
                        max_lag = self.cfg.indexer.max_head_lag_blocks,
                        "indexer lag too high; skipping tick"
                    );
                    return Ok(());
                }
            }
            Ok(None) => {}
            Err(err) => {
                tracing::warn!(err = %err, "failed to query indexer lag; continuing without lag guard");
            }
        }

        let rows = self
            .indexer
            .fetch_open_intents(self.cfg.jobs.fill_max_claims)
            .await?;

        for row in rows {
            if !self.should_attempt(&row).await? {
                continue;
            }
            let id = parse_b256(&row.id)?;
            let specs = parse_hex_bytes(&row.intent_specs)?;
            self.db
                .insert_job_if_new(b256_to_bytes32(id), row.intent_type, &specs, row.deadline)
                .await?;
        }

        let jobs = self
            .db
            .lease_jobs(
                &self.instance_id,
                std::time::Duration::from_secs(LEASE_FOR_SECS),
                i64::try_from(self.cfg.jobs.fill_max_claims)
                    .unwrap_or(50)
                    .max(1),
            )
            .await?;

        let ctx = JobCtx {
            cfg: self.cfg.clone(),
            db: self.db.clone(),
            hub: self.hub.clone(),
            tron: self.tron.clone(),
            instance_id: self.instance_id.clone(),
            hub_userop_submit_sem: self.hub_userop_submit_sem.clone(),
        };

        let mut set = JoinSet::new();
        for job in jobs {
            let ctx = ctx.clone();
            set.spawn(async move {
                if let Err(err) = process_job(ctx, job).await {
                    tracing::warn!(err = %err, "job failed");
                }
            });
        }
        while let Some(res) = set.join_next().await {
            if let Err(err) = res {
                tracing::warn!(err = %err, "job task panicked");
            }
        }
        Ok(())
    }

    async fn should_attempt(&mut self, row: &PoolOpenIntentRow) -> Result<bool> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let hub_cost_usd = self.estimate_hub_cost_usd().await?;
        let tron_fee_usd = self.estimate_tron_fee_usd(row.intent_type).await?;
        let eval = self
            .policy
            .evaluate_open_intent(row, now, &mut self.pricing, hub_cost_usd, tron_fee_usd)
            .await?;
        if !eval.allowed {
            if let Ok(id) = parse_b256(&row.id) {
                let _ = self
                    .db
                    .upsert_intent_skip(
                        b256_to_bytes32(id),
                        row.intent_type,
                        eval.reason.as_deref().unwrap_or("policy_reject"),
                        None,
                    )
                    .await;
            }
            if let Some(reason) = eval.reason.as_deref() {
                tracing::debug!(id = %row.id, intent_type = row.intent_type, reason, "skip intent");
            }
            return Ok(false);
        }

        // Dynamic breaker (if applicable).
        if let Some(b) = eval.breaker {
            if self.is_breaker_active(b).await? {
                if let Ok(id) = parse_b256(&row.id) {
                    let _ = self
                        .db
                        .upsert_intent_skip(
                            b256_to_bytes32(id),
                            row.intent_type,
                            "breaker_active",
                            None,
                        )
                        .await;
                }
                return Ok(false);
            }
        }

        // Optional Tron emulation gating: avoid claiming intents we know will revert.
        if self.cfg.tron.emulation_enabled && self.cfg.tron.mode == TronMode::Grpc {
            let ty = IntentType::from_i16(row.intent_type)?;
            if matches!(
                ty,
                IntentType::TriggerSmartContract | IntentType::UsdtTransfer
            ) {
                let specs = parse_hex_bytes(&row.intent_specs)?;
                let emu = self
                    .tron
                    .precheck_emulation(self.hub.as_ref(), ty, &specs)
                    .await;
                if !emu.ok {
                    if let Ok(id) = parse_b256(&row.id) {
                        let _ = self
                            .db
                            .upsert_intent_skip(
                                b256_to_bytes32(id),
                                row.intent_type,
                                emu.reason.as_deref().unwrap_or("tron_emulation_failed"),
                                None,
                            )
                            .await;
                    }
                    tracing::debug!(
                        id = %row.id,
                        intent_type = row.intent_type,
                        reason = emu.reason.as_deref().unwrap_or("tron_emulation_failed"),
                        "skip intent (tron emulation)"
                    );
                    return Ok(false);
                }
            }
        }

        Ok(true)
    }

    async fn estimate_hub_cost_usd(&mut self) -> Result<f64> {
        if self.cfg.hub.tx_mode != HubTxMode::Safe4337 {
            return Ok(self.cfg.policy.hub_cost_usd);
        }

        let eth_usd = match self.pricing.eth_usd().await {
            Ok(v) => v,
            Err(err) => {
                tracing::warn!(
                    err = %err,
                    "eth_usd unavailable; using SOLVER_HUB_COST_USD fallback"
                );
                return Ok(self.cfg.policy.hub_cost_usd);
            }
        };

        let lookback = i64::try_from(self.cfg.policy.hub_cost_history_lookback).unwrap_or(50);
        let claim = self
            .db
            .hub_userop_avg_actual_gas_cost_wei(HubUserOpKind::Claim, lookback)
            .await?;
        let prove = self
            .db
            .hub_userop_avg_actual_gas_cost_wei(HubUserOpKind::Prove, lookback)
            .await?;

        let (Some(claim), Some(prove)) = (claim, prove) else {
            return Ok(self.cfg.policy.hub_cost_usd);
        };

        Ok(estimate_hub_cost_usd_from_userops(
            eth_usd,
            claim,
            prove,
            self.cfg.policy.hub_cost_headroom_ppm,
        )
        .unwrap_or(self.cfg.policy.hub_cost_usd))
    }

    async fn is_breaker_active(&self, _b: BreakerQuery) -> Result<bool> {
        self.db.breaker_is_active(_b.contract, _b.selector).await
    }

    async fn estimate_tron_fee_usd(&mut self, intent_type: i16) -> Result<f64> {
        let trx_usd = match self.pricing.trx_usd().await {
            Ok(v) => v,
            Err(_) => return Ok(self.cfg.policy.tron_fee_usd),
        };

        let lookback = i64::try_from(self.cfg.policy.tron_fee_history_lookback).unwrap_or(50);
        let fee_sun = self
            .db
            .tron_tx_costs_avg_fee_sun(intent_type, lookback)
            .await?
            .unwrap_or(0);
        if fee_sun <= 0 {
            return Ok(self.cfg.policy.tron_fee_usd);
        }
        let mut usd = (fee_sun as f64 / 1e6) * trx_usd;
        usd *= 1.0 + (self.cfg.policy.tron_fee_headroom_ppm as f64 / 1e6);
        Ok(usd)
    }
}

async fn process_job(ctx: JobCtx, job: SolverJob) -> Result<()> {
    let id = B256::from_slice(&job.intent_id);
    let ty = IntentType::from_i16(job.intent_type)?;

    let retry_in = |attempts: i32| {
        // Exponential backoff with caps. This is intentionally simple and centralized.
        let shift = u32::try_from(attempts.max(0).min(10)).unwrap_or(0);
        let base = 1u64.checked_shl(shift).unwrap_or(u64::MAX);
        std::time::Duration::from_secs(base.min(300))
    };

    match job.state.as_str() {
        "ready" => {
            tracing::info!(id = %id, intent_type = job.intent_type, "claiming intent");
            // Ensure claim deposit can be pulled. We do this here (rather than at startup) so:
            // - the solver starts even if AA infrastructure is temporarily down
            // - retries are controlled by the job state machine
            let usdt = match ctx.hub.pool_usdt().await {
                Ok(v) => v,
                Err(err) => {
                    let msg = format!("pool_usdt failed: {err:#}");
                    ctx.db
                        .record_retryable_error(
                            job.job_id,
                            &ctx.instance_id,
                            &msg,
                            retry_in(job.attempts),
                        )
                        .await?;
                    return Ok(());
                }
            };
            if let Err(err) = ctx
                .hub
                .ensure_erc20_allowance(
                    usdt,
                    ctx.hub.pool_address(),
                    U256::from(INTENT_CLAIM_DEPOSIT),
                )
                .await
            {
                let msg = format!("ensure_erc20_allowance failed: {err:#}");
                ctx.db
                    .record_retryable_error(
                        job.job_id,
                        &ctx.instance_id,
                        &msg,
                        retry_in(job.attempts),
                    )
                    .await?;
                return Ok(());
            }
            match ctx.cfg.hub.tx_mode {
                HubTxMode::Eoa => match ctx.hub.claim_intent(id).await {
                    Ok(receipt) => {
                        ctx.db
                            .record_claim(
                                job.job_id,
                                &ctx.instance_id,
                                b256_to_bytes32(receipt.transaction_hash),
                            )
                            .await?;
                        Ok(())
                    }
                    Err(err) => {
                        let msg = err.to_string();
                        // If already claimed, don't keep retrying.
                        if msg.contains("AlreadyClaimed") {
                            ctx.db
                                .record_fatal_error(job.job_id, &ctx.instance_id, &msg)
                                .await?;
                            return Ok(());
                        }
                        ctx.db
                            .record_retryable_error(
                                job.job_id,
                                &ctx.instance_id,
                                &msg,
                                retry_in(job.attempts),
                            )
                            .await?;
                        Ok(())
                    }
                },
                HubTxMode::Safe4337 => {
                    let kind = HubUserOpKind::Claim;
                    let mut row = ctx.db.get_hub_userop(job.job_id, kind).await?;
                    if let Some(r) = row.as_ref() {
                        // If we've already included it, we should have advanced state.
                        if r.state == "included" {
                            return Ok(());
                        }
                        // If we have a prepared-but-not-submitted op, ensure it isn't stale
                        // (nonce already used onchain). Stale prepared ops can happen if we
                        // back off for a long time while other ops progress.
                        if r.userop_hash.is_none() && r.state == "prepared" {
                            let u: PackedUserOperation = serde_json::from_str(&r.userop_json)
                                .context("deserialize claim userop")?;
                            let chain_nonce = ctx.hub.safe4337_chain_nonce().await?;
                            if u.nonce < chain_nonce {
                                ctx.db
                                    .delete_hub_userop_prepared(job.job_id, &ctx.instance_id, kind)
                                    .await
                                    .ok();
                                row = None;
                            }
                        }
                    }

                    match row {
                        None => {
                            let _permit = ctx
                                .hub_userop_submit_sem
                                .acquire()
                                .await
                                .context("acquire hub_userop_submit_sem (claim)")?;
                            let call = crate::hub::IUntronIntents::claimIntentCall { id };
                            let userop = ctx
                                .hub
                                .safe4337_build_call_userop(
                                    ctx.hub.pool_address(),
                                    call.abi_encode(),
                                )
                                .await
                                .context("build claimIntent userop")?;
                            let json =
                                serde_json::to_string(&userop).context("serialize claim userop")?;
                            ctx.db
                                .insert_hub_userop_prepared(
                                    job.job_id,
                                    &ctx.instance_id,
                                    kind,
                                    &json,
                                )
                                .await?;
                            match ctx.hub.safe4337_send_userop(userop).await {
                                Ok(userop_hash) => {
                                    ctx.db
                                        .record_hub_userop_submitted(
                                            job.job_id,
                                            &ctx.instance_id,
                                            kind,
                                            &userop_hash,
                                        )
                                        .await?;
                                }
                                Err(err) => {
                                    let msg = err.to_string();
                                    if msg.contains("AA25 invalid account nonce") {
                                        ctx.db
                                            .delete_hub_userop_prepared(
                                                job.job_id,
                                                &ctx.instance_id,
                                                kind,
                                            )
                                            .await
                                            .ok();
                                    }
                                    ctx.db
                                        .record_hub_userop_retryable_error(
                                            job.job_id,
                                            &ctx.instance_id,
                                            kind,
                                            &msg,
                                            retry_in(job.attempts),
                                        )
                                        .await
                                        .ok();
                                    ctx.db
                                        .record_retryable_error(
                                            job.job_id,
                                            &ctx.instance_id,
                                            &msg,
                                            retry_in(job.attempts),
                                        )
                                        .await?;
                                    return Ok(());
                                }
                            }
                        }
                        Some(r) => {
                            if r.userop_hash.is_none() && r.state == "prepared" {
                                let _permit = ctx
                                    .hub_userop_submit_sem
                                    .acquire()
                                    .await
                                    .context("acquire hub_userop_submit_sem (claim)")?;
                                let u: PackedUserOperation = serde_json::from_str(&r.userop_json)
                                    .context("deserialize claim userop")?;
                                match ctx.hub.safe4337_send_userop(u).await {
                                    Ok(userop_hash) => {
                                        ctx.db
                                            .record_hub_userop_submitted(
                                                job.job_id,
                                                &ctx.instance_id,
                                                kind,
                                                &userop_hash,
                                            )
                                            .await?;
                                    }
                                    Err(err) => {
                                        let msg = err.to_string();
                                        if msg.contains("AA25 invalid account nonce") {
                                            ctx.db
                                                .delete_hub_userop_prepared(
                                                    job.job_id,
                                                    &ctx.instance_id,
                                                    kind,
                                                )
                                                .await
                                                .ok();
                                        }
                                        ctx.db
                                            .record_hub_userop_retryable_error(
                                                job.job_id,
                                                &ctx.instance_id,
                                                kind,
                                                &msg,
                                                retry_in(job.attempts),
                                            )
                                            .await
                                            .ok();
                                        ctx.db
                                            .record_retryable_error(
                                                job.job_id,
                                                &ctx.instance_id,
                                                &msg,
                                                retry_in(job.attempts),
                                            )
                                            .await?;
                                        return Ok(());
                                    }
                                }
                            }
                        }
                    }

                    // Poll receipt if we have a userop hash.
                    let row = ctx.db.get_hub_userop(job.job_id, kind).await?;
                    let Some(r) = row else {
                        return Ok(());
                    };
                    let Some(userop_hash) = r.userop_hash.clone() else {
                        return Ok(());
                    };

                    match ctx.hub.safe4337_get_userop_receipt(&userop_hash).await {
                        Ok(Some(receipt)) => {
                            let Some(tx_hash) = receipt.tx_hash else {
                                return Ok(());
                            };
                            let success = receipt.success.unwrap_or(false);
                            let receipt_json = serde_json::to_string(&receipt.raw)
                                .unwrap_or_else(|_| "{}".to_string());
                            ctx.db
                                .record_hub_userop_included(
                                    job.job_id,
                                    &ctx.instance_id,
                                    kind,
                                    b256_to_bytes32(tx_hash),
                                    receipt.block_number.map(|n| n as i64),
                                    success,
                                    receipt.actual_gas_cost_wei,
                                    receipt.actual_gas_used,
                                    &receipt_json,
                                )
                                .await?;
                            if success {
                                ctx.db
                                    .record_claim(
                                        job.job_id,
                                        &ctx.instance_id,
                                        b256_to_bytes32(tx_hash),
                                    )
                                    .await?;
                            } else {
                                let msg = format!(
                                    "claim userop failed: {:?}",
                                    receipt.reason.unwrap_or(serde_json::Value::Null)
                                );
                                ctx.db
                                    .record_hub_userop_fatal_error(
                                        job.job_id,
                                        &ctx.instance_id,
                                        kind,
                                        &msg,
                                    )
                                    .await
                                    .ok();
                                ctx.db
                                    .record_fatal_error(job.job_id, &ctx.instance_id, &msg)
                                    .await?;
                            }
                            Ok(())
                        }
                        Ok(None) => Ok(()),
                        Err(err) => {
                            let msg = err.to_string();
                            ctx.db
                                .record_hub_userop_retryable_error(
                                    job.job_id,
                                    &ctx.instance_id,
                                    kind,
                                    &msg,
                                    retry_in(job.attempts),
                                )
                                .await
                                .ok();
                            ctx.db
                                .record_retryable_error(
                                    job.job_id,
                                    &ctx.instance_id,
                                    &msg,
                                    retry_in(job.attempts),
                                )
                                .await?;
                            Ok(())
                        }
                    }
                }
            }
        }
        "claimed" => {
            if ctx.cfg.tron.mode == TronMode::Mock {
                tracing::info!(id = %id, "executing tron tx (mock)");
                let exec_res = match ty {
                    IntentType::TriggerSmartContract => ctx
                        .tron
                        .prepare_trigger_smart_contract(ctx.hub.as_ref(), id, &job.intent_specs)
                        .await
                        .context("execute trigger smart contract"),
                    IntentType::TrxTransfer => ctx
                        .tron
                        .prepare_trx_transfer(ctx.hub.as_ref(), id, &job.intent_specs)
                        .await
                        .context("execute trx transfer"),
                    IntentType::UsdtTransfer => ctx
                        .tron
                        .prepare_usdt_transfer(ctx.hub.as_ref(), id, &job.intent_specs)
                        .await
                        .context("execute usdt transfer"),
                    IntentType::DelegateResource => ctx
                        .tron
                        .prepare_delegate_resource(ctx.hub.as_ref(), id, &job.intent_specs)
                        .await
                        .context("execute delegate resource"),
                };

                let exec = match exec_res {
                    Ok(v) => v,
                    Err(err) => {
                        let msg = err.to_string();

                        // If this is a trigger smart contract, record a breaker on likely-deterministic failures.
                        if ty == IntentType::TriggerSmartContract
                            && !looks_like_tron_server_busy(&msg)
                            && looks_like_tron_contract_failure(&msg)
                        {
                            if let Some((contract, selector)) =
                                decode_trigger_contract_and_selector(&job.intent_specs)
                            {
                                let _ = ctx
                                    .db
                                    .breaker_record_failure(contract, selector, &msg)
                                    .await;
                            }
                        }

                        ctx.db
                            .record_retryable_error(
                                job.job_id,
                                &ctx.instance_id,
                                &msg,
                                retry_in(job.attempts),
                            )
                            .await?;
                        return Ok(());
                    }
                };

                match exec {
                    TronExecution::ImmediateProof(tron) => {
                        // Store proof first; submit prove in the next tick (restart-safe).
                        let proof_row = TronProofRow {
                            blocks: tron.blocks.into_iter().collect(),
                            encoded_tx: tron.encoded_tx,
                            proof: tron
                                .proof
                                .into_iter()
                                .map(|b| b.as_slice().to_vec())
                                .collect(),
                            index_dec: tron.index.to_string(),
                        };
                        let txid = job.tron_txid.unwrap_or_else(|| {
                            b256_to_bytes32(alloy::primitives::keccak256(id.as_slice()))
                        });
                        ctx.db.save_tron_proof(txid, &proof_row).await?;
                        ctx.db
                            .record_tron_txid(job.job_id, &ctx.instance_id, txid)
                            .await?;
                        ctx.db
                            .record_proof_built(job.job_id, &ctx.instance_id)
                            .await?;
                        Ok(())
                    }
                    TronExecution::PreparedTx(_) => {
                        anyhow::bail!("unexpected PreparedTx in TRON_MODE=mock")
                    }
                }
            } else {
                tracing::info!(id = %id, "preparing tron tx (persist signed bytes)");
                let exec_res = match ty {
                    IntentType::TriggerSmartContract => ctx
                        .tron
                        .prepare_trigger_smart_contract(ctx.hub.as_ref(), id, &job.intent_specs)
                        .await
                        .context("prepare trigger smart contract"),
                    IntentType::TrxTransfer => ctx
                        .tron
                        .prepare_trx_transfer(ctx.hub.as_ref(), id, &job.intent_specs)
                        .await
                        .context("prepare trx transfer"),
                    IntentType::UsdtTransfer => ctx
                        .tron
                        .prepare_usdt_transfer(ctx.hub.as_ref(), id, &job.intent_specs)
                        .await
                        .context("prepare usdt transfer"),
                    IntentType::DelegateResource => ctx
                        .tron
                        .prepare_delegate_resource(ctx.hub.as_ref(), id, &job.intent_specs)
                        .await
                        .context("prepare delegate resource"),
                };

                let exec = match exec_res {
                    Ok(v) => v,
                    Err(err) => {
                        let msg = err.to_string();
                        if ty == IntentType::TriggerSmartContract
                            && !looks_like_tron_server_busy(&msg)
                            && looks_like_tron_contract_failure(&msg)
                        {
                            if let Some((contract, selector)) =
                                decode_trigger_contract_and_selector(&job.intent_specs)
                            {
                                let _ = ctx
                                    .db
                                    .breaker_record_failure(contract, selector, &msg)
                                    .await;
                            }
                        }

                        ctx.db
                            .record_retryable_error(
                                job.job_id,
                                &ctx.instance_id,
                                &msg,
                                retry_in(job.attempts),
                            )
                            .await?;
                        return Ok(());
                    }
                };

                match exec {
                    TronExecution::ImmediateProof(_) => {
                        anyhow::bail!("unexpected ImmediateProof in TRON_MODE=grpc")
                    }
                    TronExecution::PreparedTx(p) => {
                        ctx.db
                            .record_tron_prepared(
                                job.job_id,
                                &ctx.instance_id,
                                p.txid,
                                &p.tx_bytes,
                                p.fee_limit_sun,
                                p.energy_required,
                                p.tx_size_bytes,
                            )
                            .await?;
                        Ok(())
                    }
                }
            }
        }
        "tron_prepared" => {
            let Some(txid) = job.tron_txid else {
                ctx.db
                    .record_retryable_error(
                        job.job_id,
                        &ctx.instance_id,
                        "missing tron_txid",
                        retry_in(job.attempts),
                    )
                    .await?;
                return Ok(());
            };

            // If we already see the tx onchain, just move forward. This avoids double-broadcasts
            // across crashes between broadcast and state update.
            if ctx.tron.tx_is_known(txid).await {
                ctx.db
                    .record_tron_txid(job.job_id, &ctx.instance_id, txid)
                    .await?;
                return Ok(());
            }

            let tx_bytes = ctx.db.load_tron_signed_tx_bytes(txid).await?;
            if let Err(err) = ctx.tron.broadcast_signed_tx(&tx_bytes).await {
                let msg = err.to_string();
                ctx.db
                    .record_retryable_error(
                        job.job_id,
                        &ctx.instance_id,
                        &msg,
                        retry_in(job.attempts),
                    )
                    .await?;
                return Ok(());
            }

            ctx.db
                .record_tron_txid(job.job_id, &ctx.instance_id, txid)
                .await?;
            Ok(())
        }
        "tron_sent" => {
            let Some(txid) = job.tron_txid else {
                ctx.db
                    .record_retryable_error(
                        job.job_id,
                        &ctx.instance_id,
                        "missing tron_txid",
                        retry_in(job.attempts),
                    )
                    .await?;
                return Ok(());
            };
            tracing::info!(id = %id, "building tron proof");
            let tron = match ctx.tron.build_proof(txid).await {
                Ok(v) => v,
                Err(err) => {
                    let msg = err.to_string();
                    if msg.contains("tron_tx_failed:") {
                        ctx.db
                            .record_fatal_error(job.job_id, &ctx.instance_id, &msg)
                            .await?;
                        return Ok(());
                    }
                    ctx.db
                        .record_retryable_error(
                            job.job_id,
                            &ctx.instance_id,
                            &msg,
                            retry_in(job.attempts),
                        )
                        .await?;
                    return Ok(());
                }
            };
            let proof_row = TronProofRow {
                blocks: tron.blocks.into_iter().collect(),
                encoded_tx: tron.encoded_tx,
                proof: tron
                    .proof
                    .into_iter()
                    .map(|b| b.as_slice().to_vec())
                    .collect(),
                index_dec: tron.index.to_string(),
            };
            ctx.db.save_tron_proof(txid, &proof_row).await?;

            if let Ok(Some(info)) = ctx.tron.fetch_transaction_info(txid).await {
                let receipt = info.receipt.as_ref();
                let costs = TronTxCostsRow {
                    fee_sun: Some(info.fee),
                    energy_usage_total: receipt.map(|r| r.energy_usage_total),
                    net_usage: receipt.map(|r| r.net_usage),
                    energy_fee_sun: receipt.map(|r| r.energy_fee),
                    net_fee_sun: receipt.map(|r| r.net_fee),
                    block_number: Some(info.block_number),
                    block_timestamp: Some(info.block_time_stamp),
                    result_code: Some(info.result),
                    result_message: Some(String::from_utf8_lossy(&info.res_message).into_owned()),
                };
                let _ = ctx.db.upsert_tron_tx_costs(job.job_id, txid, &costs).await;
            }

            ctx.db
                .record_proof_built(job.job_id, &ctx.instance_id)
                .await?;
            Ok(())
        }
        "proof_built" => {
            let Some(txid) = job.tron_txid else {
                ctx.db
                    .record_retryable_error(
                        job.job_id,
                        &ctx.instance_id,
                        "missing tron_txid",
                        retry_in(job.attempts),
                    )
                    .await?;
                return Ok(());
            };
            let proof = ctx.db.load_tron_proof(txid).await?;
            let tron = TronProof {
                blocks: std::array::from_fn(|i| proof.blocks[i].clone()),
                encoded_tx: proof.encoded_tx,
                proof: proof
                    .proof
                    .into_iter()
                    .map(|b| B256::from_slice(&b))
                    .collect(),
                index: crate::types::parse_u256_dec(&proof.index_dec).unwrap_or(U256::ZERO),
            };
            tracing::info!(id = %id, "submitting proveIntentFill");
            match ctx.cfg.hub.tx_mode {
                HubTxMode::Eoa => match ctx.hub.prove_intent_fill(id, tron).await {
                    Ok(receipt) => {
                        ctx.db
                            .record_prove(
                                job.job_id,
                                &ctx.instance_id,
                                b256_to_bytes32(receipt.transaction_hash),
                            )
                            .await?;
                        Ok(())
                    }
                    Err(err) => {
                        let msg = err.to_string();
                        ctx.db
                            .record_retryable_error(
                                job.job_id,
                                &ctx.instance_id,
                                &msg,
                                retry_in(job.attempts),
                            )
                            .await?;
                        Ok(())
                    }
                },
                HubTxMode::Safe4337 => {
                    let kind = HubUserOpKind::Prove;
                    let mut row = ctx.db.get_hub_userop(job.job_id, kind).await?;
                    if let Some(r) = row.as_ref() {
                        if r.state == "included" {
                            return Ok(());
                        }
                        if r.userop_hash.is_none() && r.state == "prepared" {
                            let u: PackedUserOperation = serde_json::from_str(&r.userop_json)
                                .context("deserialize prove userop")?;
                            let chain_nonce = ctx.hub.safe4337_chain_nonce().await?;
                            if u.nonce < chain_nonce {
                                ctx.db
                                    .delete_hub_userop_prepared(job.job_id, &ctx.instance_id, kind)
                                    .await
                                    .ok();
                                row = None;
                            }
                        }
                    }

                    match row {
                        None => {
                            let _permit = ctx
                                .hub_userop_submit_sem
                                .acquire()
                                .await
                                .context("acquire hub_userop_submit_sem (prove)")?;
                            let call = crate::hub::IUntronIntents::proveIntentFillCall {
                                id,
                                blocks: tron.blocks.map(alloy::primitives::Bytes::from),
                                encodedTx: tron.encoded_tx.into(),
                                proof: tron.proof,
                                index: tron.index,
                            };
                            let userop = ctx
                                .hub
                                .safe4337_build_call_userop(
                                    ctx.hub.pool_address(),
                                    call.abi_encode(),
                                )
                                .await
                                .context("build proveIntentFill userop")?;
                            let json =
                                serde_json::to_string(&userop).context("serialize prove userop")?;
                            ctx.db
                                .insert_hub_userop_prepared(
                                    job.job_id,
                                    &ctx.instance_id,
                                    kind,
                                    &json,
                                )
                                .await?;
                            match ctx.hub.safe4337_send_userop(userop).await {
                                Ok(userop_hash) => {
                                    ctx.db
                                        .record_hub_userop_submitted(
                                            job.job_id,
                                            &ctx.instance_id,
                                            kind,
                                            &userop_hash,
                                        )
                                        .await?;
                                }
                                Err(err) => {
                                    let msg = err.to_string();
                                    if msg.contains("AA25 invalid account nonce") {
                                        ctx.db
                                            .delete_hub_userop_prepared(
                                                job.job_id,
                                                &ctx.instance_id,
                                                kind,
                                            )
                                            .await
                                            .ok();
                                    }
                                    ctx.db
                                        .record_hub_userop_retryable_error(
                                            job.job_id,
                                            &ctx.instance_id,
                                            kind,
                                            &msg,
                                            retry_in(job.attempts),
                                        )
                                        .await
                                        .ok();
                                    ctx.db
                                        .record_retryable_error(
                                            job.job_id,
                                            &ctx.instance_id,
                                            &msg,
                                            retry_in(job.attempts),
                                        )
                                        .await?;
                                    return Ok(());
                                }
                            }
                        }
                        Some(r) => {
                            if r.userop_hash.is_none() && r.state == "prepared" {
                                let _permit = ctx
                                    .hub_userop_submit_sem
                                    .acquire()
                                    .await
                                    .context("acquire hub_userop_submit_sem (prove)")?;
                                let u: PackedUserOperation = serde_json::from_str(&r.userop_json)
                                    .context("deserialize prove userop")?;
                                match ctx.hub.safe4337_send_userop(u).await {
                                    Ok(userop_hash) => {
                                        ctx.db
                                            .record_hub_userop_submitted(
                                                job.job_id,
                                                &ctx.instance_id,
                                                kind,
                                                &userop_hash,
                                            )
                                            .await?;
                                    }
                                    Err(err) => {
                                        let msg = err.to_string();
                                        if msg.contains("AA25 invalid account nonce") {
                                            ctx.db
                                                .delete_hub_userop_prepared(
                                                    job.job_id,
                                                    &ctx.instance_id,
                                                    kind,
                                                )
                                                .await
                                                .ok();
                                        }
                                        ctx.db
                                            .record_hub_userop_retryable_error(
                                                job.job_id,
                                                &ctx.instance_id,
                                                kind,
                                                &msg,
                                                retry_in(job.attempts),
                                            )
                                            .await
                                            .ok();
                                        ctx.db
                                            .record_retryable_error(
                                                job.job_id,
                                                &ctx.instance_id,
                                                &msg,
                                                retry_in(job.attempts),
                                            )
                                            .await?;
                                        return Ok(());
                                    }
                                }
                            }
                        }
                    }

                    let row = ctx.db.get_hub_userop(job.job_id, kind).await?;
                    let Some(r) = row else {
                        return Ok(());
                    };
                    let Some(userop_hash) = r.userop_hash.clone() else {
                        return Ok(());
                    };

                    match ctx.hub.safe4337_get_userop_receipt(&userop_hash).await {
                        Ok(Some(receipt)) => {
                            let Some(tx_hash) = receipt.tx_hash else {
                                return Ok(());
                            };
                            let success = receipt.success.unwrap_or(false);
                            let receipt_json = serde_json::to_string(&receipt.raw)
                                .unwrap_or_else(|_| "{}".to_string());
                            ctx.db
                                .record_hub_userop_included(
                                    job.job_id,
                                    &ctx.instance_id,
                                    kind,
                                    b256_to_bytes32(tx_hash),
                                    receipt.block_number.map(|n| n as i64),
                                    success,
                                    receipt.actual_gas_cost_wei,
                                    receipt.actual_gas_used,
                                    &receipt_json,
                                )
                                .await?;
                            if success {
                                ctx.db
                                    .record_prove(
                                        job.job_id,
                                        &ctx.instance_id,
                                        b256_to_bytes32(tx_hash),
                                    )
                                    .await?;
                            } else {
                                let msg = format!(
                                    "prove userop failed: {:?}",
                                    receipt.reason.unwrap_or(serde_json::Value::Null)
                                );
                                ctx.db
                                    .record_hub_userop_fatal_error(
                                        job.job_id,
                                        &ctx.instance_id,
                                        kind,
                                        &msg,
                                    )
                                    .await
                                    .ok();
                                ctx.db
                                    .record_fatal_error(job.job_id, &ctx.instance_id, &msg)
                                    .await?;
                            }
                            Ok(())
                        }
                        Ok(None) => Ok(()),
                        Err(err) => {
                            let msg = err.to_string();
                            ctx.db
                                .record_hub_userop_retryable_error(
                                    job.job_id,
                                    &ctx.instance_id,
                                    kind,
                                    &msg,
                                    retry_in(job.attempts),
                                )
                                .await
                                .ok();
                            ctx.db
                                .record_retryable_error(
                                    job.job_id,
                                    &ctx.instance_id,
                                    &msg,
                                    retry_in(job.attempts),
                                )
                                .await?;
                            Ok(())
                        }
                    }
                }
            }
        }
        "done" | "failed_fatal" => Ok(()),
        other => {
            ctx.db
                .record_fatal_error(
                    job.job_id,
                    &ctx.instance_id,
                    &format!("unknown job state: {other}"),
                )
                .await?;
            Ok(())
        }
    }
}

fn b256_to_bytes32(v: B256) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(v.as_slice());
    out
}

fn looks_like_tron_server_busy(msg: &str) -> bool {
    msg.contains("SERVER_BUSY")
}

fn looks_like_tron_contract_failure(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("revert")
        || m.contains("contract_validate_error")
        || m.contains("contract validate error")
        || m.contains("out_of_energy")
        || m.contains("out of energy")
        || m.contains("validate")
}

fn decode_trigger_contract_and_selector(
    intent_specs: &[u8],
) -> Option<(alloy::primitives::Address, Option<[u8; 4]>)> {
    use alloy::sol_types::SolValue;

    let intent = crate::tron_backend::TriggerSmartContractIntent::abi_decode(intent_specs).ok()?;
    let data = intent.data.as_ref();
    let selector = if data.len() >= 4 {
        let mut out = [0u8; 4];
        out.copy_from_slice(&data[..4]);
        Some(out)
    } else {
        None
    };
    Some((intent.to, selector))
}
