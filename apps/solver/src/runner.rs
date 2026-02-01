use crate::{
    config::{AppConfig, HubTxMode},
    db::SolverDb,
    db::{SolverJob, TronProofRow},
    indexer::{IndexerClient, PoolOpenIntentRow},
    metrics::SolverTelemetry,
    tron_backend::TronBackend,
    tron_backend::TronExecution,
    types::{IntentType, parse_b256, parse_hex_bytes},
};
use crate::{hub::HubClient, hub::TronProof};
use alloy::primitives::{B256, U256};
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

        let indexer = IndexerClient::new(
            cfg.indexer.base_url.clone(),
            cfg.indexer.timeout,
            telemetry.clone(),
        );

        let tron = TronBackend::new(cfg.tron.clone(), cfg.jobs.clone(), telemetry.clone());

        // Ensure we can claim by approving USDT once.
        let usdt = hub.pool_usdt().await?;
        hub.ensure_erc20_allowance(usdt, hub.pool_address(), U256::from(INTENT_CLAIM_DEPOSIT))
            .await?;

        Ok(Self {
            instance_id: cfg.instance_id.clone(),
            cfg,
            telemetry,
            db,
            indexer,
            hub,
            tron,
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
        let rows = self
            .indexer
            .fetch_open_intents(self.cfg.jobs.fill_max_claims)
            .await?;

        for row in rows {
            if !self.should_attempt(&row)? {
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
                i64::try_from(self.cfg.jobs.fill_max_claims).unwrap_or(50),
            )
            .await?;

        for job in jobs {
            if let Err(err) = self.process_job(job).await {
                tracing::warn!(err = %err, "job failed");
            }
        }
        Ok(())
    }

    fn should_attempt(&self, row: &PoolOpenIntentRow) -> Result<bool> {
        if row.closed || row.solved {
            return Ok(false);
        }
        if !row.funded {
            return Ok(false);
        }
        if row.solver.is_some() {
            return Ok(false);
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let slack = i64::try_from(self.cfg.policy.min_deadline_slack_secs).unwrap_or(i64::MAX);
        if row.deadline.saturating_sub(now) < slack {
            return Ok(false);
        }

        let ty = IntentType::from_i16(row.intent_type)?;
        Ok(self.cfg.policy.enabled_intent_types.contains(&ty))
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
                match self.hub.claim_intent(id).await {
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
                }
            }
            "claimed" => {
                tracing::info!(id = %id, "executing tron tx");
                let exec = match ty {
                    IntentType::TrxTransfer => self
                        .tron
                        .execute_trx_transfer(&self.hub, id, &job.intent_specs)
                        .await
                        .context("execute trx transfer")?,
                    IntentType::UsdtTransfer => self
                        .tron
                        .execute_usdt_transfer(&self.hub, id, &job.intent_specs)
                        .await
                        .context("execute usdt transfer")?,
                    IntentType::DelegateResource => self
                        .tron
                        .execute_delegate_resource(&self.hub, id, &job.intent_specs)
                        .await
                        .context("execute delegate resource")?,
                    _ => unreachable!(),
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
                    TronExecution::BroadcastedTx { txid } => {
                        self.db
                            .record_tron_txid(job.job_id, &self.instance_id, txid)
                            .await?;
                        Ok(())
                    }
                }
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
                let tron = self.tron.build_proof(txid).await?;
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
                let receipt = self
                    .hub
                    .prove_intent_fill(id, tron)
                    .await
                    .context("proveIntentFill")?;
                self.db
                    .record_prove(
                        job.job_id,
                        &self.instance_id,
                        b256_to_bytes32(receipt.transaction_hash),
                    )
                    .await?;
                Ok(())
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
