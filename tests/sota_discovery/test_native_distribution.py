"""Deterministic tests for universal Linux installer (`install.sh`) and `tako update`/`uninstall` (M028A)."""

from __future__ import annotations

import hashlib
import os
import pathlib
import re
import shutil
import subprocess
import tempfile
import unittest
import zipfile


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]


def _build_versioned_wheel(dest_dir: pathlib.Path, version: str) -> pathlib.Path:
    dest_dir.mkdir(parents=True, exist_ok=True)
    whl_path = dest_dir / f"tako_researcher_preview-{version}-py3-none-any.whl"
    dist_info = f"tako_researcher_preview-{version}.dist-info"
    with zipfile.ZipFile(whl_path, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        for py in sorted((REPO_ROOT / "tools").rglob("*.py")):
            rel = py.relative_to(REPO_ROOT).as_posix()
            if py.name == "cli.py":
                text = re.sub(r'__version__ = "[^"]+"', f'__version__ = "{version}"', py.read_text(encoding="utf-8"))
                zf.writestr(rel, text)
            else:
                zf.write(py, rel)
        fx = REPO_ROOT / "tools" / "sota_ai_native" / "example_fixture.json"
        if fx.is_file():
            zf.write(fx, fx.relative_to(REPO_ROOT).as_posix())
        zf.writestr(f"{dist_info}/METADATA", f"Metadata-Version: 2.1\nName: tako-researcher-preview\nVersion: {version}\n")
        zf.writestr(f"{dist_info}/WHEEL", "Wheel-Version: 1.0\nGenerator: tako-test\nRoot-Is-Purelib: true\nTag: py3-none-any\n")
        zf.writestr(f"{dist_info}/entry_points.txt", "[console_scripts]\ntako = tools.cli:main\n")
        zf.writestr(f"{dist_info}/RECORD", "")
    return whl_path


def _publish_local_release(releases_dir: pathlib.Path, version: str, bad_sum: bool = False) -> pathlib.Path:
    tag_dir = releases_dir / f"v{version}"
    whl = _build_versioned_wheel(tag_dir, version)
    digest = "0" * 64 if bad_sum else hashlib.sha256(whl.read_bytes()).hexdigest()
    (tag_dir / "SHA256SUMS").write_text(f"{digest}  {whl.name}\n", encoding="utf-8")
    return whl


class NativeDistributionTests(unittest.TestCase):
    def setUp(self):
        if os.name == "nt":
            self.skipTest("Linux native distribution tests run on POSIX")

    def test_full_lifecycle_upgrade_failure_recovery_and_shells(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            releases = root / "releases"
            _publish_local_release(releases, "0.1.0a1")
            _publish_local_release(releases, "0.1.0a2")
            _publish_local_release(releases / "bad", "0.1.0a3", bad_sum=True)

            home_dir = root / "tako_home"
            bin_dir = root / "local_bin"
            ws_dir = root / "research_ws"
            ws_dir.mkdir()
            (ws_dir / "tako_workspace.json").write_text('{"topic":"jetseg"}\n', encoding="utf-8")
            cfg_file = root / "user_config.json"
            cfg_file.write_text('{"profile":"free"}\n', encoding="utf-8")

            base_url = f"file://{releases}"
            env = os.environ.copy()
            env.pop("PYTHONPATH", None)
            env["TAKO_CONFIG_PATH"] = str(cfg_file)
            env["HOME"] = str(root)

            # 1. Fresh install + PATH instructions when bin_dir not on PATH
            res_install = subprocess.run(
                ["bash", str(REPO_ROOT / "install.sh"), "--version", "v0.1.0a1", "--home", str(home_dir), "--bin-dir", str(bin_dir), "--release-base-url", base_url],
                capture_output=True, text=True, env=env,
            )
            self.assertEqual(res_install.returncode, 0, msg=res_install.stderr)
            self.assertIn("TAKO_INSTALL_COMPLETE", res_install.stdout)
            self.assertIn("PATH_ACTION_REQUIRED", res_install.stdout)
            tako_bin = bin_dir / "tako"
            self.assertTrue(tako_bin.is_symlink())

            # 2. Idempotent repeated install
            res_repeat = subprocess.run(
                ["bash", str(REPO_ROOT / "install.sh"), "--version", "v0.1.0a1", "--home", str(home_dir), "--bin-dir", str(bin_dir), "--release-base-url", base_url],
                capture_output=True, text=True, env=env,
            )
            self.assertEqual(res_repeat.returncode, 0, msg=res_repeat.stderr)
            self.assertIn("TAKO_ALREADY_INSTALLED", res_repeat.stdout)

            # 3. Bash, Zsh, Fish invocation without venv activation
            env_with_bin = env.copy()
            env_with_bin["PATH"] = f"{bin_dir}:{env.get('PATH', '')}"
            for shell_name in ("bash", "zsh", "fish"):
                shell_path = shutil.which(shell_name)
                if shell_path:
                    sh_res = subprocess.run([shell_path, "-c", "tako --version"], capture_output=True, text=True, env=env_with_bin)
                    self.assertEqual(sh_res.returncode, 0, msg=f"{shell_name}: {sh_res.stderr}")
                    self.assertIn("0.1.0a1", sh_res.stdout)

            # 4. Failed upgrade (checksum mismatch) must preserve 0.1.0a1
            res_bad_up = subprocess.run(
                [str(tako_bin), "update", "--version", "v0.1.0a3", "--home", str(home_dir), "--bin-dir", str(bin_dir), "--release-base-url", f"file://{releases / 'bad'}"],
                capture_output=True, text=True, env=env,
            )
            self.assertEqual(res_bad_up.returncode, 3)
            self.assertIn("Existing installation preserved", res_bad_up.stderr)
            ver_still = subprocess.run([str(tako_bin), "--version"], capture_output=True, text=True, env=env)
            self.assertIn("0.1.0a1", ver_still.stdout)

            # 5. Successful upgrade to 0.1.0a2
            res_up = subprocess.run(
                [str(tako_bin), "update", "--version", "v0.1.0a2", "--home", str(home_dir), "--bin-dir", str(bin_dir), "--release-base-url", base_url],
                capture_output=True, text=True, env=env,
            )
            self.assertEqual(res_up.returncode, 0, msg=res_up.stderr)
            self.assertIn("TAKO_UPDATE_COMPLETE", res_up.stdout)
            ver_new = subprocess.run([str(tako_bin), "--version"], capture_output=True, text=True, env=env)
            self.assertIn("0.1.0a2", ver_new.stdout)

            # 6. Uninstall removes owned files only; preserves workspace and config
            res_un = subprocess.run(
                [str(tako_bin), "uninstall", "--home", str(home_dir), "--bin-dir", str(bin_dir)],
                capture_output=True, text=True, env=env,
            )
            self.assertEqual(res_un.returncode, 0, msg=res_un.stderr)
            self.assertFalse(home_dir.exists())
            self.assertFalse(tako_bin.exists())
            self.assertTrue((ws_dir / "tako_workspace.json").exists())
            self.assertTrue(cfg_file.exists())

    def test_error_guards_missing_prereqs_platform_network_and_unrelated_bin(self):
        if os.name == "nt":
            self.skipTest("POSIX only")
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            releases = root / "releases"
            _publish_local_release(releases, "0.1.0a1")
            home_dir = root / "tako_home"
            bin_dir = root / "local_bin"
            bin_dir.mkdir(parents=True)

            # 1. Existing unrelated executable must be refused (exit 4)
            unrelated = bin_dir / "tako"
            unrelated.write_text("#!/bin/sh\necho unrelated\n", encoding="utf-8")
            unrelated.chmod(0o755)
            res_unrel = subprocess.run(
                ["bash", str(REPO_ROOT / "install.sh"), "--version", "v0.1.0a1", "--home", str(home_dir), "--bin-dir", str(bin_dir), "--release-base-url", f"file://{releases}"],
                capture_output=True, text=True,
            )
            self.assertEqual(res_unrel.returncode, 4)
            self.assertIn("unrelated executable", res_unrel.stderr)
            self.assertIn("unrelated", unrelated.read_text(encoding="utf-8"))

            # 2. Unsupported OS (uname -> Darwin) -> exit 5
            fake_bin = root / "fake_bin"
            fake_bin.mkdir()
            fake_uname = fake_bin / "uname"
            fake_uname.write_text("#!/bin/sh\necho Darwin\n", encoding="utf-8")
            fake_uname.chmod(0o755)
            env_os = os.environ.copy()
            env_os["PATH"] = f"{fake_bin}:{env_os.get('PATH', '')}"
            res_os = subprocess.run(["bash", str(REPO_ROOT / "install.sh")], capture_output=True, text=True, env=env_os)
            self.assertEqual(res_os.returncode, 5)
            self.assertIn("Unsupported operating system", res_os.stderr)

            # 3. Missing python3 -> exit 2
            empty_bin = root / "empty_bin"
            empty_bin.mkdir()
            for tool in ("uname", "mktemp", "rm", "tr", "sed", "awk", "grep", "dirname", "basename"):
                p = shutil.which(tool)
                if p:
                    (empty_bin / tool).symlink_to(p)
            env_nopy = {"PATH": str(empty_bin), "HOME": str(root)}
            res_nopy = subprocess.run(["/bin/bash", str(REPO_ROOT / "install.sh")], capture_output=True, text=True, env=env_nopy)
            self.assertEqual(res_nopy.returncode, 2)
            self.assertIn("python3", res_nopy.stderr)

            # 4. Network / missing remote artifact & non-HTTPS refusal -> exit 6
            res_net = subprocess.run(
                ["bash", str(REPO_ROOT / "install.sh"), "--version", "v9.9.9", "--home", str(home_dir), "--bin-dir", str(root / "clean_bin"), "--release-base-url", f"file://{releases}"],
                capture_output=True, text=True,
            )
            self.assertEqual(res_net.returncode, 6)
            res_http = subprocess.run(
                ["bash", str(REPO_ROOT / "install.sh"), "--release-base-url", "http://example.com/releases"],
                capture_output=True, text=True,
            )
            self.assertEqual(res_http.returncode, 6)


if __name__ == "__main__":
    unittest.main()
