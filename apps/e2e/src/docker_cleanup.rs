use anyhow::{Context, Result};
use std::process::Command;

/// Best-effort cleanup of containers created by Untron e2e tests.
///
/// Why this exists: aborted test runs can leave docker containers around (still holding ports),
/// which makes subsequent e2e runs flaky.
///
/// Safety: by default, we only remove containers whose **name starts with `untron-e2e-`**.
/// This prefix is reserved for tests in this repo.
///
/// Set `UNTRON_E2E_DOCKER_CLEANUP=0` to disable.
pub fn cleanup_untron_e2e_containers() -> Result<()> {
    let enabled = std::env::var("UNTRON_E2E_DOCKER_CLEANUP")
        .map(|v| v != "0")
        .unwrap_or(true);
    if !enabled {
        return Ok(());
    }

    let force_remove_running = std::env::var("UNTRON_E2E_DOCKER_CLEANUP_FORCE")
        .map(|v| v == "1")
        .unwrap_or(false);

    let out = Command::new("docker")
        .args(["ps", "-a", "--format", "{{.ID}} {{.Names}} {{.Status}}"])
        .output()
        .context("docker ps -a")?;
    if !out.status.success() {
        // If docker isn't available, don't fail the test.
        return Ok(());
    }

    let s = String::from_utf8_lossy(&out.stdout);
    for line in s.lines() {
        let mut parts = line.split_whitespace();
        let Some(id) = parts.next() else { continue };
        let Some(name) = parts.next() else { continue };
        let status = parts.collect::<Vec<_>>().join(" ");

        if !name.starts_with("untron-e2e-") {
            continue;
        }

        // Avoid interfering with concurrently-running tests. Only remove stopped containers by
        // default. To force-remove running containers, set UNTRON_E2E_DOCKER_CLEANUP_FORCE=1.
        let is_running = status.starts_with("Up ");
        if is_running && !force_remove_running {
            continue;
        }

        // Best-effort; ignore failures (race with other cleanup or container already gone).
        let _ = Command::new("docker").args(["rm", "-f", id]).output();
    }

    Ok(())
}
