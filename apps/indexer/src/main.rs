mod config;
mod db;
mod decode;
mod logs;
mod metrics;
mod rpc;
mod runner;
mod timestamps;

use anyhow::{Context, Result};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let cfg = config::load_config()?;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    tracing::info!("indexer starting");
    tracing::info!(
        pool_chain_id = cfg.pool.chain_id,
        pool_contract = %cfg.pool.contract_address,
        pool_rpc_urls = cfg.pool.rpc.urls.len(),
        forwarder_instances = cfg.forwarders.len(),
        only_stream = ?cfg.only_stream,
        "config loaded"
    );

    let shutdown = CancellationToken::new();

    let mut join_set: tokio::task::JoinSet<Result<()>> = tokio::task::JoinSet::new();
    {
        let shutdown = shutdown.clone();
        join_set.spawn(async move { runner::run(cfg, shutdown).await });
    }

    tracing::info!("indexer started");

    let mut fatal: Option<anyhow::Error> = None;
    tokio::select! {
        res = shutdown_signal() => {
            res?;
            tracing::info!("shutdown requested");
        },
        res = join_set.join_next() => {
            if let Some(res) = res {
                let res = res.context("indexer task panicked")?;
                match res {
                    Ok(()) => fatal = Some(anyhow::anyhow!("indexer task exited unexpectedly")),
                    Err(e) => fatal = Some(e.context("indexer task failed")),
                }
            }
        }
    }

    shutdown.cancel();

    while let Some(res) = join_set.join_next().await {
        let res = res.context("indexer task panicked")?;
        if let Err(e) = res {
            fatal.get_or_insert_with(|| e.context("indexer task failed"));
        }
    }

    fatal.map_or(Ok(()), Err)
}

async fn shutdown_signal() -> Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut sigterm = signal(SignalKind::terminate()).context("install SIGTERM handler")?;
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = sigterm.recv() => {},
        }
        Ok(())
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await.context("ctrl-c")?;
        Ok(())
    }
}
