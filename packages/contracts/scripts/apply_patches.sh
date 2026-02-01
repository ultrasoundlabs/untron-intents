#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

SAFE_MODULES_DIR="$ROOT/lib/safe-modules"
PATCH="$ROOT/patches/safe-modules-4337-solc-pragma.patch"

if [[ ! -d "$SAFE_MODULES_DIR" ]]; then
  echo "safe-modules not present at $SAFE_MODULES_DIR; skipping patches" >&2
  exit 0
fi

if [[ ! -f "$PATCH" ]]; then
  echo "missing patch file: $PATCH" >&2
  exit 1
fi

pushd "$SAFE_MODULES_DIR" >/dev/null

# Idempotent apply: if patch is already applied, do nothing.
if git apply --check "$PATCH" >/dev/null 2>&1; then
  git apply "$PATCH"
  echo "applied: $(basename "$PATCH")"
else
  if git apply --reverse --check "$PATCH" >/dev/null 2>&1; then
    echo "already applied: $(basename "$PATCH")"
  else
    echo "failed to apply patch (and not already applied): $(basename "$PATCH")" >&2
    exit 1
  fi
fi

popd >/dev/null

