use crate::process::null_stdio;
use crate::util::repo_root;
use anyhow::{Context, Result};
use std::process::{Child, Command, Stdio};

pub fn spawn_anvil(port: u16) -> Result<Child> {
    let mut cmd = Command::new("anvil");
    // Bind to 0.0.0.0 so Docker containers (e.g. Alto) can reach the host-run Anvil via
    // `host.docker.internal`. Binding to 127.0.0.1 makes it unreachable from containers.
    cmd.args(["--host", "0.0.0.0", "--port", &port.to_string(), "--silent"])
        .current_dir(repo_root())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    null_stdio(&mut cmd);
    cmd.spawn().context("spawn anvil")
}

pub fn spawn_anvil_with_block_time(port: u16, block_time_secs: u64) -> Result<Child> {
    let mut cmd = Command::new("anvil");
    cmd.args([
        "--host",
        "0.0.0.0",
        "--port",
        &port.to_string(),
        "--block-time",
        &block_time_secs.to_string(),
        "--silent",
    ])
    .current_dir(repo_root())
    .stdout(Stdio::inherit())
    .stderr(Stdio::inherit());
    null_stdio(&mut cmd);
    cmd.spawn().context("spawn anvil (block time)")
}
