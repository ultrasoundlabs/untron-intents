use crate::config::Stream;
use opentelemetry::{
    KeyValue, global,
    metrics::{Counter, Histogram, ObservableGauge},
};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[derive(Clone)]
pub struct StreamTelemetry {
    inner: Arc<Inner>,
}

struct Inner {
    attrs: Vec<KeyValue>,

    // Counters
    ranges_total: Counter<u64>,
    blocks_total: Counter<u64>,
    logs_total: Counter<u64>,
    reorgs_total: Counter<u64>,
    rpc_errors_total: Counter<u64>,
    db_errors_total: Counter<u64>,
    rows_upserted_total: Counter<u64>,

    // Histograms
    rpc_latency_ms: Histogram<u64>,
    db_latency_ms: Histogram<u64>,
    range_total_ms: Histogram<u64>,
    timestamp_enrichment_ms: Histogram<u64>,

    // Gauges (observables)
    head_block: Arc<AtomicU64>,
    safe_head_block: Arc<AtomicU64>,
    next_block: Arc<AtomicU64>,
    backlog_blocks: Arc<AtomicU64>,
    chunk_blocks: Arc<AtomicU64>,
    _g_head_block: ObservableGauge<u64>,
    _g_safe_head_block: ObservableGauge<u64>,
    _g_next_block: ObservableGauge<u64>,
    _g_backlog_blocks: ObservableGauge<u64>,
    _g_chunk_blocks: ObservableGauge<u64>,
}

impl StreamTelemetry {
    pub fn new(stream: Stream, chain_id: u64) -> Self {
        let meter = global::meter("indexer");
        let attrs = vec![
            KeyValue::new("stream", stream.as_str()),
            KeyValue::new("chain_id", i64::try_from(chain_id).unwrap_or_default()),
        ];

        let ranges_total = meter
            .u64_counter("indexer.ranges_total")
            .with_description("Total processed eth_getLogs ranges")
            .build();
        let blocks_total = meter
            .u64_counter("indexer.blocks_total")
            .with_description("Total blocks covered by processed ranges")
            .build();
        let logs_total = meter
            .u64_counter("indexer.logs_total")
            .with_description("Total logs ingested")
            .build();
        let reorgs_total = meter
            .u64_counter("indexer.reorgs_total")
            .with_description("Total reorgs detected")
            .build();
        let rpc_errors_total = meter
            .u64_counter("indexer.rpc_errors_total")
            .with_description("Total RPC errors (best-effort classification via code paths)")
            .build();
        let db_errors_total = meter
            .u64_counter("indexer.db_errors_total")
            .with_description("Total DB errors (best-effort classification via code paths)")
            .build();
        let rows_upserted_total = meter
            .u64_counter("indexer.rows_upserted_total")
            .with_description("Total rows upserted into tables (best-effort)")
            .build();

        let rpc_latency_ms = meter
            .u64_histogram("indexer.rpc_latency_ms")
            .with_description("RPC call latency in milliseconds")
            .with_unit("ms")
            .build();
        let db_latency_ms = meter
            .u64_histogram("indexer.db_latency_ms")
            .with_description("DB operation latency in milliseconds")
            .with_unit("ms")
            .build();
        let range_total_ms = meter
            .u64_histogram("indexer.range_total_ms")
            .with_description("Total time to process a block range, milliseconds")
            .with_unit("ms")
            .build();
        let timestamp_enrichment_ms = meter
            .u64_histogram("indexer.timestamp_enrichment_ms")
            .with_description("Timestamp enrichment time, milliseconds")
            .with_unit("ms")
            .build();

        let head_block = Arc::new(AtomicU64::new(0));
        let safe_head_block = Arc::new(AtomicU64::new(0));
        let next_block = Arc::new(AtomicU64::new(0));
        let backlog_blocks = Arc::new(AtomicU64::new(0));
        let chunk_blocks = Arc::new(AtomicU64::new(0));

        let attrs_clone = attrs.clone();
        let head_block_clone = head_block.clone();
        let _g_head_block = meter
            .u64_observable_gauge("indexer.head_block")
            .with_description("Latest chain head block number")
            .with_callback(move |observer| {
                observer.observe(head_block_clone.load(Ordering::Relaxed), &attrs_clone);
            })
            .build();

        let attrs_clone = attrs.clone();
        let safe_head_block_clone = safe_head_block.clone();
        let _g_safe_head_block = meter
            .u64_observable_gauge("indexer.safe_head_block")
            .with_description("Latest safe head (head - confirmations)")
            .with_callback(move |observer| {
                observer.observe(safe_head_block_clone.load(Ordering::Relaxed), &attrs_clone);
            })
            .build();

        let attrs_clone = attrs.clone();
        let next_block_clone = next_block.clone();
        let _g_next_block = meter
            .u64_observable_gauge("indexer.next_block")
            .with_description("Next block number to index")
            .with_callback(move |observer| {
                observer.observe(next_block_clone.load(Ordering::Relaxed), &attrs_clone);
            })
            .build();

        let attrs_clone = attrs.clone();
        let backlog_blocks_clone = backlog_blocks.clone();
        let _g_backlog_blocks = meter
            .u64_observable_gauge("indexer.backlog_blocks")
            .with_description("Approx backlog size in blocks (safe_head - next_block)")
            .with_callback(move |observer| {
                observer.observe(backlog_blocks_clone.load(Ordering::Relaxed), &attrs_clone);
            })
            .build();

        let attrs_clone = attrs.clone();
        let chunk_blocks_clone = chunk_blocks.clone();
        let _g_chunk_blocks = meter
            .u64_observable_gauge("indexer.chunk_blocks")
            .with_description("Current block chunk size for eth_getLogs")
            .with_callback(move |observer| {
                observer.observe(chunk_blocks_clone.load(Ordering::Relaxed), &attrs_clone);
            })
            .build();

        Self {
            inner: Arc::new(Inner {
                attrs,
                ranges_total,
                blocks_total,
                logs_total,
                reorgs_total,
                rpc_errors_total,
                db_errors_total,
                rows_upserted_total,
                rpc_latency_ms,
                db_latency_ms,
                range_total_ms,
                timestamp_enrichment_ms,
                head_block,
                safe_head_block,
                next_block,
                backlog_blocks,
                chunk_blocks,
                _g_head_block,
                _g_safe_head_block,
                _g_next_block,
                _g_backlog_blocks,
                _g_chunk_blocks,
            }),
        }
    }

    pub fn set_chain_position(&self, head: u64, safe_head: u64, next: u64, chunk: u64) {
        self.inner.head_block.store(head, Ordering::Relaxed);
        self.inner
            .safe_head_block
            .store(safe_head, Ordering::Relaxed);
        self.inner.next_block.store(next, Ordering::Relaxed);
        self.inner.chunk_blocks.store(chunk, Ordering::Relaxed);
        let backlog = if next > safe_head {
            0
        } else {
            safe_head - next + 1
        };
        self.inner.backlog_blocks.store(backlog, Ordering::Relaxed);
    }

    pub fn observe_range(
        &self,
        from_block: u64,
        to_block: u64,
        event_logs: u64,
        proof_logs: u64,
        range_total_ms: u64,
    ) {
        let blocks = to_block.saturating_sub(from_block).saturating_add(1);
        let logs = event_logs.saturating_add(proof_logs);
        self.inner.ranges_total.add(1, &self.inner.attrs);
        self.inner.blocks_total.add(blocks, &self.inner.attrs);
        self.inner.logs_total.add(logs, &self.inner.attrs);
        self.inner
            .range_total_ms
            .record(range_total_ms, &self.inner.attrs);
    }

    pub fn observe_rpc_latency_ms(&self, method: &'static str, ms: u64) {
        self.inner.rpc_latency_ms.record(
            ms,
            &[
                self.inner.attrs[0].clone(),
                self.inner.attrs[1].clone(),
                KeyValue::new("method", method),
            ],
        );
    }

    pub fn observe_db_latency_ms(&self, op: &'static str, ms: u64) {
        self.inner.db_latency_ms.record(
            ms,
            &[
                self.inner.attrs[0].clone(),
                self.inner.attrs[1].clone(),
                KeyValue::new("op", op),
            ],
        );
    }

    pub fn observe_timestamp_enrichment_ms(&self, ms: u64) {
        self.inner
            .timestamp_enrichment_ms
            .record(ms, &self.inner.attrs);
    }

    pub fn rpc_error(&self, method: &'static str) {
        self.inner.rpc_errors_total.add(
            1,
            &[
                self.inner.attrs[0].clone(),
                self.inner.attrs[1].clone(),
                KeyValue::new("method", method),
            ],
        );
    }

    pub fn db_error(&self, op: &'static str) {
        self.inner.db_errors_total.add(
            1,
            &[
                self.inner.attrs[0].clone(),
                self.inner.attrs[1].clone(),
                KeyValue::new("op", op),
            ],
        );
    }

    pub fn reorg_detected(&self) {
        self.inner.reorgs_total.add(1, &self.inner.attrs);
    }

    pub fn rows_upserted(&self, table: &'static str, rows: u64) {
        self.inner.rows_upserted_total.add(
            rows,
            &[
                self.inner.attrs[0].clone(),
                self.inner.attrs[1].clone(),
                KeyValue::new("table", table),
            ],
        );
    }
}
