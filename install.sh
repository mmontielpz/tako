#!/usr/bin/env bash
set -euo pipefail

# Universal Linux bootstrap installer for TAKO Experimental CLI.
# Supports CachyOS and Ubuntu 24.04 LTS (x86_64/arm64) without sudo,
# automatic system package installation, or silent shell profile edits.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/mmontielpz/tako/main/install.sh | bash
#   bash install.sh --version v0.1.0a1
#
# Security & Trust Model:
#   Downloads release assets and SHA256SUMS over HTTPS from GitHub Releases.
#   Same-origin SHA256SUMS verifies download integrity against the release host,
#   NOT independent cryptographic publisher authentication (GPG/Sigstore).

DEFAULT_RELEASE_TAG="v0.1.0a1"
DEFAULT_BASE_URL="https://github.com/mmontielpz/tako/releases/download"

TAKO_HOME="${TAKO_HOME:-${XDG_DATA_HOME:-$HOME/.local/share}/tako}"
TAKO_BIN_DIR="${TAKO_BIN_DIR:-$HOME/.local/bin}"
REQUESTED_VERSION="${TAKO_VERSION:-$DEFAULT_RELEASE_TAG}"
RELEASE_BASE_URL="${TAKO_RELEASE_BASE_URL:-$DEFAULT_BASE_URL}"
LOCAL_WHEEL=""
LOCAL_CHECKSUMS=""
ACTION="install"

usage() {
  cat << 'EOF'
TAKO Linux Bootstrap Installer

Options:
  --version <ver>           Release version/tag to install (e.g. v0.1.0a1 or 0.1.0a1)
  --home <dir>              Managed installation directory (default: ~/.local/share/tako)
  --bin-dir <dir>           User bin directory for tako symlink (default: ~/.local/bin)
  --release-base-url <url>  Override release download base URL (HTTPS or file://)
  --wheel <path>            Install from a local wheel file (requires --checksums)
  --checksums <path>        Local SHA256SUMS file for --wheel
  --uninstall               Remove TAKO-managed installation only
  -h, --help                Show this help
EOF
}

while [ $# -gt 0 ]; do
  case "$1" in
    --version)
      [ $# -ge 2 ] || { echo "INSTALL_ERROR: --version requires an argument" >&2; exit 2; }
      REQUESTED_VERSION="$2"; shift 2 ;;
    --home)
      [ $# -ge 2 ] || { echo "INSTALL_ERROR: --home requires an argument" >&2; exit 2; }
      TAKO_HOME="$2"; shift 2 ;;
    --bin-dir)
      [ $# -ge 2 ] || { echo "INSTALL_ERROR: --bin-dir requires an argument" >&2; exit 2; }
      TAKO_BIN_DIR="$2"; shift 2 ;;
    --release-base-url)
      [ $# -ge 2 ] || { echo "INSTALL_ERROR: --release-base-url requires an argument" >&2; exit 2; }
      RELEASE_BASE_URL="$2"; shift 2 ;;
    --wheel)
      [ $# -ge 2 ] || { echo "INSTALL_ERROR: --wheel requires an argument" >&2; exit 2; }
      LOCAL_WHEEL="$2"; shift 2 ;;
    --checksums)
      [ $# -ge 2 ] || { echo "INSTALL_ERROR: --checksums requires an argument" >&2; exit 2; }
      LOCAL_CHECKSUMS="$2"; shift 2 ;;
    --uninstall)
      ACTION="uninstall"; shift ;;
    -h|--help)
      usage; exit 0 ;;
    *)
      echo "INSTALL_ERROR: Unknown option: $1" >&2
      exit 2 ;;
  esac
done

normalize_tag() {
  local raw="$1"
  if [[ "$raw" == v* ]]; then
    printf "%s" "$raw"
  else
    printf "v%s" "$raw"
  fi
}

normalize_pep440() {
  local raw="$1"
  printf "%s" "${raw#v}"
}

uninstall_tako() {
  local venv_dir="$TAKO_HOME/venv"
  local manifest="$TAKO_HOME/.tako-install-manifest"
  local bin_link="$TAKO_BIN_DIR/tako"

  if [ ! -e "$TAKO_HOME" ] && [ ! -L "$bin_link" ]; then
    echo "UNINSTALL_INFO: No TAKO installation found at $TAKO_HOME"
    exit 0
  fi

  if [ -e "$TAKO_HOME" ] && [ ! -f "$manifest" ]; then
    echo "UNINSTALL_ERROR: Refusing to remove $TAKO_HOME (missing .tako-install-manifest; not installer-owned)." >&2
    exit 4
  fi

  if [ -L "$bin_link" ]; then
    local target
    target="$(readlink "$bin_link" || true)"
    if [[ "$target" == "$venv_dir/"* ]]; then
      rm -f "$bin_link"
    else
      echo "UNINSTALL_WARN: Preserving $bin_link (points to $target, not $venv_dir)" >&2
    fi
  elif [ -e "$bin_link" ]; then
    echo "UNINSTALL_WARN: Preserving non-symlink executable at $bin_link" >&2
  fi

  if [ -f "$manifest" ]; then
    rm -rf "$venv_dir" "$manifest"
    rmdir "$TAKO_HOME" 2>/dev/null || true
  fi

  echo "TAKO_UNINSTALL_COMPLETE removed=\"$TAKO_HOME\" (user config and research workspaces preserved)"
  exit 0
}

if [ "$ACTION" = "uninstall" ]; then
  uninstall_tako
fi

# 1. Platform check (Linux only; CachyOS and Ubuntu 24.04 LTS primary targets)
OS_NAME="$(uname -s)"
if [ "$OS_NAME" != "Linux" ]; then
  echo "INSTALL_ERROR: Unsupported operating system '$OS_NAME'. This installer supports Linux (CachyOS, Ubuntu 24.04 LTS)." >&2
  exit 5
fi

DISTRO_ID="linux"
DISTRO_VER=""
if [ -f /etc/os-release ]; then
  # shellcheck disable=SC1091
  DISTRO_ID="$(. /etc/os-release && printf "%s" "${ID:-linux}")"
  DISTRO_VER="$(. /etc/os-release && printf "%s" "${VERSION_ID:-}")"
fi

# 2. Prerequisite checks (no sudo, no automatic package installation)
if ! command -v python3 >/dev/null 2>&1; then
  echo "INSTALL_ERROR: python3 (>= 3.9) is required and was not found on PATH. Install python3 with your distro package manager first." >&2
  exit 2
fi

PY_VER="$(python3 -c 'import sys; print(f"{sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}")')"
PY_OK="$(python3 -c 'import sys; print("1" if sys.version_info >= (3, 9) else "0")')"
if [ "$PY_OK" != "1" ]; then
  echo "INSTALL_ERROR: Python >= 3.9 is required (found $PY_VER)." >&2
  exit 2
fi

if ! python3 -m venv --help >/dev/null 2>&1; then
  echo "INSTALL_ERROR: python3-venv is required (e.g. 'sudo apt install python3-venv' on Ubuntu 24.04 LTS)." >&2
  exit 2
fi

if ! command -v sha256sum >/dev/null 2>&1; then
  echo "INSTALL_ERROR: sha256sum is required to verify artifact integrity." >&2
  exit 2
fi

TMP_DIR="$(mktemp -d)"
STAGING_VENV=""
cleanup() {
  rm -rf "$TMP_DIR"
  if [ -n "$STAGING_VENV" ] && [ -d "$STAGING_VENV" ]; then
    rm -rf "$STAGING_VENV"
  fi
}
trap cleanup EXIT

TAG="$(normalize_tag "$REQUESTED_VERSION")"
PEP440_VER="$(normalize_pep440 "$REQUESTED_VERSION")"
WHEEL_NAME="tako_researcher_preview-${PEP440_VER}-py3-none-any.whl"
WHEEL_FILE="$TMP_DIR/$WHEEL_NAME"
SUMS_FILE="$TMP_DIR/SHA256SUMS"

fetch_file() {
  local url="$1"
  local dest="$2"
  if [[ "$url" == file://* ]]; then
    local src="${url#file://}"
    [ -f "$src" ] || return 1
    cp "$src" "$dest"
    return 0
  fi
  if [[ "$url" != https://* ]] && [ "${TAKO_ALLOW_INSECURE_LOCALHOST:-0}" != "1" ]; then
    echo "INSTALL_ERROR: Refusing non-HTTPS remote URL '$url'." >&2
    return 6
  fi
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$dest"
  elif command -v wget >/dev/null 2>&1; then
    wget -qO "$dest" "$url"
  else
    echo "INSTALL_ERROR: curl or wget is required to download release artifacts." >&2
    return 2
  fi
}

# 3. Acquire wheel & SHA256SUMS (local fixture or HTTPS release)
if [ -n "$LOCAL_WHEEL" ] || [ -n "$LOCAL_CHECKSUMS" ]; then
  if [ -z "$LOCAL_WHEEL" ] || [ -z "$LOCAL_CHECKSUMS" ]; then
    echo "INSTALL_ERROR: --wheel and --checksums must be provided together." >&2
    exit 2
  fi
  [ -f "$LOCAL_WHEEL" ] || { echo "INSTALL_ERROR: Wheel not found at $LOCAL_WHEEL" >&2; exit 2; }
  [ -f "$LOCAL_CHECKSUMS" ] || { echo "INSTALL_ERROR: Checksums file not found at $LOCAL_CHECKSUMS" >&2; exit 2; }
  WHEEL_NAME="$(basename "$LOCAL_WHEEL")"
  WHEEL_FILE="$TMP_DIR/$WHEEL_NAME"
  cp "$LOCAL_WHEEL" "$WHEEL_FILE"
  cp "$LOCAL_CHECKSUMS" "$SUMS_FILE"
else
  BASE="${RELEASE_BASE_URL%/}"
  if ! fetch_file "$BASE/$TAG/SHA256SUMS" "$SUMS_FILE"; then
    echo "INSTALL_ERROR: Failed to download SHA256SUMS from $BASE/$TAG/SHA256SUMS" >&2
    exit 6
  fi
  if ! fetch_file "$BASE/$TAG/$WHEEL_NAME" "$WHEEL_FILE"; then
    echo "INSTALL_ERROR: Failed to download $WHEEL_NAME from $BASE/$TAG/$WHEEL_NAME" >&2
    exit 6
  fi
fi

# 4. Verify SHA-256 integrity before touching TAKO_HOME
EXPECTED_SUM="$(grep -E "(^|[[:space:]]|/)($(printf '%s' "$WHEEL_NAME" | sed 's/[.[\*^$()+?{|]/\\&/g'))$" "$SUMS_FILE" | head -n 1 | awk '{print $1}' || true)"
ACTUAL_SUM="$(sha256sum "$WHEEL_FILE" | awk '{print $1}')"
if [ -z "$EXPECTED_SUM" ] || [ "$EXPECTED_SUM" != "$ACTUAL_SUM" ]; then
  echo "INSTALL_ERROR: Wheel SHA-256 verification failed (expected=$EXPECTED_SUM actual=$ACTUAL_SUM)." >&2
  exit 3
fi

# 5. Check ownership of existing TAKO_HOME and target executable in TAKO_BIN_DIR
VENV_DIR="$TAKO_HOME/venv"
MANIFEST="$TAKO_HOME/.tako-install-manifest"
BIN_LINK="$TAKO_BIN_DIR/tako"

if [ -e "$VENV_DIR" ] && [ ! -f "$MANIFEST" ]; then
  echo "INSTALL_ERROR: Existing environment at $VENV_DIR lacks .tako-install-manifest. Refusing unsafe overwrite." >&2
  exit 4
fi

if [ -e "$BIN_LINK" ] || [ -L "$BIN_LINK" ]; then
  if [ -L "$BIN_LINK" ]; then
    EXISTING_TARGET="$(readlink "$BIN_LINK" || true)"
    if [[ "$EXISTING_TARGET" != "$VENV_DIR/"* ]]; then
      echo "INSTALL_ERROR: Refusing to overwrite unrelated symlink at $BIN_LINK (-> $EXISTING_TARGET)." >&2
      exit 4
    fi
  else
    echo "INSTALL_ERROR: Refusing to overwrite unrelated executable at $BIN_LINK." >&2
    exit 4
  fi
fi

# Idempotent fast path if exact same wheel SHA-256 & version are already installed
if [ -f "$MANIFEST" ] && [ -x "$VENV_DIR/bin/tako" ]; then
  INSTALLED_SUM="$(grep '^sha256=' "$MANIFEST" | head -n 1 | cut -d= -f2- || true)"
  if [ "$INSTALLED_SUM" = "$ACTUAL_SUM" ]; then
    mkdir -p "$TAKO_BIN_DIR"
    ln -sfn "$VENV_DIR/bin/tako" "$BIN_LINK"
    echo "TAKO_ALREADY_INSTALLED version=\"$PEP440_VER\" home=\"$TAKO_HOME\" bin=\"$BIN_LINK\" sha256=\"$ACTUAL_SUM\""
    exit 0
  fi
fi

# 6. Rollback-safe installation / upgrade at $VENV_DIR
# (Download, checksum verification, and python3 -m venv check already succeeded above.)
mkdir -p "$TAKO_HOME" "$TAKO_BIN_DIR"
BACKUP_VENV=""
if [ -e "$VENV_DIR" ]; then
  BACKUP_VENV="$TAKO_HOME/.venv.backup.$$"
  mv "$VENV_DIR" "$BACKUP_VENV"
fi

restore_backup() {
  rm -rf "$VENV_DIR"
  if [ -n "$BACKUP_VENV" ] && [ -e "$BACKUP_VENV" ]; then
    mv "$BACKUP_VENV" "$VENV_DIR" || true
  fi
}

if ! python3 -m venv "$VENV_DIR" \
  || ! "$VENV_DIR/bin/python" -m pip install --no-deps --no-index --quiet "$WHEEL_FILE" \
  || ! "$VENV_DIR/bin/tako" --version >/dev/null 2>&1; then
  restore_backup
  echo "INSTALL_ERROR: Failed to install $WHEEL_NAME into $VENV_DIR. Previous installation restored if present." >&2
  exit 1
fi
[ -n "$BACKUP_VENV" ] && rm -rf "$BACKUP_VENV"

printf "managed_by=tako-install\nversion=%s\nwheel=%s\nsha256=%s\npython=%s\ndistro=%s:%s\nbin_link=%s\n" \
  "$PEP440_VER" "$WHEEL_NAME" "$ACTUAL_SUM" "$PY_VER" "$DISTRO_ID" "$DISTRO_VER" "$BIN_LINK" > "$MANIFEST"
ln -sfn "$VENV_DIR/bin/tako" "$BIN_LINK"

echo "TAKO_INSTALL_COMPLETE version=\"$PEP440_VER\" home=\"$TAKO_HOME\" bin=\"$BIN_LINK\" python=\"$PY_VER\" distro=\"$DISTRO_ID\""

case ":$PATH:" in
  *":$TAKO_BIN_DIR:"*)
    echo "PATH_READY: $TAKO_BIN_DIR is already on PATH." ;;
  *)
    echo "PATH_ACTION_REQUIRED: $TAKO_BIN_DIR is not on your PATH. Add it to your shell:"
    echo "  Bash: echo 'export PATH=\"$TAKO_BIN_DIR:\$PATH\"' >> ~/.bashrc && source ~/.bashrc"
    echo "  Zsh:  echo 'export PATH=\"$TAKO_BIN_DIR:\$PATH\"' >> ~/.zshrc && source ~/.zshrc"
    echo "  Fish: fish_add_path \"$TAKO_BIN_DIR\"" ;;
esac
