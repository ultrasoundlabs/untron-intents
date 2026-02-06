use super::IntentType;
use crate::{
    config::AppConfig, db::SolverDb, hub::HubClient, indexer::IndexerClient,
    metrics::SolverTelemetry, tron_backend::TronBackend,
};
use std::sync::Arc;
use tokio::sync::Semaphore;

#[derive(Clone)]
pub(super) struct JobCtx {
    pub(super) cfg: AppConfig,
    pub(super) db: SolverDb,
    pub(super) indexer: IndexerClient,
    pub(super) hub: Arc<HubClient>,
    pub(super) tron: TronBackend,
    pub(super) instance_id: String,
    pub(super) hub_userop_submit_sem: Arc<Semaphore>,
    pub(super) tron_broadcast_sem: Arc<Semaphore>,
    pub(super) job_type_sems: Arc<JobTypeSems>,
    pub(super) telemetry: SolverTelemetry,
}

pub(super) struct JobTypeSems {
    pub(super) trx_transfer: Arc<Semaphore>,
    pub(super) usdt_transfer: Arc<Semaphore>,
    pub(super) delegate_resource: Arc<Semaphore>,
    pub(super) trigger_smart_contract: Arc<Semaphore>,
}

impl JobTypeSems {
    pub(super) fn for_intent_type(&self, ty: IntentType) -> Arc<Semaphore> {
        match ty {
            IntentType::TrxTransfer => Arc::clone(&self.trx_transfer),
            IntentType::UsdtTransfer => Arc::clone(&self.usdt_transfer),
            IntentType::DelegateResource => Arc::clone(&self.delegate_resource),
            IntentType::TriggerSmartContract => Arc::clone(&self.trigger_smart_contract),
        }
    }
}
