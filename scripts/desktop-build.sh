#!/usr/bin/env bash
# Standalone build script: builds ui/dist and embeds frontend assets into the native binary (custom-protocol).
set -euo pipefail
cd "$(dirname "$0")/.."

for cmd in node npm cargo; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "ERROR: Required tool '$cmd' not found on PATH." >&2
    exit 1
  fi
done

if [ ! -d ui/node_modules ]; then
  echo "ERROR: ui/node_modules is missing. Run 'npm --prefix ui ci' first." >&2
  exit 1
fi

echo "Building frontend assets (ui/dist)..."
npm --prefix ui run build

echo "Building standalone Tauri binary with embedded assets (custom-protocol)..."
cargo build --manifest-path src-tauri/Cargo.toml --release --features custom-protocol --offline --locked

BIN_PATH="$(pwd)/src-tauri/target/release/tako-desktop"
if [ ! -x "$BIN_PATH" ]; then
  echo "ERROR: Expected standalone binary not found at $BIN_PATH" >&2
  exit 1
fi

echo ""
echo "STANDALONE_BINARY=$BIN_PATH"
echo "Launch with:"
echo "  TAKO_PYTHON_CLI_BIN=/path/to/isolated/venv/bin/tako \"$BIN_PATH\""
