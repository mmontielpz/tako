#!/usr/bin/env bash
# Development launch script: starts the Vite frontend dev server and Tauri Desktop shell.
set -euo pipefail
cd "$(dirname "$0")/.."

for cmd in node npm cargo curl; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "ERROR: Required tool '$cmd' not found on PATH." >&2
    exit 1
  fi
done

if [ ! -d ui/node_modules ]; then
  echo "ERROR: ui/node_modules is missing. Run 'npm --prefix ui ci' first." >&2
  exit 1
fi

if [ -z "${TAKO_PYTHON_CLI_BIN:-}" ]; then
  echo "WARN: TAKO_PYTHON_CLI_BIN is not set. Set it to an isolated TAKO CLI (>= 0.1.0a3) to enable Python SOTA execution." >&2
fi

VITE_PID=""
CARGO_PID=""

kill_tree() {
  local pid="${1:-}"
  [ -z "$pid" ] && return 0
  if command -v pkill >/dev/null 2>&1; then
    pkill -TERM -P "$pid" 2>/dev/null || true
  fi
  kill -TERM "$pid" 2>/dev/null || true
  sleep 0.3
  if command -v pkill >/dev/null 2>&1; then
    pkill -KILL -P "$pid" 2>/dev/null || true
  fi
  kill -KILL "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
}

cleanup() {
  trap - EXIT INT TERM
  kill_tree "$CARGO_PID"
  kill_tree "$VITE_PID"
}
trap cleanup EXIT INT TERM

npm --prefix ui run dev &
VITE_PID=$!

for _ in $(seq 1 40); do
  if curl -sSf http://localhost:1420 >/dev/null 2>&1; then
    break
  fi
  sleep 0.25
done

if ! curl -sSf http://localhost:1420 >/dev/null 2>&1; then
  echo "ERROR: Vite dev server failed to start on http://localhost:1420." >&2
  exit 1
fi

cargo run --manifest-path src-tauri/Cargo.toml --offline --locked "$@" &
CARGO_PID=$!
wait "$CARGO_PID"
