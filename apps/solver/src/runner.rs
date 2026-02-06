use crate::hub::HubClient;
use crate::{
    config::{AppConfig, HubTxMode},
    db::SolverDb,
    db::{HubUserOpKind, SolverJob},
    hub_cost::estimate_hub_cost_usd_from_userops,
    indexer::IndexerClient,
    metrics::SolverTelemetry,
    policy::{BreakerQuery, PolicyEngine},
    pricing::Pricing,
    tron_backend::TronBackend,
    types::{parse_b256, parse_hex_bytes, IntentType, JobState},
};
use alloy::primitives::B256;
use alloy::sol_types::SolValue;
use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

mod candidate;
mod hub_flow;
mod retry;
mod tron_flow;

const INTENT_CLAIM_DEPOSIT: u64 = 1_000_000;
const LEASE_FOR_SECS: u64 = 30;

struct ShouldAttemptDecision {
    ok: bool,
    rental_quote: Option<RentalQuoteDecision>,
}

struct RentalQuoteDecision {
    provider: String,
    receiver_evm: [u8; 20],
    balance_sun: i64,
    lock_period: i64,
    amount_units: u64,
    duration_hours: u64,
    cost_trx: f64,
    rendered_request: tron::RenderedJsonApiRequest,
    response_json: serde_json::Value,
}

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
    tron_broadcast_sem: Arc<Semaphore>,
    job_type_sems: Arc<JobTypeSems>,
}

#[derive(Clone)]
struct JobCtx {
    cfg: AppConfig,
    db: SolverDb,
    indexer: IndexerClient,
    hub: Arc<HubClient>,
    tron: TronBackend,
    instance_id: String,
    hub_userop_submit_sem: Arc<Semaphore>,
    tron_broadcast_sem: Arc<Semaphore>,
    job_type_sems: Arc<JobTypeSems>,
    telemetry: SolverTelemetry,
}

struct JobTypeSems {
    trx_transfer: Arc<Semaphore>,
    usdt_transfer: Arc<Semaphore>,
    delegate_resource: Arc<Semaphore>,
    trigger_smart_contract: Arc<Semaphore>,
}

impl JobTypeSems {
    fn for_intent_type(&self, ty: IntentType) -> Arc<Semaphore> {
        match ty {
            IntentType::TrxTransfer => Arc::clone(&self.trx_transfer),
            IntentType::UsdtTransfer => Arc::clone(&self.usdt_transfer),
            IntentType::DelegateResource => Arc::clone(&self.delegate_resource),
            IntentType::TriggerSmartContract => Arc::clone(&self.trigger_smart_contract),
        }
    }
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

        let job_type_sems = Arc::new(JobTypeSems {
            trx_transfer: Arc::new(Semaphore::new(
                usize::try_from(cfg.jobs.concurrency_trx_transfer).unwrap_or(1),
            )),
            usdt_transfer: Arc::new(Semaphore::new(
                usize::try_from(cfg.jobs.concurrency_usdt_transfer).unwrap_or(1),
            )),
            delegate_resource: Arc::new(Semaphore::new(
                usize::try_from(cfg.jobs.concurrency_delegate_resource).unwrap_or(1),
            )),
            trigger_smart_contract: Arc::new(Semaphore::new(
                usize::try_from(cfg.jobs.concurrency_trigger_smart_contract).unwrap_or(1),
            )),
        });
        let tron_broadcast_sem = Arc::new(Semaphore::new(
            usize::try_from(cfg.jobs.concurrency_tron_broadcast).unwrap_or(1),
        ));

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
            tron_broadcast_sem,
            job_type_sems,
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
        let _ = self.db.cleanup_expired_delegate_reservations().await;

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
            let decision = self.should_attempt(&row).await?;
            if !decision.ok {
                continue;
            }
            let id = parse_b256(&row.id)?;
            let specs = parse_hex_bytes(&row.intent_specs)?;
            let intent_id = b256_to_bytes32(id);
            self.db
                .insert_job_if_new(intent_id, row.intent_type, &specs, row.deadline)
                .await?;

            if let Some(q) = decision.rental_quote {
                if let Some(job_id) = self.db.job_id_for_intent(intent_id).await? {
                    let request_json = serde_json::json!({
                        "quote": q.rendered_request,
                        "quote_meta": {
                            "duration_hours": q.duration_hours,
                            "amount_units": q.amount_units,
                            "cost_trx": q.cost_trx,
                        }
                    });
                    let response_json = serde_json::json!({
                        "quote": q.response_json
                    });
                    let _ = self
                        .db
                        .upsert_tron_rental(
                            job_id,
                            &q.provider,
                            "energy",
                            q.receiver_evm,
                            q.balance_sun,
                            q.lock_period,
                            None,
                            None,
                            Some(&request_json),
                            Some(&response_json),
                        )
                        .await;
                }
            }
        }

        let jobs = self
            .db
            .lease_jobs(
                &self.instance_id,
                std::time::Duration::from_secs(LEASE_FOR_SECS),
                i64::try_from(self.cfg.jobs.max_in_flight_jobs)
                    .unwrap_or(50)
                    .max(1),
            )
            .await?;

        let ctx = JobCtx {
            cfg: self.cfg.clone(),
            db: self.db.clone(),
            indexer: self.indexer.clone(),
            hub: self.hub.clone(),
            tron: self.tron.clone(),
            instance_id: self.instance_id.clone(),
            hub_userop_submit_sem: self.hub_userop_submit_sem.clone(),
            tron_broadcast_sem: self.tron_broadcast_sem.clone(),
            job_type_sems: self.job_type_sems.clone(),
            telemetry: self.telemetry.clone(),
        };

        let mut set = JoinSet::new();
        for job in jobs {
            let ctx = ctx.clone();
            set.spawn(async move {
                let ty = match IntentType::from_i16(job.intent_type) {
                    Ok(v) => v,
                    Err(err) => {
                        tracing::warn!(err = %err, "unknown intent type in job");
                        return;
                    }
                };
                let _permit = match ctx.job_type_sems.for_intent_type(ty).acquire_owned().await {
                    Ok(p) => p,
                    Err(err) => {
                        tracing::warn!(err = %err, "failed to acquire job type permit");
                        return;
                    }
                };
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

async fn ensure_delegate_reservation(ctx: &JobCtx, job: &SolverJob) -> Result<[u8; 32]> {
    let intent = crate::tron_backend::DelegateResourceIntent::abi_decode(&job.intent_specs)
        .context("decode DelegateResourceIntent")?;
    let needed = i64::try_from(intent.balanceSun).unwrap_or(i64::MAX);
    let resource_i16 = i16::from(intent.resource);
    let rc = match intent.resource {
        0 => tron::protocol::ResourceCode::Bandwidth,
        1 => tron::protocol::ResourceCode::Energy,
        2 => tron::protocol::ResourceCode::TronPower,
        other => anyhow::bail!("unsupported DelegateResourceIntent.resource: {other}"),
    };

    // If already reserved, just refresh TTL and return the chosen key.
    if let Some(existing) = ctx.db.get_delegate_reservation_for_job(job.job_id).await? {
        ctx.db
            .upsert_delegate_reservation_for_job(
                job.job_id,
                &existing.owner_address,
                existing.resource,
                existing.amount_sun,
                i64::try_from(ctx.cfg.jobs.delegate_reservation_ttl_secs).unwrap_or(600),
            )
            .await?;
        return ctx
            .tron
            .private_key_for_owner(&existing.owner_address)
            .context("delegate reservation owner not in configured keys");
    }

    let by_key = ctx.tron.delegate_available_sun_by_key(rc).await?;
    let reserved = ctx
        .db
        .sum_delegate_reserved_sun_by_owner(resource_i16)
        .await?;
    let mut reserved_map = std::collections::HashMap::<Vec<u8>, i64>::new();
    for (owner, amt) in reserved {
        reserved_map.insert(owner, amt);
    }

    let mut owners = Vec::with_capacity(by_key.len());
    let mut avail = Vec::with_capacity(by_key.len());
    let mut resv = Vec::with_capacity(by_key.len());
    for (addr, a) in &by_key {
        let owner = addr.prefixed_bytes().to_vec();
        let r = *reserved_map.get(&owner).unwrap_or(&0);
        owners.push(owner);
        avail.push(*a);
        resv.push(r);
    }

    let Some(idx) = crate::tron_backend::select_delegate_executor_index(&avail, &resv, needed)
    else {
        ctx.telemetry.delegate_reservation_conflict();
        anyhow::bail!("delegate_capacity_insufficient");
    };

    let owner = owners[idx].clone();
    ctx.db
        .upsert_delegate_reservation_for_job(
            job.job_id,
            &owner,
            resource_i16,
            needed,
            i64::try_from(ctx.cfg.jobs.delegate_reservation_ttl_secs).unwrap_or(600),
        )
        .await?;
    ctx.tron
        .private_key_for_owner(&owner)
        .context("delegate reservation owner not in configured keys")
}

async fn finalize_after_prove(ctx: &JobCtx, job: &SolverJob) -> Result<()> {
    let id = B256::from_slice(&job.intent_id);
    // After proving, the hub state is the source of truth. Poll briefly so downstream tooling/tests
    // don't depend on “one extra solver tick”.
    for _ in 0..25 {
        match ctx.hub.intent_status(id).await {
            Ok(status) => {
                if status.closed || (status.solved && status.funded && status.settled) {
                    ctx.db.record_done(job.job_id, &ctx.instance_id).await?;
                    ctx.telemetry
                        .job_state_transition(job.intent_type, "proved", "done");
                    return Ok(());
                }
                if status.solved && !status.funded {
                    ctx.db
                        .record_job_state(job.job_id, &ctx.instance_id, "proved_waiting_funding")
                        .await?;
                    ctx.telemetry.job_state_transition(
                        job.intent_type,
                        "proved",
                        "proved_waiting_funding",
                    );
                    return Ok(());
                }
                if status.solved && status.funded && !status.settled {
                    ctx.db
                        .record_job_state(job.job_id, &ctx.instance_id, "proved_waiting_settlement")
                        .await?;
                    ctx.telemetry.job_state_transition(
                        job.intent_type,
                        "proved",
                        "proved_waiting_settlement",
                    );
                    return Ok(());
                }
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                continue;
            }
            Err(_) => {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                continue;
            }
        }
    }
    Ok(())
}

async fn process_job(ctx: JobCtx, job: SolverJob) -> Result<()> {
    let id = B256::from_slice(&job.intent_id);
    let ty = IntentType::from_i16(job.intent_type)?;
    let state = match JobState::parse(&job.state) {
        Ok(state) => state,
        Err(_) => {
            ctx.telemetry
                .job_state_transition(job.intent_type, "unknown", "failed_fatal");
            retry::record_fatal(&ctx, &job, &format!("unknown job state: {}", job.state)).await?;
            return Ok(());
        }
    };

    match state {
        JobState::Ready => hub_flow::process_ready_state(&ctx, &job, id, ty).await,
        JobState::Claimed => tron_flow::process_claimed_state(&ctx, &job, id, ty).await,
        JobState::TronPrepared => tron_flow::process_tron_prepared_state(&ctx, &job, ty).await,
        JobState::TronSent => tron_flow::process_tron_sent_state(&ctx, &job, id, ty).await,
        JobState::ProofBuilt => hub_flow::process_proof_built_state(&ctx, &job, id).await,
        JobState::Proved | JobState::ProvedWaitingFunding | JobState::ProvedWaitingSettlement => {
            hub_flow::process_proved_state(&ctx, &job, state).await
        }
        JobState::Done | JobState::FailedFatal => Ok(()),
    }
}
fn b256_to_bytes32(v: B256) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(v.as_slice());
    out
}

fn duration_hours_for_lock_period_blocks(lock_period_blocks: u64) -> u64 {
    let secs = lock_period_blocks.saturating_mul(3);
    let hours = secs.saturating_add(3599) / 3600;
    hours.max(1)
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
