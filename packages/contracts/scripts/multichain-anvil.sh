#!/usr/bin/env bash
set -euo pipefail

# Set DEBUG=1 for verbose execution.
if [[ "${DEBUG:-0}" == "1" ]]; then
  set -x
fi

trap 'echo "[multichain-anvil] failed at line ${LINENO}: ${BASH_COMMAND}" >&2' ERR

# Spins up multiple Anvil chains, deploys the protocol via CREATE3 (CreateX),
# and generates on-chain activity for indexer testing.
#
# Prereqs: `anvil`, `cast`, `forge` on PATH.
#
# Example:
#   PRIVATE_KEY=0x... ./scripts/multichain-anvil.sh
#
# Notes:
# - Each chain MUST have CreateX deployed at the canonical address; `DeployProtocol` will auto-etch it on Anvil.
# - Use the same deployer key across chains so CREATE3 addresses match.
# - By default we pass `--skip-simulation` to `forge script --broadcast` to avoid interactive prompts (and hangs) caused
#   by Foundry warnings about calling addresses that don't yet have code in the pre-state. Set `FORGE_SKIP_SIMULATION=0`
#   if you want on-chain simulation enabled (may require answering prompts).

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

HUB_CHAIN_ID="${HUB_CHAIN_ID:-222}"
SPOKE_CHAIN_IDS_CSV="${SPOKE_CHAIN_IDS:-111,333,444}"
BASE_PORT="${BASE_PORT:-8545}"
HOST="${HOST:-127.0.0.1}"

PRIVATE_KEY="${PRIVATE_KEY:-}"
if [[ -z "${PRIVATE_KEY}" ]]; then
  echo "Set PRIVATE_KEY (hex) to run deploy+activity broadcasts."
  exit 1
fi

MNEMONIC="${MNEMONIC:-test test test test test test test test test test test junk}"
# Default to Anvil automine (mines on every tx). Set to e.g. `1` to also mine on an interval.
ANVIL_BLOCK_TIME="${ANVIL_BLOCK_TIME:-0}"
ANVIL_BASE_FEE="${ANVIL_BASE_FEE:-0}"
ANVIL_GAS_PRICE="${ANVIL_GAS_PRICE:-1000000000}" # 1 gwei
ANVIL_DISABLE_MIN_PRIORITY_FEE="${ANVIL_DISABLE_MIN_PRIORITY_FEE:-1}"

rpc_call() {
  local rpc_url="$1"
  local method="$2"
  local params_json="$3"

  if ! command -v curl >/dev/null 2>&1; then
    echo "[multichain-anvil] Missing required dependency: curl" >&2
    return 1
  fi

  local payload resp
  payload=$(printf '{"jsonrpc":"2.0","id":1,"method":"%s","params":%s}' "${method}" "${params_json}")
  resp="$(curl -sS -H 'Content-Type: application/json' --data "${payload}" "${rpc_url}")"
  # JSON-RPC success responses never include `"error":{...}`.
  if [[ "${resp}" == *"\"error\""* ]]; then
    echo "[multichain-anvil] RPC error (${method}) -> ${resp}" >&2
    return 1
  fi
  return 0
}

rpc_request() {
  local rpc_url="$1"
  local method="$2"
  local params_json="$3"

  local payload resp
  payload=$(printf '{"jsonrpc":"2.0","id":1,"method":"%s","params":%s}' "${method}" "${params_json}")
  resp="$(curl -sS -H 'Content-Type: application/json' --data "${payload}" "${rpc_url}")"
  if [[ "${resp}" == *"\"error\""* ]]; then
    echo "[multichain-anvil] RPC error (${method}) -> ${resp}" >&2
    return 1
  fi
  echo "${resp}"
  return 0
}

wait_for_rpc() {
  local rpc_url="$1"
  local tries="${2:-60}"
  local delay="${3:-0.1}"

  for _ in $(seq 1 "${tries}"); do
    if rpc_call "${rpc_url}" "web3_clientVersion" "[]" >/dev/null 2>&1; then
      return 0
    fi
    sleep "${delay}"
  done

  echo "[multichain-anvil] RPC not responding: ${rpc_url}" >&2
  return 1
}

hex_to_dec() {
  local hex="$1"
  python3 - <<'PY' "${hex}"
import sys
h = sys.argv[1]
if h.startswith("0x"):
    h = h[2:]
print(int(h or "0", 16))
PY
}

rpc_block_number_dec() {
  local rpc_url="$1"
  local resp hex
  resp="$(rpc_request "${rpc_url}" "eth_blockNumber" "[]")"
  hex="$(python3 - <<'PY' "${resp}"
import json,sys
obj=json.loads(sys.argv[1])
print(obj["result"])
PY
)"
  hex_to_dec "${hex}"
}

rpc_latest_timestamp_dec() {
  local rpc_url="$1"
  local resp ts_hex
  resp="$(rpc_request "${rpc_url}" "eth_getBlockByNumber" "[\"latest\",false]")"
  ts_hex="$(python3 - <<'PY' "${resp}"
import json,sys
obj=json.loads(sys.argv[1])
print(obj["result"]["timestamp"])
PY
)"
  hex_to_dec "${ts_hex}"
}

advance_time_seconds() {
  local rpc_url="$1"
  local seconds="$2"

  local now target
  now="$(rpc_latest_timestamp_dec "${rpc_url}")"
  target="$((now + seconds))"

  # Prefer setting an explicit timestamp for the next block.
  if ! rpc_call "${rpc_url}" "evm_setNextBlockTimestamp" "[${target}]" >/dev/null 2>&1; then
    # Fallback: relative time jump.
    rpc_call "${rpc_url}" "evm_increaseTime" "[${seconds}]" >/dev/null 2>&1 || true
  fi
  # Ensure the timestamp update takes effect.
  rpc_call "${rpc_url}" "evm_mine" "[]" >/dev/null 2>&1 || true
}

IFS=',' read -r -a SPOKE_CHAIN_IDS <<< "${SPOKE_CHAIN_IDS_CSV}"

CHAIN_IDS=("${HUB_CHAIN_ID}" "${SPOKE_CHAIN_IDS[@]}")

port_is_open() {
  local host="$1"
  local port="$2"
  python3 - <<'PY' "${host}" "${port}"
import socket, sys
host = sys.argv[1]
port = int(sys.argv[2])
s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
s.settimeout(0.2)
try:
    s.connect((host, port))
    sys.exit(0)
except Exception:
    sys.exit(1)
finally:
    try:
        s.close()
    except Exception:
        pass
PY
}

cleanup() {
  if [[ -n "${ANVIL_PIDS:-}" ]]; then
    echo "Stopping Anvil..."
    for pid in ${ANVIL_PIDS}; do
      kill "${pid}" >/dev/null 2>&1 || true
    done
  fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

ANVIL_PIDS=""

echo "Starting Anvil chains:"
ANVIL_QUIET="${ANVIL_QUIET:-1}"
for i in "${!CHAIN_IDS[@]}"; do
  PORT="$((BASE_PORT + i))"
  if port_is_open "${HOST}" "${PORT}"; then
    echo "[multichain-anvil] Port already in use: ${HOST}:${PORT}" >&2
    echo "[multichain-anvil] Stop the existing process (Ctrl+C in the other terminal), or set BASE_PORT to another value." >&2
    exit 1
  fi
done

for i in "${!CHAIN_IDS[@]}"; do
  CHAIN_ID="${CHAIN_IDS[$i]}"
  PORT="$((BASE_PORT + i))"
  echo "- chainId=${CHAIN_ID} rpc=http://${HOST}:${PORT}"
  ANVIL_FLAGS=(--chain-id "${CHAIN_ID}" --port "${PORT}" --host "${HOST}" --mnemonic "${MNEMONIC}" --block-base-fee-per-gas "${ANVIL_BASE_FEE}" --gas-price "${ANVIL_GAS_PRICE}")
  if [[ "${ANVIL_BLOCK_TIME}" != "0" ]]; then
    ANVIL_FLAGS+=(--block-time "${ANVIL_BLOCK_TIME}" --mixed-mining)
  fi
  if [[ "${ANVIL_DISABLE_MIN_PRIORITY_FEE}" == "1" ]]; then
    ANVIL_FLAGS+=(--disable-min-priority-fee)
  fi
  if [[ "${ANVIL_QUIET}" == "1" ]]; then
    anvil "${ANVIL_FLAGS[@]}" --quiet &
  else
    anvil "${ANVIL_FLAGS[@]}" &
  fi
  ANVIL_PIDS="${ANVIL_PIDS} $!"
done

echo "Waiting for RPC..."
for i in "${!CHAIN_IDS[@]}"; do
  PORT="$((BASE_PORT + i))"
  RPC="http://${HOST}:${PORT}"
  wait_for_rpc "${RPC}"
  # Best-effort: ensure automine is on.
  rpc_call "${RPC}" "evm_setAutomine" "[true]" >/dev/null 2>&1 || true
  rpc_call "${RPC}" "anvil_setAutomine" "[true]" >/dev/null 2>&1 || true
  rpc_call "${RPC}" "anvil_setAutoMine" "[true]" >/dev/null 2>&1 || true
  # Best-effort: ensure timestamps advance between blocks.
  rpc_call "${RPC}" "anvil_setBlockTimestampInterval" "[1]" >/dev/null 2>&1 || true

  # Sanity: ensure we can mine at least one block.
  if [[ "${DEBUG:-0}" == "1" ]]; then
    local_before="$(rpc_block_number_dec "${RPC}")"
    rpc_call "${RPC}" "evm_mine" "[]" >/dev/null 2>&1 || true
    local_after="$(rpc_block_number_dec "${RPC}")"
    echo "[debug] ${RPC} blockNumber ${local_before} -> ${local_after}"
  fi
done

DEPLOYER_ADDR="$(cast wallet address --private-key "${PRIVATE_KEY}")"
DEPLOYER_BALANCE_HEX="${DEPLOYER_BALANCE_HEX:-0x3635C9ADC5DEA00000}" # 1000 ETH

echo "Funding deployer ${DEPLOYER_ADDR} on each chain..."
for i in "${!CHAIN_IDS[@]}"; do
  PORT="$((BASE_PORT + i))"
  RPC="http://${HOST}:${PORT}"
  rpc_call "${RPC}" "anvil_setBalance" "[\"${DEPLOYER_ADDR}\",\"${DEPLOYER_BALANCE_HEX}\"]" >/dev/null
done

echo "Deploying protocol on each chain..."
FORGE_FLAGS="${FORGE_FLAGS:-}"
FORGE_QUIET="${FORGE_QUIET:-}"
if [[ "${DEBUG:-0}" == "1" && -z "${FORGE_FLAGS}" ]]; then
  FORGE_FLAGS="-vvv"
fi
if [[ "${DEBUG:-0}" == "1" && -z "${FORGE_QUIET}" ]]; then
  FORGE_QUIET="0"
fi
if [[ -z "${FORGE_QUIET}" ]]; then
  FORGE_QUIET="1"
fi

QUIET_FLAG=()
if [[ "${FORGE_QUIET}" == "1" ]]; then
  QUIET_FLAG=(-q)
fi

FORGE_SLOW="${FORGE_SLOW:-1}"
FORGE_TIMEOUT="${FORGE_TIMEOUT:-180}"
FORGE_LEGACY="${FORGE_LEGACY:-1}"
FORGE_GAS_PRICE="${FORGE_GAS_PRICE:-1000000000}" # 1 gwei
FORGE_SKIP_SIMULATION="${FORGE_SKIP_SIMULATION:-1}"
FORGE_BROADCAST_FLAGS=()
if [[ "${FORGE_SLOW}" == "1" ]]; then
  FORGE_BROADCAST_FLAGS+=(--slow)
fi
FORGE_BROADCAST_FLAGS+=(--timeout "${FORGE_TIMEOUT}")
if [[ "${FORGE_LEGACY}" == "1" ]]; then
  FORGE_BROADCAST_FLAGS+=(--legacy --with-gas-price "${FORGE_GAS_PRICE}")
else
  FORGE_BROADCAST_FLAGS+=(--with-gas-price "${FORGE_GAS_PRICE}")
fi
if [[ "${FORGE_SKIP_SIMULATION}" == "1" ]]; then
  # Avoid Foundry's interactive "Do you wish to continue?" prompts (txs to addresses that
  # don't yet have code in the pre-state), which can otherwise hang unattended runs.
  FORGE_BROADCAST_FLAGS+=(--skip-simulation)
fi

for i in "${!CHAIN_IDS[@]}"; do
  PORT="$((BASE_PORT + i))"
  RPC="http://${HOST}:${PORT}"
  (
    cd "${ROOT_DIR}"
    MOCKS=true DEPLOY_ALL=true CONFIGURE_FORWARDER=true CONFIGURE_INTENTS=true CREATE3_CROSS_CHAIN_FLAG=0 \
      RPC_URL="${RPC}" forge script script/DeployProtocol.s.sol:DeployProtocol --rpc-url "${RPC}" --broadcast --private-key "${PRIVATE_KEY}" \
      ${FORGE_FLAGS} ${QUIET_FLAG[@]:+"${QUIET_FLAG[@]}"} ${FORGE_BROADCAST_FLAGS[@]:+"${FORGE_BROADCAST_FLAGS[@]}"}
  )
done

echo "Generating activity on spokes..."
for i in "${!SPOKE_CHAIN_IDS[@]}"; do
  CHAIN_ID="${SPOKE_CHAIN_IDS[$i]}"
  # Hub is at index 0; spokes start at index 1.
  PORT="$((BASE_PORT + i + 1))"
  RPC="http://${HOST}:${PORT}"
  (
    cd "${ROOT_DIR}"
    MODE=spoke HUB_CHAIN_ID="${HUB_CHAIN_ID}" USE_CREATE3_PREDICTION=true \
      RPC_URL="${RPC}" forge script script/SimulateActivity.s.sol:SimulateActivity --rpc-url "${RPC}" --broadcast --private-key "${PRIVATE_KEY}" \
      ${FORGE_FLAGS} ${QUIET_FLAG[@]:+"${QUIET_FLAG[@]}"} ${FORGE_BROADCAST_FLAGS[@]:+"${FORGE_BROADCAST_FLAGS[@]}"}
  )
  echo "  spoke chainId=${CHAIN_ID} done"
done

echo "Generating activity on hub..."
HUB_RPC="http://${HOST}:${BASE_PORT}"
(
  cd "${ROOT_DIR}"
  MODE=hub HUB_CHAIN_ID="${HUB_CHAIN_ID}" SPOKE_CHAIN_IDS="${SPOKE_CHAIN_IDS_CSV}" USE_CREATE3_PREDICTION=true \
    RPC_URL="${HUB_RPC}" forge script script/SimulateActivity.s.sol:SimulateActivity --rpc-url "${HUB_RPC}" --broadcast --private-key "${PRIVATE_KEY}" \
    ${FORGE_FLAGS} ${QUIET_FLAG[@]:+"${QUIET_FLAG[@]}"} ${FORGE_BROADCAST_FLAGS[@]:+"${FORGE_BROADCAST_FLAGS[@]}"}
)

TIME_WARP_SECS="${TIME_WARP_SECS:-600}"
echo "Advancing time by ${TIME_WARP_SECS}s on each chain (for time-gated actions)..."
for i in "${!CHAIN_IDS[@]}"; do
  PORT="$((BASE_PORT + i))"
  RPC="http://${HOST}:${PORT}"
  advance_time_seconds "${RPC}" "${TIME_WARP_SECS}"
done

echo "Executing time-gated actions on each chain..."
for i in "${!CHAIN_IDS[@]}"; do
  PORT="$((BASE_PORT + i))"
  RPC="http://${HOST}:${PORT}"
  (
    cd "${ROOT_DIR}"
    RPC_URL="${RPC}" forge script script/SimulateTimedActions.s.sol:SimulateTimedActions --rpc-url "${RPC}" --broadcast --private-key "${PRIVATE_KEY}" \
      ${FORGE_FLAGS} ${QUIET_FLAG[@]:+"${QUIET_FLAG[@]}"} ${FORGE_BROADCAST_FLAGS[@]:+"${FORGE_BROADCAST_FLAGS[@]}"}
  )
done

echo "Done. RPCs:"
for i in "${!CHAIN_IDS[@]}"; do
  CHAIN_ID="${CHAIN_IDS[$i]}"
  PORT="$((BASE_PORT + i))"
  echo "- chainId=${CHAIN_ID} rpc=http://${HOST}:${PORT}"
done

KEEP_RUNNING="${KEEP_RUNNING:-1}"
if [[ "${KEEP_RUNNING}" == "1" ]]; then
  echo ""
  echo "Anvil is running. Press Ctrl+C to stop."
  # Keep the script alive so indexers/tools can connect to the running Anvil instances.
  wait ${ANVIL_PIDS}
fi
