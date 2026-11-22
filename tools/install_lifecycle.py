"""Managed Linux CLI lifecycle commands (`tako update`, `tako uninstall`)."""

from __future__ import annotations

import argparse
import hashlib
import os
import pathlib
import platform
import shutil
import subprocess
import sys
import tempfile
import urllib.error
import urllib.request
import venv

DEFAULT_RELEASE_TAG = "v0.1.0a1"
DEFAULT_RELEASE_BASE_URL = "https://github.com/mmontielpz/tako/releases/download"


def default_tako_home() -> pathlib.Path:
    env_home = os.environ.get("TAKO_HOME", "").strip()
    if env_home:
        return pathlib.Path(env_home).expanduser()
    xdg = os.environ.get("XDG_DATA_HOME", "").strip()
    base = pathlib.Path(xdg).expanduser() if xdg else (pathlib.Path.home() / ".local" / "share")
    return base / "tako"


def default_bin_dir() -> pathlib.Path:
    env_bin = os.environ.get("TAKO_BIN_DIR", "").strip()
    if env_bin:
        return pathlib.Path(env_bin).expanduser()
    return pathlib.Path.home() / ".local" / "bin"


def _normalize_tag(ver: str) -> str:
    return ver if ver.startswith("v") else f"v{ver}"


def _normalize_pep440(ver: str) -> str:
    return ver[1:] if ver.startswith("v") else ver


def _read_manifest(manifest_path: pathlib.Path) -> dict[str, str]:
    data: dict[str, str] = {}
    for line in manifest_path.read_text(encoding="utf-8").splitlines():
        if "=" in line:
            k, v = line.split("=", 1)
            data[k.strip()] = v.strip()
    return data


def _fetch_to_file(url: str, dest: pathlib.Path) -> None:
    if url.startswith("file://"):
        src = pathlib.Path(url[len("file://") :])
        if not src.is_file():
            raise FileNotFoundError(f"Release file not found: {src}")
        shutil.copy2(src, dest)
        return
    allow_insecure = os.environ.get("TAKO_ALLOW_INSECURE_LOCALHOST", "0") == "1"
    if not url.startswith("https://") and not allow_insecure:
        raise ValueError(f"Refusing non-HTTPS remote URL: {url}")
    req = urllib.request.Request(url, headers={"User-Agent": "tako-cli-updater"})
    with urllib.request.urlopen(req, timeout=30) as resp:
        dest.write_bytes(resp.read())


def _expected_sha256(sums_text: str, wheel_name: str) -> str:
    for line in sums_text.splitlines():
        parts = line.strip().split()
        if len(parts) >= 2:
            filename = pathlib.Path(parts[-1]).name
            if filename == wheel_name:
                return parts[0].lower()
    return ""


def cmd_update(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(prog="tako update", description="Safely update a TAKO-managed Linux installation")
    parser.add_argument("--version", default=os.environ.get("TAKO_VERSION", DEFAULT_RELEASE_TAG), help="Target release version/tag")
    parser.add_argument("--release-base-url", default=os.environ.get("TAKO_RELEASE_BASE_URL", DEFAULT_RELEASE_BASE_URL), help="Release download base URL")
    parser.add_argument("--home", default=str(default_tako_home()), help="Managed TAKO_HOME directory")
    parser.add_argument("--bin-dir", default=str(default_bin_dir()), help="User bin directory for tako symlink")
    ns = parser.parse_args(argv)

    if platform.system() != "Linux":
        print(f"UPDATE_ERROR: tako update supports Linux-managed installs only (found {platform.system()}).", file=sys.stderr)
        return 5

    tako_home = pathlib.Path(ns.home).expanduser()
    bin_dir = pathlib.Path(ns.bin_dir).expanduser()
    manifest_path = tako_home / ".tako-install-manifest"
    venv_dir = tako_home / "venv"

    if not manifest_path.is_file() or not venv_dir.is_dir():
        print(
            f"UPDATE_ERROR: No TAKO-managed installation found at {tako_home} (missing .tako-install-manifest).",
            file=sys.stderr,
        )
        return 4

    manifest = _read_manifest(manifest_path)
    if manifest.get("managed_by") != "tako-install":
        print(f"UPDATE_ERROR: Refusing update of unmanaged installation at {tako_home}.", file=sys.stderr)
        return 4

    tag = _normalize_tag(ns.version)
    pep440 = _normalize_pep440(ns.version)
    wheel_name = f"tako_researcher_preview-{pep440}-py3-none-any.whl"
    base = ns.release_base_url.rstrip("/")

    with tempfile.TemporaryDirectory() as tmp:
        tmp_dir = pathlib.Path(tmp)
        sums_path = tmp_dir / "SHA256SUMS"
        wheel_path = tmp_dir / wheel_name
        try:
            _fetch_to_file(f"{base}/{tag}/SHA256SUMS", sums_path)
            _fetch_to_file(f"{base}/{tag}/{wheel_name}", wheel_path)
        except (urllib.error.URLError, FileNotFoundError, ValueError, OSError) as exc:
            print(f"UPDATE_ERROR: Failed to download release artifacts ({exc}). Existing installation preserved.", file=sys.stderr)
            return 6

        expected = _expected_sha256(sums_path.read_text(encoding="utf-8"), wheel_name)
        actual = hashlib.sha256(wheel_path.read_bytes()).hexdigest().lower()
        if not expected or expected != actual:
            print(
                f"UPDATE_ERROR: Wheel SHA-256 verification failed (expected={expected} actual={actual}). Existing installation preserved.",
                file=sys.stderr,
            )
            return 3

        if manifest.get("sha256", "").lower() == actual and (venv_dir / "bin" / "tako").exists():
            print(f'TAKO_UP_TO_DATE version="{pep440}" home="{tako_home}" sha256="{actual}"')
            return 0

        backup_venv = tako_home / f".venv.backup.{os.getpid()}"
        venv_dir.rename(backup_venv)
        try:
            venv.create(venv_dir, with_pip=True)
            venv_py = venv_dir / "bin" / "python"
            subprocess.run(
                [str(venv_py), "-m", "pip", "install", "--no-deps", "--no-index", "--quiet", str(wheel_path)],
                check=True,
                capture_output=True,
                text=True,
            )
            venv_tako = venv_dir / "bin" / "tako"
            subprocess.run([str(venv_tako), "--version"], check=True, capture_output=True, text=True)
            shutil.rmtree(backup_venv, ignore_errors=True)
        except Exception as exc:
            shutil.rmtree(venv_dir, ignore_errors=True)
            if backup_venv.exists():
                backup_venv.rename(venv_dir)
            print(f"UPDATE_ERROR: Upgrade failed ({exc}). Existing installation preserved.", file=sys.stderr)
            return 1

        bin_dir.mkdir(parents=True, exist_ok=True)
        bin_link = bin_dir / "tako"
        if bin_link.is_symlink() or not bin_link.exists():
            if bin_link.is_symlink():
                bin_link.unlink()
            bin_link.symlink_to(venv_dir / "bin" / "tako")

        py_ver = f"{sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}"
        manifest_path.write_text(
            f"managed_by=tako-install\nversion={pep440}\nwheel={wheel_name}\nsha256={actual}\npython={py_ver}\nbin_link={bin_link}\n",
            encoding="utf-8",
        )
        print(f'TAKO_UPDATE_COMPLETE version="{pep440}" home="{tako_home}" sha256="{actual}"')
        return 0


def cmd_uninstall(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(prog="tako uninstall", description="Remove TAKO-managed installation (preserves workspaces and config)")
    parser.add_argument("--home", default=str(default_tako_home()), help="Managed TAKO_HOME directory")
    parser.add_argument("--bin-dir", default=str(default_bin_dir()), help="User bin directory for tako symlink")
    ns = parser.parse_args(argv)

    tako_home = pathlib.Path(ns.home).expanduser()
    bin_dir = pathlib.Path(ns.bin_dir).expanduser()
    manifest_path = tako_home / ".tako-install-manifest"
    venv_dir = tako_home / "venv"
    bin_link = bin_dir / "tako"

    if not tako_home.exists() and not bin_link.is_symlink():
        print(f"UNINSTALL_INFO: No TAKO-managed installation found at {tako_home}")
        return 0

    if tako_home.exists() and not manifest_path.is_file():
        print(f"UNINSTALL_ERROR: Refusing to remove {tako_home} (missing .tako-install-manifest; not installer-owned).", file=sys.stderr)
        return 4

    if bin_link.is_symlink():
        try:
            target = os.readlink(bin_link)
        except OSError:
            target = ""
        if target.startswith(str(venv_dir) + "/"):
            bin_link.unlink()
        else:
            print(f"UNINSTALL_WARN: Preserving {bin_link} (points to {target}, not {venv_dir})", file=sys.stderr)
    elif bin_link.exists():
        print(f"UNINSTALL_WARN: Preserving non-symlink executable at {bin_link}", file=sys.stderr)

    shutil.rmtree(venv_dir, ignore_errors=True)
    if manifest_path.exists():
        manifest_path.unlink()
    try:
        tako_home.rmdir()
    except OSError:
        pass

    print(f'TAKO_UNINSTALL_COMPLETE removed="{tako_home}" (user config and research workspaces preserved)')
    return 0
