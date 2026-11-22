#!/usr/bin/env bash
set -euo pipefail

# Minimal local Linux uninstaller for TAKO Experimental CLI.
# Removes ONLY the installer-owned .tako-venv; never touches user config or research workspaces.

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
VENV_DIR="${1:-$SCRIPT_DIR/.tako-venv}"

if [ ! -e "$VENV_DIR" ]; then
  echo "UNINSTALL_INFO: Nothing to remove at $VENV_DIR"
  exit 0
fi

if [ ! -f "$VENV_DIR/.tako-install-manifest" ]; then
  echo "UNINSTALL_ERROR: Refusing to remove $VENV_DIR (missing .tako-install-manifest; not installer-owned)." >&2
  exit 4
fi

rm -rf "$VENV_DIR"
echo "TAKO_UNINSTALL_COMPLETE removed=\"$VENV_DIR\" (user config and research workspaces preserved)"
