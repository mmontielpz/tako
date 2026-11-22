#!/usr/bin/env bash
set -euo pipefail

# Minimal local Linux installer for TAKO Experimental CLI (CachyOS-qualified).
# No sudo, no pacman, no global pip, no system Python or shell profile changes.

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
WHEEL_NAME="tako_researcher_preview-0.1.0a1-py3-none-any.whl"
WHEEL_PATH="$SCRIPT_DIR/packages/$WHEEL_NAME"
SUMS_PATH="$SCRIPT_DIR/SHA256SUMS"
VENV_DIR="${1:-$SCRIPT_DIR/.tako-venv}"
REINSTALL="${2:-}"

if [ ! -f "$WHEEL_PATH" ]; then
  echo "INSTALL_ERROR: Bundled wheel not found at $WHEEL_PATH" >&2
  exit 2
fi
if [ ! -f "$SUMS_PATH" ]; then
  echo "INSTALL_ERROR: Checksum manifest not found at $SUMS_PATH" >&2
  exit 2
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "INSTALL_ERROR: python3 is required (>= 3.9) and was not found on PATH." >&2
  exit 2
fi

PY_VER="$(python3 -c 'import sys; print(f"{sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}")')"
PY_OK="$(python3 -c 'import sys; print("1" if sys.version_info >= (3, 9) else "0")')"
if [ "$PY_OK" != "1" ]; then
  echo "INSTALL_ERROR: Python >= 3.9 is required (found $PY_VER)." >&2
  exit 2
fi

if command -v sha256sum >/dev/null 2>&1; then
  EXPECTED_SUM="$(grep -E "packages/${WHEEL_NAME}$|${WHEEL_NAME}$" "$SUMS_PATH" | head -n 1 | awk '{print $1}')"
  ACTUAL_SUM="$(sha256sum "$WHEEL_PATH" | awk '{print $1}')"
  if [ -z "$EXPECTED_SUM" ] || [ "$EXPECTED_SUM" != "$ACTUAL_SUM" ]; then
    echo "INSTALL_ERROR: Wheel SHA-256 verification failed (expected=$EXPECTED_SUM actual=$ACTUAL_SUM)." >&2
    exit 3
  fi
else
  echo "INSTALL_ERROR: sha256sum is required to verify wheel integrity." >&2
  exit 2
fi

if [ -e "$VENV_DIR" ]; then
  if [ "$REINSTALL" = "--reinstall" ] && [ -f "$VENV_DIR/.tako-install-manifest" ]; then
    rm -rf "$VENV_DIR"
  else
    echo "INSTALL_ERROR: Target environment already exists at $VENV_DIR. Refusing unsafe overwrite (remove it first or pass --reinstall on an installer-owned venv)." >&2
    exit 4
  fi
fi

python3 -m venv "$VENV_DIR"
"$VENV_DIR/bin/python" -m pip install --no-deps --no-index --quiet "$WHEEL_PATH"
printf "installed_wheel=%s\nsha256=%s\npython=%s\n" "$WHEEL_NAME" "$ACTUAL_SUM" "$PY_VER" > "$VENV_DIR/.tako-install-manifest"

echo "TAKO_INSTALL_COMPLETE venv=\"$VENV_DIR\" python=\"$PY_VER\" version=\"0.1.0a1\""
echo "Next commands:"
echo "  source \"$VENV_DIR/bin/activate\""
echo "  tako --version"
echo "  tako setup --profile free"
echo "  tako doctor"
