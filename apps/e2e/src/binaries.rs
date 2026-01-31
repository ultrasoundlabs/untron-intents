use crate::util::repo_root;
use anyhow::{Context, Result};
use std::process::{Command, Stdio};

pub fn cargo_build_indexer_bins() -> Result<()> {
    let root = repo_root();
    let status = Command::new("cargo")
        .args([
            "build", "-p", "indexer", "--bin", "indexer", "--bin", "migrate", "--quiet",
        ])
        .current_dir(&root)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("cargo build indexer binaries")?;
    if !status.success() {
        anyhow::bail!("failed to build indexer binaries");
    }
    Ok(())
}

pub fn cargo_build_solver_bin() -> Result<()> {
    let root = repo_root();
    let status = Command::new("cargo")
        .args(["build", "-p", "solver", "--bin", "solver", "--quiet"])
        .current_dir(&root)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("cargo build solver binary")?;
    if !status.success() {
        anyhow::bail!("failed to build solver binary");
    }
    Ok(())
}

pub fn run_migrations(db_url: &str, no_notify_pgrst: bool) -> Result<()> {
    let root = repo_root();
    let mut cmd = Command::new(root.join("target/debug/migrate"));
    if no_notify_pgrst {
        cmd.arg("--no-notify-pgrst");
    }
    let status = cmd
        .current_dir(&root)
        .env("DATABASE_URL", db_url)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("run migrations")?;
    if !status.success() {
        anyhow::bail!("migrations failed");
    }
    Ok(())
}
