use crate::{
    config::{AppConfig, InstanceConfig, Stream, StreamSelection},
    db, decode, logs,
    metrics::StreamTelemetry,
    rpc::{self, RpcClient},
    timestamps,
};
use alloy::sol_types::SolEvent;
use anyhow::{Context, Result};
use std::time::{Duration, Instant};
use tokio::time;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

fn event_appended_topic0(stream: Stream) -> String {
    let sig = match stream {
        Stream::Pool => {
            <untron_intents_bindings::untron_intents_index::UntronIntentsIndex::EventAppended as SolEvent>::SIGNATURE_HASH
        }
        Stream::Forwarder => {
            <untron_intents_bindings::intents_forwarder_index::IntentsForwarderIndex::EventAppended as SolEvent>::SIGNATURE_HASH
        }
    };
    format!("0x{}", hex::encode(sig.as_slice()))
}

pub async fn run(cfg: AppConfig, shutdown: CancellationToken) -> Result<()> {
    let dbh = db::Db::connect(&cfg.database_url, cfg.db_max_connections).await?;
    let _schema_version = db::ensure_schema_version(&dbh, 5).await?;

    let block_timestamp_cache_size = cfg.block_timestamp_cache_size;
    let block_header_concurrency = cfg.block_header_concurrency;

    let mut instances = Vec::new();
    instances.push(cfg.pool.clone());
    instances.extend(cfg.forwarders.clone());

    if instances.is_empty() {
        anyhow::bail!("no instances configured");
    }

    // Filter instances by optional selection.
    if let Some(sel) = cfg.only_stream {
        match sel {
            StreamSelection::Pool => instances.retain(|i| i.stream == Stream::Pool),
            StreamSelection::Forwarder => instances.retain(|i| i.stream == Stream::Forwarder),
            StreamSelection::All => {}
        }
    }

    let mut join_set: tokio::task::JoinSet<Result<()>> = tokio::task::JoinSet::new();
    for inst in instances {
        let dbh = dbh.clone();
        let shutdown = shutdown.clone();
        let progress_interval = cfg.progress_interval;
        let tail_lag_blocks = cfg.progress_tail_lag_blocks;
        join_set.spawn(async move {
            let mut backoff = Duration::from_millis(250);
            loop {
                if shutdown.is_cancelled() {
                    return Ok(());
                }
                let res = run_instance(
                    &dbh,
                    &inst,
                    progress_interval,
                    tail_lag_blocks,
                    block_timestamp_cache_size,
                    block_header_concurrency,
                    &shutdown,
                )
                .await;
                match res {
                    Ok(()) => warn!(
                        stream = inst.stream.as_str(),
                        chain_id = inst.chain_id,
                        contract = %inst.contract_address,
                        "instance task exited; restarting"
                    ),
                    Err(e) => {
                        error!(
                            stream = inst.stream.as_str(),
                            chain_id = inst.chain_id,
                            contract = %inst.contract_address,
                            err = ?e,
                            "instance task failed; restarting"
                        );
                    }
                }
                time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(5));
            }
        });
    }

    // Wait for shutdown or first task error (tasks are expected to run forever).
    tokio::select! {
        _ = shutdown.cancelled() => {},
        res = join_set.join_next() => {
            if let Some(res) = res {
                return res.context("instance task panicked")?;
            }
        }
    }

    while let Some(res) = join_set.join_next().await {
        let res = res.context("instance task panicked")?;
        if let Err(e) = res {
            warn!(err = %e, "instance exited with error during shutdown");
        }
    }

    Ok(())
}

async fn run_instance(
    dbh: &db::Db,
    cfg: &InstanceConfig,
    progress_interval: Duration,
    tail_lag_blocks: u64,
    block_timestamp_cache_size: usize,
    block_header_concurrency: usize,
    shutdown: &CancellationToken,
) -> Result<()> {
    db::ensure_instance_config(dbh, cfg.stream, cfg.chain_id, &cfg.contract_address).await?;

    let rpc = RpcClient::new(cfg.rpc.urls.clone()).context("build rpc client")?;

    let mut from_block = db::resume_from_block(
        dbh,
        cfg.stream,
        cfg.chain_id,
        &cfg.contract_address,
        cfg.deployment_block,
    )
    .await?;

    let mut timestamps_cache = timestamps::TimestampCache::new(block_timestamp_cache_size);
    let topic0 = event_appended_topic0(cfg.stream);
    let telemetry = StreamTelemetry::new(cfg.stream, cfg.chain_id);

    let mut ticker = time::interval(cfg.poll_interval.max(Duration::from_secs(1)));
    ticker.set_missed_tick_behavior(time::MissedTickBehavior::Delay);

    let mut chunk_current = cfg.chunk_blocks.max(1);
    let chunk_target = cfg.chunk_blocks.max(1);

    let mut last_progress_at = Instant::now();
    let mut transient_attempts: u32 = 0;
    let mut transient_backoff = Duration::from_millis(250);

    info!(
        stream = cfg.stream.as_str(),
        chain_id = cfg.chain_id,
        contract = %cfg.contract_address,
        from_block,
        confirmations = cfg.confirmations,
        poll_interval_secs = cfg.poll_interval.as_secs(),
        chunk_blocks = cfg.chunk_blocks,
        reorg_scan_depth = cfg.reorg_scan_depth,
        "instance starting"
    );

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return Ok(()),
            _ = ticker.tick() => {}
        }

        let head_start = Instant::now();
        let head = rpc.block_number().await.map_err(|e| {
            telemetry.rpc_error("eth_blockNumber");
            e.context("eth_blockNumber")
        })?;
        telemetry
            .observe_rpc_latency_ms("eth_blockNumber", head_start.elapsed().as_millis() as u64);

        let safe_head = head.saturating_sub(cfg.confirmations);
        telemetry.set_chain_position(head, safe_head, from_block, chunk_current);

        if last_progress_at.elapsed() >= progress_interval.max(Duration::from_secs(1)) {
            let backlog_blocks = if from_block > safe_head {
                0
            } else {
                safe_head.saturating_sub(from_block).saturating_add(1)
            };
            let stage = if backlog_blocks <= tail_lag_blocks {
                "tail"
            } else {
                "backfill"
            };
            info!(
                stream = cfg.stream.as_str(),
                chain_id = cfg.chain_id,
                contract = %cfg.contract_address,
                stage,
                head,
                safe_head,
                next_block = from_block,
                backlog_blocks,
                chunk_blocks = chunk_current,
                "progress"
            );
            last_progress_at = Instant::now();
        }

        // Reorg check based on stored canonical block hash.
        if let Some(reorg_start) = detect_reorg_start(
            dbh,
            &rpc,
            cfg.stream,
            cfg.chain_id,
            &cfg.contract_address,
            cfg.reorg_scan_depth,
        )
        .await?
        {
            warn!(
                stream = cfg.stream.as_str(),
                chain_id = cfg.chain_id,
                contract = %cfg.contract_address,
                reorg_start,
                "reorg detected; invalidating"
            );
            telemetry.reorg_detected();
            db::invalidate_from_block(
                dbh,
                cfg.stream,
                cfg.chain_id,
                &cfg.contract_address,
                reorg_start,
            )
            .await?;
            timestamps_cache.clear();
            from_block = from_block.min(reorg_start);
        }

        while from_block <= safe_head {
            if shutdown.is_cancelled() {
                return Ok(());
            }

            let to_block =
                safe_head.min(from_block.saturating_add(chunk_current.saturating_sub(1)));
            let mut range_ctx = RangeCtx {
                dbh,
                cfg,
                rpc: &rpc,
                timestamps_cache: &mut timestamps_cache,
                topic0: &topic0,
                telemetry: &telemetry,
                shutdown,
                block_header_concurrency,
            };
            match process_range(&mut range_ctx, from_block, to_block).await {
                Ok((logs_count, total_ms)) => {
                    telemetry.observe_range(from_block, to_block, logs_count, 0, total_ms);
                    from_block = to_block.saturating_add(1);
                    transient_attempts = 0;
                    transient_backoff = Duration::from_millis(250);
                    chunk_current = grow_chunk(chunk_current, chunk_target);
                }
                Err(e) => {
                    if rpc::looks_like_transient(&e) && transient_attempts < 3 {
                        transient_attempts += 1;
                        warn!(
                            stream = cfg.stream.as_str(),
                            chain_id = cfg.chain_id,
                            contract = %cfg.contract_address,
                            from_block,
                            to_block,
                            attempt = transient_attempts,
                            err = %e,
                            "transient error; retrying range"
                        );
                        time::sleep(transient_backoff).await;
                        transient_backoff = (transient_backoff * 2).min(Duration::from_secs(2));
                        continue;
                    }

                    // If projections reject the inserted rows due to a hash-chain discontinuity,
                    // treat it like a deep reorg and force invalidation from an earlier block.
                    if looks_like_tip_mismatch(&e) {
                        let fallback_from = from_block.saturating_sub(cfg.reorg_scan_depth.max(1));
                        warn!(
                            stream = cfg.stream.as_str(),
                            chain_id = cfg.chain_id,
                            contract = %cfg.contract_address,
                            from_block,
                            to_block,
                            fallback_from,
                            err = %e,
                            "tip mismatch during projection; forcing invalidation and retry"
                        );
                        db::invalidate_from_block(
                            dbh,
                            cfg.stream,
                            cfg.chain_id,
                            &cfg.contract_address,
                            fallback_from,
                        )
                        .await?;
                        timestamps_cache.clear();
                        from_block = from_block.min(fallback_from);
                        chunk_current = 1;
                        transient_attempts = 0;
                        transient_backoff = Duration::from_millis(250);
                        continue;
                    }

                    if chunk_current > 1 && rpc::looks_like_range_too_large(&e) {
                        chunk_current = shrink_chunk(chunk_current);
                        warn!(
                            stream = cfg.stream.as_str(),
                            chain_id = cfg.chain_id,
                            contract = %cfg.contract_address,
                            from_block,
                            to_block,
                            chunk_blocks = chunk_current,
                            err = %e,
                            "eth_getLogs failed; shrinking chunk"
                        );
                        transient_attempts = 0;
                        transient_backoff = Duration::from_millis(250);
                        continue;
                    }

                    return Err(e);
                }
            }
        }
    }
}

fn looks_like_tip_mismatch(err: &anyhow::Error) -> bool {
    let msg = err.to_string().to_lowercase();
    msg.contains("tip mismatch")
        || (msg.contains("prev_tip") && msg.contains("expected"))
        || msg.contains("hash-chain")
}

fn grow_chunk(current: u64, target: u64) -> u64 {
    if current >= target {
        return current;
    }
    current.saturating_mul(2).min(target)
}

fn shrink_chunk(current: u64) -> u64 {
    (current / 2).max(1)
}

async fn detect_reorg_start(
    dbh: &db::Db,
    rpc: &RpcClient,
    stream: Stream,
    chain_id: u64,
    contract_address: &str,
    scan_depth: u64,
) -> Result<Option<u64>> {
    let Some(latest) =
        db::latest_canonical_block_hash(dbh, stream, chain_id, contract_address).await?
    else {
        return Ok(None);
    };

    let Some(block) = rpc
        .get_block_by_number(latest.block_number)
        .await
        .with_context(|| format!("eth_getBlockByNumber({})", latest.block_number))?
    else {
        return Ok(None);
    };

    let latest_rpc_hash = timestamps::parse_block_hash(&block)?;
    if latest_rpc_hash == latest.block_hash_hex {
        debug!(
            stream = stream.as_str(),
            chain_id,
            contract = %contract_address,
            block_number = latest.block_number,
            "reorg check: latest block matches"
        );
        return Ok(None);
    }

    let scan_depth = scan_depth.max(1);
    let mut stored =
        db::recent_canonical_block_hashes(dbh, stream, chain_id, contract_address, scan_depth)
            .await?;
    if stored.is_empty() {
        return Ok(Some(latest.block_number));
    }
    stored.sort_by_key(|b| b.block_number);

    // Ensure the mismatching latest block is included.
    if stored.last().map(|b| b.block_number) != Some(latest.block_number) {
        stored.push(latest.clone());
        stored.sort_by_key(|b| b.block_number);
    }

    let mut left = 0usize;
    let mut right = stored.len();
    while left < right {
        let mid = (left + right) / 2;
        let b = &stored[mid];
        let Some(block) = rpc
            .get_block_by_number(b.block_number)
            .await
            .with_context(|| format!("eth_getBlockByNumber({})", b.block_number))?
        else {
            return Ok(None);
        };
        let rpc_hash = timestamps::parse_block_hash(&block)?;
        if rpc_hash == b.block_hash_hex {
            left = mid + 1;
        } else {
            right = mid;
        }
    }

    if left >= stored.len() {
        return Ok(None);
    }

    Ok(Some(stored[left].block_number))
}

struct RangeCtx<'a> {
    dbh: &'a db::Db,
    cfg: &'a InstanceConfig,
    rpc: &'a RpcClient,
    timestamps_cache: &'a mut timestamps::TimestampCache,
    topic0: &'a str,
    telemetry: &'a StreamTelemetry,
    shutdown: &'a CancellationToken,
    block_header_concurrency: usize,
}

async fn process_range(
    ctx: &mut RangeCtx<'_>,
    from_block: u64,
    to_block: u64,
) -> Result<(u64, u64)> {
    let start = Instant::now();
    let filter = serde_json::json!({
        "address": ctx.cfg.contract_address.as_str(),
        "fromBlock": rpc::format_quantity(from_block),
        "toBlock": rpc::format_quantity(to_block),
        "topics": [ctx.topic0],
    });

    let rpc_start = Instant::now();
    let raw_logs = ctx.rpc.get_logs(filter).await.map_err(|e| {
        ctx.telemetry.rpc_error("eth_getLogs_EventAppended");
        e.context("eth_getLogs(EventAppended)")
    })?;
    ctx.telemetry.observe_rpc_latency_ms(
        "eth_getLogs_EventAppended",
        rpc_start.elapsed().as_millis() as u64,
    );

    let logs = logs::validate_and_sort_logs(raw_logs)?;
    if logs.is_empty() {
        return Ok((0, start.elapsed().as_millis() as u64));
    }

    let block_numbers = logs.iter().map(|l| l.block_number).collect::<Vec<_>>();
    let ts_start = Instant::now();
    timestamps::populate_timestamps(
        ctx.shutdown,
        ctx.rpc,
        ctx.timestamps_cache,
        &block_numbers,
        ctx.block_header_concurrency,
    )
    .await?;
    ctx.telemetry
        .observe_timestamp_enrichment_ms(ts_start.elapsed().as_millis() as u64);

    let decode_start = Instant::now();
    let mut rows = Vec::with_capacity(logs.len());
    for l in logs {
        if ctx.shutdown.is_cancelled() {
            break;
        }
        let ts = ctx
            .timestamps_cache
            .get(l.block_number)
            .with_context(|| format!("missing timestamp for block {}", l.block_number))?;

        let row = decode_event_appended(
            ctx.cfg.stream,
            ctx.cfg.chain_id,
            &ctx.cfg.contract_address,
            ts,
            l,
        )?;
        rows.push(row);
    }
    debug!(
    stream = ctx.cfg.stream.as_str(),
    chain_id = ctx.cfg.chain_id,
    contract = %ctx.cfg.contract_address,
    from_block,
    to_block,
    rows = rows.len(),
    decode_ms = decode_start.elapsed().as_millis() as u64,
    total_ms = start.elapsed().as_millis() as u64,
    "range decoded"
    );

    let db_start = Instant::now();
    db::insert_event_appended_batch(ctx.dbh, &rows)
        .await
        .inspect_err(|_| {
            ctx.telemetry.db_error("insert_event_appended_batch");
        })?;
    ctx.telemetry.observe_db_latency_ms(
        "insert_event_appended_batch",
        db_start.elapsed().as_millis() as u64,
    );
    ctx.telemetry
        .rows_upserted("chain.event_appended", rows.len() as u64);

    Ok((rows.len() as u64, start.elapsed().as_millis() as u64))
}

fn decode_event_appended(
    stream: Stream,
    chain_id: u64,
    contract_address: &str,
    block_timestamp: u64,
    log: logs::ValidatedLog,
) -> Result<db::EventAppendedRow> {
    let (event_seq, prev_tip, new_tip, event_signature, abi_encoded_event_data) = match stream {
        Stream::Pool => {
            let decoded = log
                .log
                .log_decode::<untron_intents_bindings::untron_intents_index::UntronIntentsIndex::EventAppended>()
                .map_err(|e| anyhow::anyhow!("EventAppended decode failed: {e}"))?;
            let ev = decoded.inner.data;
            (
                ev.eventSeq,
                ev.prevTip,
                ev.newTip,
                ev.eventSignature,
                ev.abiEncodedEventData,
            )
        }
        Stream::Forwarder => {
            let decoded = log
                .log
                .log_decode::<untron_intents_bindings::intents_forwarder_index::IntentsForwarderIndex::EventAppended>()
                .map_err(|e| anyhow::anyhow!("EventAppended decode failed: {e}"))?;
            let ev = decoded.inner.data;
            (
                ev.eventSeq,
                ev.prevTip,
                ev.newTip,
                ev.eventSignature,
                ev.abiEncodedEventData,
            )
        }
    };

    let event_seq_u64 =
        u64::try_from(event_seq).with_context(|| format!("event_seq too large: {event_seq}"))?;
    let semantic = decode::decode_semantic_event(stream, event_signature, &abi_encoded_event_data)?;
    let (event_type, args_json) = semantic.into_db_parts();

    Ok(db::EventAppendedRow {
        stream,
        chain_id: i64::try_from(chain_id).context("chain_id out of range")?,
        contract_address: contract_address.to_string(),
        block_number: i64::try_from(log.block_number).context("block_number out of range")?,
        block_timestamp: i64::try_from(block_timestamp).context("block_timestamp out of range")?,
        block_hash: format!("0x{}", hex::encode(log.block_hash.as_slice())),
        tx_hash: format!("0x{}", hex::encode(log.tx_hash.as_slice())),
        log_index: i32::try_from(log.log_index).context("log_index out of range")?,
        event_seq: i64::try_from(event_seq_u64).context("event_seq out of range")?,
        prev_tip: format!("0x{}", hex::encode(prev_tip.as_slice())),
        new_tip: format!("0x{}", hex::encode(new_tip.as_slice())),
        event_signature: format!("0x{}", hex::encode(event_signature.as_slice())),
        abi_encoded_event_data: format!("0x{}", hex::encode(abi_encoded_event_data.as_ref())),
        event_type: event_type.into_owned(),
        args_json,
    })
}
