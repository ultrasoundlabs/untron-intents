use anyhow::{Context, Result};
use std::{
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub fn find_free_port() -> Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).context("bind ephemeral port")?;
    let port = listener.local_addr().context("local_addr")?.port();
    Ok(port)
}

pub fn repo_root() -> PathBuf {
    // apps/e2e -> apps -> repo root
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(Path::to_path_buf)
        .expect("CARGO_MANIFEST_DIR has apps/e2e shape")
}

pub fn command_exists(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

pub fn require_bins(bins: &[&str]) -> bool {
    for &bin in bins {
        if !command_exists(bin) {
            eprintln!("skipping e2e test: missing `{bin}` in PATH");
            return false;
        }
    }
    true
}
