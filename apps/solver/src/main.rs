mod config;
mod db;
mod hub;
mod indexer;
mod metrics;
mod runner;
mod tron_backend;
mod types;

use anyhow::{Context, Result};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let cfg = config::load_config()?;
    let otel = untron_observability::init(untron_observability::Config {
        service_name: "solver",
        service_version: env!("CARGO_PKG_VERSION"),
    })?;
    let telemetry = metrics::SolverTelemetry::new();

    tracing::info!("solver starting");
    tracing::info!(
        indexer = %cfg.indexer.base_url,
        hub_rpc = %cfg.hub.rpc_url,
        tron_mode = ?cfg.tron.mode,
        "config loaded"
    );

    let shutdown = CancellationToken::new();

    let mut join_set = tokio::task::JoinSet::new();
    {
        let shutdown = shutdown.clone();
        let telemetry = telemetry.clone();
        join_set.spawn(async move {
            let solver = runner::Solver::new(cfg, telemetry).await?;
            solver.run(shutdown).await
        });
    }

    tracing::info!("solver started");

    let mut fatal: Option<anyhow::Error> = None;
    tokio::select! {
        res = shutdown_signal() => {
            res?;
            tracing::info!("shutdown requested");
        },
        res = join_set.join_next() => {
            if let Some(res) = res {
                let res = res.context("solver task panicked")?;
                match res {
                    Ok(()) => fatal = Some(anyhow::anyhow!("solver task exited unexpectedly")),
                    Err(e) => fatal = Some(e.context("solver task failed")),
                }
            }
        }
    }

    shutdown.cancel();

    while let Some(res) = join_set.join_next().await {
        let res = res.context("solver task panicked")?;
        if let Err(e) = res {
            fatal.get_or_insert_with(|| e.context("solver task failed"));
        }
    }

    otel.shutdown().await;
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
