use crate::{
    config::{AppConfig, HubTxMode, TronMode},
    db::SolverDb,
    db::{HubUserOpKind, SolverJob, TronProofRow},
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
use std::time::Instant;
use tokio_util::sync::CancellationToken;

const INTENT_CLAIM_DEPOSIT: u64 = 1_000_000;
const LEASE_FOR_SECS: u64 = 30;

pub struct Solver {
    cfg: AppConfig,
    telemetry: SolverTelemetry,
    db: SolverDb,
    indexer: IndexerClient,
    hub: HubClient,
    tron: TronBackend,
    pricing: Pricing,
    policy: PolicyEngine,
    instance_id: String,
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
                i64::try_from(match self.cfg.hub.tx_mode {
                    // Safe4337 uses an onchain nonce. Until we implement a persistent "pending nonce"
                    // manager, we must not submit multiple userops in parallel (or we will hit AA25).
                    HubTxMode::Safe4337 => 1,
                    HubTxMode::Eoa => self.cfg.jobs.fill_max_claims,
                })
                .unwrap_or(50),
            )
            .await?;

        for job in jobs {
            if let Err(err) = self.process_job(job).await {
                tracing::warn!(err = %err, "job failed");
            }
        }
        Ok(())
    }

    async fn should_attempt(&mut self, row: &PoolOpenIntentRow) -> Result<bool> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let eval = self
            .policy
            .evaluate_open_intent(row, now, &mut self.pricing)
            .await?;
        if !eval.allowed {
            if let Some(reason) = eval.reason.as_deref() {
                tracing::debug!(id = %row.id, intent_type = row.intent_type, reason, "skip intent");
            }
            return Ok(false);
        }

        // Dynamic breaker (if applicable).
        if let Some(b) = eval.breaker {
            if self.is_breaker_active(b).await? {
                return Ok(false);
            }
        }

        Ok(true)
    }

    async fn is_breaker_active(&self, _b: BreakerQuery) -> Result<bool> {
        self.db.breaker_is_active(_b.contract, _b.selector).await
    }

    async fn process_job(&mut self, job: SolverJob) -> Result<()> {
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
                let usdt = match self.hub.pool_usdt().await {
                    Ok(v) => v,
                    Err(err) => {
                        let msg = format!("pool_usdt failed: {err:#}");
                        self.db
                            .record_retryable_error(
                                job.job_id,
                                &self.instance_id,
                                &msg,
                                retry_in(job.attempts),
                            )
                            .await?;
                        return Ok(());
                    }
                };
                if let Err(err) = self
                    .hub
                    .ensure_erc20_allowance(
                        usdt,
                        self.hub.pool_address(),
                        U256::from(INTENT_CLAIM_DEPOSIT),
                    )
                    .await
                {
                    let msg = format!("ensure_erc20_allowance failed: {err:#}");
                    self.db
                        .record_retryable_error(
                            job.job_id,
                            &self.instance_id,
                            &msg,
                            retry_in(job.attempts),
                        )
                        .await?;
                    return Ok(());
                }
                match self.cfg.hub.tx_mode {
                    HubTxMode::Eoa => match self.hub.claim_intent(id).await {
                        Ok(receipt) => {
                            self.db
                                .record_claim(
                                    job.job_id,
                                    &self.instance_id,
                                    b256_to_bytes32(receipt.transaction_hash),
                                )
                                .await?;
                            Ok(())
                        }
                        Err(err) => {
                            let msg = err.to_string();
                            // If already claimed, don't keep retrying.
                            if msg.contains("AlreadyClaimed") {
                                self.db
                                    .record_fatal_error(job.job_id, &self.instance_id, &msg)
                                    .await?;
                                return Ok(());
                            }
                            self.db
                                .record_retryable_error(
                                    job.job_id,
                                    &self.instance_id,
                                    &msg,
                                    retry_in(job.attempts),
                                )
                                .await?;
                            Ok(())
                        }
                    },
                    HubTxMode::Safe4337 => {
                        let kind = HubUserOpKind::Claim;
                        let mut row = self.db.get_hub_userop(job.job_id, kind).await?;
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
                                let chain_nonce = self.hub.safe4337_chain_nonce().await?;
                                if u.nonce < chain_nonce {
                                    self.db
                                        .delete_hub_userop_prepared(
                                            job.job_id,
                                            &self.instance_id,
                                            kind,
                                        )
                                        .await
                                        .ok();
                                    row = None;
                                }
                            }
                        }

                        let userop = match row {
                            None => {
                                let call = crate::hub::IUntronIntents::claimIntentCall { id };
                                let userop = self
                                    .hub
                                    .safe4337_build_call_userop(
                                        self.hub.pool_address(),
                                        call.abi_encode(),
                                    )
                                    .await
                                    .context("build claimIntent userop")?;
                                let json = serde_json::to_string(&userop)
                                    .context("serialize claim userop")?;
                                self.db
                                    .insert_hub_userop_prepared(
                                        job.job_id,
                                        &self.instance_id,
                                        kind,
                                        &json,
                                    )
                                    .await?;
                                Some(userop)
                            }
                            Some(r) => {
                                if r.userop_hash.is_none() && r.state == "prepared" {
                                    let u: PackedUserOperation =
                                        serde_json::from_str(&r.userop_json)
                                            .context("deserialize claim userop")?;
                                    Some(u)
                                } else {
                                    None
                                }
                            }
                        };

                        if let Some(userop) = userop {
                            match self.hub.safe4337_send_userop(userop).await {
                                Ok(userop_hash) => {
                                    self.db
                                        .record_hub_userop_submitted(
                                            job.job_id,
                                            &self.instance_id,
                                            kind,
                                            &userop_hash,
                                        )
                                        .await?;
                                }
                                Err(err) => {
                                    let msg = err.to_string();
                                    if msg.contains("AA25 invalid account nonce") {
                                        self.db
                                            .delete_hub_userop_prepared(
                                                job.job_id,
                                                &self.instance_id,
                                                kind,
                                            )
                                            .await
                                            .ok();
                                    }
                                    self.db
                                        .record_hub_userop_retryable_error(
                                            job.job_id,
                                            &self.instance_id,
                                            kind,
                                            &msg,
                                            retry_in(job.attempts),
                                        )
                                        .await
                                        .ok();
                                    self.db
                                        .record_retryable_error(
                                            job.job_id,
                                            &self.instance_id,
                                            &msg,
                                            retry_in(job.attempts),
                                        )
                                        .await?;
                                    return Ok(());
                                }
                            }
                        }

                        // Poll receipt if we have a userop hash.
                        let row = self.db.get_hub_userop(job.job_id, kind).await?;
                        let Some(r) = row else {
                            return Ok(());
                        };
                        let Some(userop_hash) = r.userop_hash.clone() else {
                            return Ok(());
                        };

                        match self.hub.safe4337_get_userop_receipt(&userop_hash).await {
                            Ok(Some(receipt)) => {
                                let Some(tx_hash) = receipt.tx_hash else {
                                    return Ok(());
                                };
                                let success = receipt.success.unwrap_or(false);
                                self.db
                                    .record_hub_userop_included(
                                        job.job_id,
                                        &self.instance_id,
                                        kind,
                                        b256_to_bytes32(tx_hash),
                                        success,
                                    )
                                    .await?;
                                if success {
                                    self.db
                                        .record_claim(
                                            job.job_id,
                                            &self.instance_id,
                                            b256_to_bytes32(tx_hash),
                                        )
                                        .await?;
                                } else {
                                    let msg = format!(
                                        "claim userop failed: {:?}",
                                        receipt.reason.unwrap_or(serde_json::Value::Null)
                                    );
                                    self.db
                                        .record_hub_userop_fatal_error(
                                            job.job_id,
                                            &self.instance_id,
                                            kind,
                                            &msg,
                                        )
                                        .await
                                        .ok();
                                    self.db
                                        .record_fatal_error(job.job_id, &self.instance_id, &msg)
                                        .await?;
                                }
                                Ok(())
                            }
                            Ok(None) => Ok(()),
                            Err(err) => {
                                let msg = err.to_string();
                                self.db
                                    .record_hub_userop_retryable_error(
                                        job.job_id,
                                        &self.instance_id,
                                        kind,
                                        &msg,
                                        retry_in(job.attempts),
                                    )
                                    .await
                                    .ok();
                                self.db
                                    .record_retryable_error(
                                        job.job_id,
                                        &self.instance_id,
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
                if self.cfg.tron.mode == TronMode::Mock {
                    tracing::info!(id = %id, "executing tron tx (mock)");
                    let exec_res = match ty {
                        IntentType::TriggerSmartContract => self
                            .tron
                            .prepare_trigger_smart_contract(&self.hub, id, &job.intent_specs)
                            .await
                            .context("execute trigger smart contract"),
                        IntentType::TrxTransfer => self
                            .tron
                            .prepare_trx_transfer(&self.hub, id, &job.intent_specs)
                            .await
                            .context("execute trx transfer"),
                        IntentType::UsdtTransfer => self
                            .tron
                            .prepare_usdt_transfer(&self.hub, id, &job.intent_specs)
                            .await
                            .context("execute usdt transfer"),
                        IntentType::DelegateResource => self
                            .tron
                            .prepare_delegate_resource(&self.hub, id, &job.intent_specs)
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
                                    let _ = self
                                        .db
                                        .breaker_record_failure(contract, selector, &msg)
                                        .await;
                                }
                            }

                            self.db
                                .record_retryable_error(
                                    job.job_id,
                                    &self.instance_id,
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
                            self.db.save_tron_proof(txid, &proof_row).await?;
                            self.db
                                .record_tron_txid(job.job_id, &self.instance_id, txid)
                                .await?;
                            self.db
                                .record_proof_built(job.job_id, &self.instance_id)
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
                        IntentType::TriggerSmartContract => self
                            .tron
                            .prepare_trigger_smart_contract(&self.hub, id, &job.intent_specs)
                            .await
                            .context("prepare trigger smart contract"),
                        IntentType::TrxTransfer => self
                            .tron
                            .prepare_trx_transfer(&self.hub, id, &job.intent_specs)
                            .await
                            .context("prepare trx transfer"),
                        IntentType::UsdtTransfer => self
                            .tron
                            .prepare_usdt_transfer(&self.hub, id, &job.intent_specs)
                            .await
                            .context("prepare usdt transfer"),
                        IntentType::DelegateResource => self
                            .tron
                            .prepare_delegate_resource(&self.hub, id, &job.intent_specs)
                            .await
                            .context("prepare delegate resource"),
                    };

                    let exec = match exec_res {
                        Ok(v) => v,
                        Err(err) => {
                            let msg = err.to_string();
                            self.db
                                .record_retryable_error(
                                    job.job_id,
                                    &self.instance_id,
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
                            self.db
                                .record_tron_prepared(
                                    job.job_id,
                                    &self.instance_id,
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
                    self.db
                        .record_retryable_error(
                            job.job_id,
                            &self.instance_id,
                            "missing tron_txid",
                            retry_in(job.attempts),
                        )
                        .await?;
                    return Ok(());
                };

                // If we already see the tx onchain, just move forward. This avoids double-broadcasts
                // across crashes between broadcast and state update.
                if self.tron.tx_is_known(txid).await {
                    self.db
                        .record_tron_txid(job.job_id, &self.instance_id, txid)
                        .await?;
                    return Ok(());
                }

                let tx_bytes = self.db.load_tron_signed_tx_bytes(txid).await?;
                if let Err(err) = self.tron.broadcast_signed_tx(&tx_bytes).await {
                    let msg = err.to_string();
                    self.db
                        .record_retryable_error(
                            job.job_id,
                            &self.instance_id,
                            &msg,
                            retry_in(job.attempts),
                        )
                        .await?;
                    return Ok(());
                }

                self.db
                    .record_tron_txid(job.job_id, &self.instance_id, txid)
                    .await?;
                Ok(())
            }
            "tron_sent" => {
                let Some(txid) = job.tron_txid else {
                    self.db
                        .record_retryable_error(
                            job.job_id,
                            &self.instance_id,
                            "missing tron_txid",
                            retry_in(job.attempts),
                        )
                        .await?;
                    return Ok(());
                };
                tracing::info!(id = %id, "building tron proof");
                let tron = match self.tron.build_proof(txid).await {
                    Ok(v) => v,
                    Err(err) => {
                        let msg = err.to_string();
                        self.db
                            .record_retryable_error(
                                job.job_id,
                                &self.instance_id,
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
                self.db.save_tron_proof(txid, &proof_row).await?;
                self.db
                    .record_proof_built(job.job_id, &self.instance_id)
                    .await?;
                Ok(())
            }
            "proof_built" => {
                let Some(txid) = job.tron_txid else {
                    self.db
                        .record_retryable_error(
                            job.job_id,
                            &self.instance_id,
                            "missing tron_txid",
                            retry_in(job.attempts),
                        )
                        .await?;
                    return Ok(());
                };
                let proof = self.db.load_tron_proof(txid).await?;
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
                match self.cfg.hub.tx_mode {
                    HubTxMode::Eoa => match self.hub.prove_intent_fill(id, tron).await {
                        Ok(receipt) => {
                            self.db
                                .record_prove(
                                    job.job_id,
                                    &self.instance_id,
                                    b256_to_bytes32(receipt.transaction_hash),
                                )
                                .await?;
                            Ok(())
                        }
                        Err(err) => {
                            let msg = err.to_string();
                            self.db
                                .record_retryable_error(
                                    job.job_id,
                                    &self.instance_id,
                                    &msg,
                                    retry_in(job.attempts),
                                )
                                .await?;
                            Ok(())
                        }
                    },
                    HubTxMode::Safe4337 => {
                        let kind = HubUserOpKind::Prove;
                        let mut row = self.db.get_hub_userop(job.job_id, kind).await?;
                        if let Some(r) = row.as_ref() {
                            if r.state == "included" {
                                return Ok(());
                            }
                            if r.userop_hash.is_none() && r.state == "prepared" {
                                let u: PackedUserOperation = serde_json::from_str(&r.userop_json)
                                    .context("deserialize prove userop")?;
                                let chain_nonce = self.hub.safe4337_chain_nonce().await?;
                                if u.nonce < chain_nonce {
                                    self.db
                                        .delete_hub_userop_prepared(
                                            job.job_id,
                                            &self.instance_id,
                                            kind,
                                        )
                                        .await
                                        .ok();
                                    row = None;
                                }
                            }
                        }

                        let userop = match row {
                            None => {
                                let call = crate::hub::IUntronIntents::proveIntentFillCall {
                                    id,
                                    blocks: tron.blocks.map(alloy::primitives::Bytes::from),
                                    encodedTx: tron.encoded_tx.into(),
                                    proof: tron.proof,
                                    index: tron.index,
                                };
                                let userop = self
                                    .hub
                                    .safe4337_build_call_userop(
                                        self.hub.pool_address(),
                                        call.abi_encode(),
                                    )
                                    .await
                                    .context("build proveIntentFill userop")?;
                                let json = serde_json::to_string(&userop)
                                    .context("serialize prove userop")?;
                                self.db
                                    .insert_hub_userop_prepared(
                                        job.job_id,
                                        &self.instance_id,
                                        kind,
                                        &json,
                                    )
                                    .await?;
                                Some(userop)
                            }
                            Some(r) => {
                                if r.userop_hash.is_none() && r.state == "prepared" {
                                    let u: PackedUserOperation =
                                        serde_json::from_str(&r.userop_json)
                                            .context("deserialize prove userop")?;
                                    Some(u)
                                } else {
                                    None
                                }
                            }
                        };

                        if let Some(userop) = userop {
                            match self.hub.safe4337_send_userop(userop).await {
                                Ok(userop_hash) => {
                                    self.db
                                        .record_hub_userop_submitted(
                                            job.job_id,
                                            &self.instance_id,
                                            kind,
                                            &userop_hash,
                                        )
                                        .await?;
                                }
                                Err(err) => {
                                    let msg = err.to_string();
                                    if msg.contains("AA25 invalid account nonce") {
                                        self.db
                                            .delete_hub_userop_prepared(
                                                job.job_id,
                                                &self.instance_id,
                                                kind,
                                            )
                                            .await
                                            .ok();
                                    }
                                    self.db
                                        .record_hub_userop_retryable_error(
                                            job.job_id,
                                            &self.instance_id,
                                            kind,
                                            &msg,
                                            retry_in(job.attempts),
                                        )
                                        .await
                                        .ok();
                                    self.db
                                        .record_retryable_error(
                                            job.job_id,
                                            &self.instance_id,
                                            &msg,
                                            retry_in(job.attempts),
                                        )
                                        .await?;
                                    return Ok(());
                                }
                            }
                        }

                        let row = self.db.get_hub_userop(job.job_id, kind).await?;
                        let Some(r) = row else {
                            return Ok(());
                        };
                        let Some(userop_hash) = r.userop_hash.clone() else {
                            return Ok(());
                        };

                        match self.hub.safe4337_get_userop_receipt(&userop_hash).await {
                            Ok(Some(receipt)) => {
                                let Some(tx_hash) = receipt.tx_hash else {
                                    return Ok(());
                                };
                                let success = receipt.success.unwrap_or(false);
                                self.db
                                    .record_hub_userop_included(
                                        job.job_id,
                                        &self.instance_id,
                                        kind,
                                        b256_to_bytes32(tx_hash),
                                        success,
                                    )
                                    .await?;
                                if success {
                                    self.db
                                        .record_prove(
                                            job.job_id,
                                            &self.instance_id,
                                            b256_to_bytes32(tx_hash),
                                        )
                                        .await?;
                                } else {
                                    let msg = format!(
                                        "prove userop failed: {:?}",
                                        receipt.reason.unwrap_or(serde_json::Value::Null)
                                    );
                                    self.db
                                        .record_hub_userop_fatal_error(
                                            job.job_id,
                                            &self.instance_id,
                                            kind,
                                            &msg,
                                        )
                                        .await
                                        .ok();
                                    self.db
                                        .record_fatal_error(job.job_id, &self.instance_id, &msg)
                                        .await?;
                                }
                                Ok(())
                            }
                            Ok(None) => Ok(()),
                            Err(err) => {
                                let msg = err.to_string();
                                self.db
                                    .record_hub_userop_retryable_error(
                                        job.job_id,
                                        &self.instance_id,
                                        kind,
                                        &msg,
                                        retry_in(job.attempts),
                                    )
                                    .await
                                    .ok();
                                self.db
                                    .record_retryable_error(
                                        job.job_id,
                                        &self.instance_id,
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
                self.db
                    .record_fatal_error(
                        job.job_id,
                        &self.instance_id,
                        &format!("unknown job state: {other}"),
                    )
                    .await?;
                Ok(())
            }
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
