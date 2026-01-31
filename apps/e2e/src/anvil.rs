use crate::process::null_stdio;
use crate::util::repo_root;
use anyhow::{Context, Result};
use std::process::{Child, Command, Stdio};

pub fn spawn_anvil(port: u16) -> Result<Child> {
    let mut cmd = Command::new("anvil");
    cmd.args(["--port", &port.to_string(), "--silent"])
        .current_dir(repo_root())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    null_stdio(&mut cmd);
    cmd.spawn().context("spawn anvil")
}
