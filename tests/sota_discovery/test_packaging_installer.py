"""Packaging and native Linux installer/uninstaller regression tests (M026)."""

from __future__ import annotations

import hashlib
import pathlib
import shutil
import subprocess
import tempfile
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]


class PackagingInstallerTests(unittest.TestCase):
    def _prepare_kit(self, kit_dir: pathlib.Path, wheel_path: pathlib.Path, bad_sum: bool = False) -> None:
        pkg_dir = kit_dir / "packages"
        pkg_dir.mkdir(parents=True, exist_ok=True)
        dest_whl = pkg_dir / wheel_path.name
        shutil.copy2(wheel_path, dest_whl)
        digest = hashlib.sha256(dest_whl.read_bytes()).hexdigest()
        if bad_sum:
            digest = "0" * 64
        (kit_dir / "SHA256SUMS").write_text(f"{digest}  packages/{wheel_path.name}\n", encoding="utf-8")
        shutil.copy2(REPO_ROOT / "scripts" / "install-linux.sh", kit_dir / "install-linux.sh")
        shutil.copy2(REPO_ROOT / "scripts" / "uninstall-linux.sh", kit_dir / "uninstall-linux.sh")

    def test_installer_checksum_mismatch_unsafe_overwrite_and_uninstall_safety(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp_p = pathlib.Path(tmp)
            dist_dir = tmp_p / "dist"
            build = subprocess.run(
                ["python3", "-m", "pip", "wheel", "--no-deps", "--no-build-isolation", "-w", str(dist_dir), str(REPO_ROOT)],
                capture_output=True,
                text=True,
                timeout=60,
            )
            self.assertEqual(build.returncode, 0, msg=build.stderr)
            whl = next(dist_dir.glob("tako_researcher_preview-0.1.0a1-*.whl"))

            # 1. Checksum mismatch must fail before creating venv
            bad_kit = tmp_p / "bad_kit"
            self._prepare_kit(bad_kit, whl, bad_sum=True)
            res_bad = subprocess.run(["bash", str(bad_kit / "install-linux.sh")], capture_output=True, text=True)
            self.assertEqual(res_bad.returncode, 3)
            self.assertIn("SHA-256 verification failed", res_bad.stderr)
            self.assertFalse((bad_kit / ".tako-venv").exists())

            # 2. Clean install in path with spaces and Unicode
            good_kit = tmp_p / "kit α space"
            self._prepare_kit(good_kit, whl, bad_sum=False)
            res_ok = subprocess.run(["bash", str(good_kit / "install-linux.sh")], capture_output=True, text=True)
            self.assertEqual(res_ok.returncode, 0, msg=res_ok.stderr)
            self.assertIn("TAKO_INSTALL_COMPLETE", res_ok.stdout)
            tako_bin = good_kit / ".tako-venv" / "bin" / "tako"
            ver = subprocess.run([str(tako_bin), "--version"], capture_output=True, text=True)
            self.assertEqual(ver.returncode, 0)
            self.assertIn("0.1.0a1", ver.stdout)

            # 3. Unsafe overwrite without --reinstall must fail
            res_dup = subprocess.run(["bash", str(good_kit / "install-linux.sh")], capture_output=True, text=True)
            self.assertEqual(res_dup.returncode, 4)
            self.assertIn("Refusing unsafe overwrite", res_dup.stderr)

            # 4. Uninstall removes .tako-venv while preserving user workspace; refuses non-installer dirs
            user_ws = good_kit / "my_research"
            user_ws.mkdir()
            (user_ws / "notes.txt").write_text("keep me", encoding="utf-8")
            res_uninst = subprocess.run(["bash", str(good_kit / "uninstall-linux.sh")], capture_output=True, text=True)
            self.assertEqual(res_uninst.returncode, 0, msg=res_uninst.stderr)
            self.assertFalse((good_kit / ".tako-venv").exists())
            self.assertTrue((user_ws / "notes.txt").exists())

            res_refuse = subprocess.run(["bash", str(good_kit / "uninstall-linux.sh"), str(user_ws)], capture_output=True, text=True)
            self.assertEqual(res_refuse.returncode, 4)
            self.assertTrue((user_ws / "notes.txt").exists())


if __name__ == "__main__":
    unittest.main()
