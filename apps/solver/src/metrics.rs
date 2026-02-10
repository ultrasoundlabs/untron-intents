use opentelemetry::{
    KeyValue, global,
    metrics::{Counter, Histogram},
};
use std::sync::Arc;

#[derive(Clone)]
pub struct SolverTelemetry {
    inner: Arc<Inner>,
}

struct Inner {
    jobs_total: Counter<u64>,
    job_errors_total: Counter<u64>,
    job_failures_by_reason_total: Counter<u64>,
    hub_userops_total: Counter<u64>,
    hub_userop_errors_total: Counter<u64>,
    tron_txs_total: Counter<u64>,
    tron_tx_errors_total: Counter<u64>,
    claim_rate_limited_total: Counter<u64>,
    global_paused_total: Counter<u64>,
    emulation_mismatch_total: Counter<u64>,
    delegate_reservation_conflicts_total: Counter<u64>,
    job_state_transitions_total: Counter<u64>,
    rental_quotes_total: Counter<u64>,
    rental_orders_total: Counter<u64>,
    rental_provider_freezes_total: Counter<u64>,
    candidate_skips_total: Counter<u64>,

    job_ms: Histogram<u64>,
    hub_submit_ms: Histogram<u64>,
    tron_broadcast_ms: Histogram<u64>,
    indexer_http_ms: Histogram<u64>,
    hub_rpc_ms: Histogram<u64>,
    tron_proof_ms: Histogram<u64>,
    tron_grpc_ms: Histogram<u64>,
    rental_quote_ms: Histogram<u64>,
    rental_order_ms: Histogram<u64>,
}

impl SolverTelemetry {
    pub fn new() -> Self {
        let meter = global::meter("solver");

        let jobs_total = meter
            .u64_counter("solver.jobs_total")
            .with_description("Total job runs")
            .build();
        let job_errors_total = meter
            .u64_counter("solver.job_errors_total")
            .with_description("Total job errors")
            .build();
        let job_failures_by_reason_total = meter
            .u64_counter("solver.job_failures_by_reason_total")
            .with_description("Total job failures partitioned by reason")
            .build();
        let hub_userops_total = meter
            .u64_counter("solver.hub_userops_total")
            .with_description("Total hub user operations sent")
            .build();
        let hub_userop_errors_total = meter
            .u64_counter("solver.hub_userop_errors_total")
            .with_description("Total hub user operation submission errors")
            .build();
        let tron_txs_total = meter
            .u64_counter("solver.tron_txs_total")
            .with_description("Total Tron transactions broadcast")
            .build();
        let tron_tx_errors_total = meter
            .u64_counter("solver.tron_tx_errors_total")
            .with_description("Total Tron transaction errors")
            .build();
        let claim_rate_limited_total = meter
            .u64_counter("solver.claim_rate_limited_total")
            .with_description("Total claim rate-limited events")
            .build();
        let global_paused_total = meter
            .u64_counter("solver.global_paused_total")
            .with_description("Total times global pause blocked claiming")
            .build();
        let emulation_mismatch_total = meter
            .u64_counter("solver.emulation_mismatch_total")
            .with_description("Total onchain failures after emulation-ok")
            .build();
        let delegate_reservation_conflicts_total = meter
            .u64_counter("solver.delegate_reservation_conflicts_total")
            .with_description("Total delegate reservation conflicts/insufficient capacity")
            .build();
        let job_state_transitions_total = meter
            .u64_counter("solver.job_state_transitions_total")
            .with_description("Total job state transitions (best-effort)")
            .build();

        let rental_quotes_total = meter
            .u64_counter("solver.rental_quotes_total")
            .with_description("Total rental quote attempts")
            .build();

        let rental_orders_total = meter
            .u64_counter("solver.rental_orders_total")
            .with_description("Total rental order attempts")
            .build();

        let rental_provider_freezes_total = meter
            .u64_counter("solver.rental_provider_freezes_total")
            .with_description("Total rental provider freeze events")
            .build();
        let candidate_skips_total = meter
            .u64_counter("solver.candidate_skips_total")
            .with_description("Total candidate intents skipped before job creation")
            .build();

        let job_ms = meter
            .u64_histogram("solver.job_ms")
            .with_description("Per-job runtime")
            .with_unit("ms")
            .build();

        let hub_submit_ms = meter
            .u64_histogram("solver.hub_submit_ms")
            .with_description("Hub userop submission runtime")
            .with_unit("ms")
            .build();

        let tron_broadcast_ms = meter
            .u64_histogram("solver.tron_broadcast_ms")
            .with_description("Tron transaction broadcast runtime")
            .with_unit("ms")
            .build();

        let indexer_http_ms = meter
            .u64_histogram("solver.indexer_http_ms")
            .with_description("Indexer (PostgREST) HTTP request runtime")
            .with_unit("ms")
            .build();

        let hub_rpc_ms = meter
            .u64_histogram("solver.hub_rpc_ms")
            .with_description("Hub chain JSON-RPC call runtime")
            .with_unit("ms")
            .build();

        let tron_proof_ms = meter
            .u64_histogram("solver.tron_proof_ms")
            .with_description("Tron proof build runtime")
            .with_unit("ms")
            .build();

        let tron_grpc_ms = meter
            .u64_histogram("solver.tron_grpc_ms")
            .with_description("Tron gRPC call runtime")
            .with_unit("ms")
            .build();

        let rental_quote_ms = meter
            .u64_histogram("solver.rental_quote_ms")
            .with_description("Rental quote HTTP runtime")
            .with_unit("ms")
            .build();

        let rental_order_ms = meter
            .u64_histogram("solver.rental_order_ms")
            .with_description("Rental order HTTP runtime")
            .with_unit("ms")
            .build();

        Self {
            inner: Arc::new(Inner {
                jobs_total,
                job_errors_total,
                job_failures_by_reason_total,
                hub_userops_total,
                hub_userop_errors_total,
                tron_txs_total,
                tron_tx_errors_total,
                claim_rate_limited_total,
                global_paused_total,
                emulation_mismatch_total,
                delegate_reservation_conflicts_total,
                job_state_transitions_total,
                rental_quotes_total,
                rental_orders_total,
                rental_provider_freezes_total,
                candidate_skips_total,
                job_ms,
                hub_submit_ms,
                tron_broadcast_ms,
                indexer_http_ms,
                hub_rpc_ms,
                tron_proof_ms,
                tron_grpc_ms,
                rental_quote_ms,
                rental_order_ms,
            }),
        }
    }

    pub fn job_ok(&self, name: &'static str, ms: u64) {
        // Avoid "job" label name collisions with Prometheus' conventional "job" label.
        let attrs = [KeyValue::new("job_name", name)];
        self.inner.jobs_total.add(1, &attrs);
        self.inner.job_ms.record(ms, &attrs);
    }

    pub fn job_err(&self, name: &'static str, ms: u64) {
        // Avoid "job" label name collisions with Prometheus' conventional "job" label.
        let attrs = [KeyValue::new("job_name", name)];
        self.inner.jobs_total.add(1, &attrs);
        self.inner.job_errors_total.add(1, &attrs);
        self.inner.job_ms.record(ms, &attrs);
    }

    pub fn job_failure_reason(&self, intent_type: i16, reason: &'static str) {
        let attrs = [
            KeyValue::new("intent_type", intent_type as i64),
            KeyValue::new("reason", reason),
        ];
        self.inner.job_failures_by_reason_total.add(1, &attrs);
    }

    pub fn hub_userop_ok(&self) {
        self.inner.hub_userops_total.add(1, &[]);
    }

    pub fn hub_userop_err(&self) {
        self.inner.hub_userop_errors_total.add(1, &[]);
    }

    pub fn tron_tx_ok(&self) {
        self.inner.tron_txs_total.add(1, &[]);
    }

    pub fn tron_tx_err(&self) {
        self.inner.tron_tx_errors_total.add(1, &[]);
    }

    pub fn claim_rate_limited(&self, key: &'static str) {
        let attrs = [KeyValue::new("key", key)];
        self.inner.claim_rate_limited_total.add(1, &attrs);
    }

    pub fn global_paused(&self) {
        self.inner.global_paused_total.add(1, &[]);
    }

    pub fn emulation_mismatch(&self) {
        self.inner.emulation_mismatch_total.add(1, &[]);
    }

    pub fn delegate_reservation_conflict(&self) {
        self.inner.delegate_reservation_conflicts_total.add(1, &[]);
    }

    pub fn job_state_transition(&self, intent_type: i16, from: &'static str, to: &'static str) {
        let attrs = [
            KeyValue::new("intent_type", intent_type as i64),
            KeyValue::new("from", from),
            KeyValue::new("to", to),
        ];
        self.inner.job_state_transitions_total.add(1, &attrs);
    }

    pub fn rental_quote_ms(&self, provider: &str, ok: bool, ms: u64) {
        let attrs = [
            KeyValue::new("provider", provider.to_string()),
            KeyValue::new("status", if ok { "ok" } else { "err" }),
        ];
        self.inner.rental_quotes_total.add(1, &attrs);
        self.inner.rental_quote_ms.record(ms, &attrs);
    }

    pub fn rental_order_ms(&self, provider: &str, ok: bool, ms: u64) {
        let attrs = [
            KeyValue::new("provider", provider.to_string()),
            KeyValue::new("status", if ok { "ok" } else { "err" }),
        ];
        self.inner.rental_orders_total.add(1, &attrs);
        self.inner.rental_order_ms.record(ms, &attrs);
    }

    pub fn rental_provider_frozen(&self, provider: &str) {
        let attrs = [KeyValue::new("provider", provider.to_string())];
        self.inner.rental_provider_freezes_total.add(1, &attrs);
    }

    pub fn candidate_skip(&self, intent_type: i16, reason: &'static str) {
        let attrs = [
            KeyValue::new("intent_type", intent_type as i64),
            KeyValue::new("reason", reason),
        ];
        self.inner.candidate_skips_total.add(1, &attrs);
    }

    pub fn hub_submit_ms(&self, name: &'static str, ok: bool, ms: u64) {
        let attrs = [
            KeyValue::new("name", name),
            KeyValue::new("status", if ok { "ok" } else { "err" }),
        ];
        self.inner.hub_submit_ms.record(ms, &attrs);
    }

    pub fn tron_broadcast_ms(&self, ok: bool, ms: u64) {
        let attrs = [KeyValue::new("status", if ok { "ok" } else { "err" })];
        self.inner.tron_broadcast_ms.record(ms, &attrs);
    }

    pub fn indexer_http_ms(&self, op: &'static str, ok: bool, ms: u64) {
        let attrs = [
            KeyValue::new("op", op),
            KeyValue::new("status", if ok { "ok" } else { "err" }),
        ];
        self.inner.indexer_http_ms.record(ms, &attrs);
    }

    pub fn hub_rpc_ms(&self, op: &'static str, ok: bool, ms: u64) {
        let attrs = [
            KeyValue::new("op", op),
            KeyValue::new("status", if ok { "ok" } else { "err" }),
        ];
        self.inner.hub_rpc_ms.record(ms, &attrs);
    }

    pub fn tron_proof_ms(&self, ok: bool, ms: u64) {
        let attrs = [KeyValue::new("status", if ok { "ok" } else { "err" })];
        self.inner.tron_proof_ms.record(ms, &attrs);
    }

    pub fn tron_grpc_ms(&self, op: &'static str, ok: bool, ms: u64) {
        let attrs = [
            KeyValue::new("op", op),
            KeyValue::new("status", if ok { "ok" } else { "err" }),
        ];
        self.inner.tron_grpc_ms.record(ms, &attrs);
    }
}
