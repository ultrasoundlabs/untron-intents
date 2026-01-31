use crate::{
    config::{AppConfig, HubTxMode},
    db::SolverDb,
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

pub struct Solver {
    cfg: AppConfig,
    telemetry: SolverTelemetry,
    db: SolverDb,
    indexer: IndexerClient,
    hub: HubClient,
    tron: TronBackend,
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
            if let Err(err) = self.try_fill(row).await {
                tracing::warn!(err = %err, "fill failed");
            }
        }

        // Continue any previously-claimed/broadcasted runs without relying on PostgREST views.
        self.process_pending_proofs().await?;
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
        let ty = IntentType::from_i16(row.intent_type)?;
        Ok(matches!(
            ty,
            IntentType::TrxTransfer | IntentType::DelegateResource
        ))
    }

    async fn try_fill(&mut self, row: PoolOpenIntentRow) -> Result<()> {
        let id = parse_b256(&row.id)?;
        let intent_specs = parse_hex_bytes(&row.intent_specs)?;
        let ty = IntentType::from_i16(row.intent_type)?;

        tracing::info!(id = %row.id, intent_type = row.intent_type, "filling intent");

        self.db
            .upsert_run_state(b256_to_bytes32(id), "claiming", None, None, None, None)
            .await?;

        let claim_receipt = self.hub.claim_intent(id).await.context("claimIntent")?;
        self.db
            .upsert_run_state(
                b256_to_bytes32(id),
                "claimed",
                Some(b256_to_bytes32(claim_receipt.transaction_hash)),
                None,
                None,
                None,
            )
            .await?;

        let exec = match ty {
            IntentType::TrxTransfer => self
                .tron
                .execute_trx_transfer(&self.hub, id, &intent_specs)
                .await
                .context("execute trx transfer")?,
            IntentType::DelegateResource => self
                .tron
                .execute_delegate_resource(&self.hub, id, &intent_specs)
                .await
                .context("execute delegate resource")?,
            _ => unreachable!(),
        };

        match exec {
            TronExecution::ImmediateProof(tron) => {
                let prove_receipt = self
                    .hub
                    .prove_intent_fill(id, tron)
                    .await
                    .context("proveIntentFill")?;
                self.db
                    .upsert_run_state(
                        b256_to_bytes32(id),
                        "proved",
                        None,
                        Some(b256_to_bytes32(prove_receipt.transaction_hash)),
                        None,
                        None,
                    )
                    .await?;
            }
            TronExecution::BroadcastedTx { txid } => {
                self.db
                    .upsert_run_state(
                        b256_to_bytes32(id),
                        "tron_sent",
                        None,
                        None,
                        Some(txid),
                        None,
                    )
                    .await?;
            }
        }

        tracing::info!(id = %row.id, "intent filled");
        Ok(())
    }

    async fn process_pending_proofs(&mut self) -> Result<()> {
        let pending = self
            .db
            .list_pending_proofs(i64::try_from(self.cfg.jobs.fill_max_claims).unwrap_or(50))
            .await?;

        for (intent_id, tron_txid) in pending {
            let id = B256::from_slice(&intent_id);
            tracing::info!(id = %id, "attempting proveIntentFill for pending run");

            let tron: TronProof = self.tron.build_proof(tron_txid).await?;
            let prove_receipt = self
                .hub
                .prove_intent_fill(id, tron)
                .await
                .context("proveIntentFill")?;
            self.db
                .upsert_run_state(
                    intent_id,
                    "proved",
                    None,
                    Some(b256_to_bytes32(prove_receipt.transaction_hash)),
                    None,
                    None,
                )
                .await?;
        }

        Ok(())
    }
}

fn b256_to_bytes32(v: B256) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(v.as_slice());
    out
}
