#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

start_epoch="$(date +%s)"
start_human="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
echo "[e2e-retry-backoff] start: ${start_human}"

cargo test -p e2e --test solver_tron_retry_backoff -- --nocapture

end_epoch="$(date +%s)"
end_human="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
elapsed="$((end_epoch - start_epoch))"
minutes="$((elapsed / 60))"
seconds="$((elapsed % 60))"
echo "[e2e-retry-backoff] end:   ${end_human}"
echo "[e2e-retry-backoff] elapsed: ${elapsed}s (${minutes}m ${seconds}s)"
