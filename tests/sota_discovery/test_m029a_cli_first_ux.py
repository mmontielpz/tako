"""Contract & UX regression tests for M029A (F-001 through F-005)."""

from __future__ import annotations

import contextlib
import io
import json
import os
import pathlib
import subprocess
import tempfile
import unittest
from unittest import mock

from tools.ai_cli_providers import (
    authorize_provider_execution,
    build_cli_invocation,
    detect_ai_cli_providers,
    sanitized_cli_env,
)
from tools.cli import main as cli_main
from tools.sota_ai_native.workflow import ProviderBlockedError, run_role


class M029ACliFirstUxTests(unittest.TestCase):
    def test_provider_detection_and_authorization_separation(self):
        with mock.patch("tools.ai_cli_providers.shutil.which", side_effect=lambda x: f"/usr/bin/{x}" if x in ("agy", "codex", "claude", "copilot") else None):
            items = {i["id"]: i for i in detect_ai_cli_providers()}
            self.assertTrue(items["agy-cli"]["executable_detected"])
            self.assertTrue(items["codex-cli"]["adapter_implemented"])
            self.assertEqual(items["agy-cli"]["authentication_status"], "UNVERIFIED_SESSION_DEPENDENT")
            self.assertFalse(items["gemini-cli"]["adapter_implemented"])
            self.assertEqual(items["gemini-cli"]["live_inference_status"], "UNSUPPORTED")

        # Missing auth fails closed
        ok, msg = authorize_provider_execution("claude-cli", allow_cli_session=False, allow_paid_api=False)
        self.assertFalse(ok)
        self.assertIn("--allow-cli-session", msg)

        # CLI session auth works for subscription CLIs, NEVER for metered API
        ok_cli, cat_cli = authorize_provider_execution("agy-cli", allow_cli_session=True, allow_paid_api=False)
        self.assertTrue(ok_cli)
        self.assertEqual(cat_cli, "subscription_cli")

        ok_api, msg_api = authorize_provider_execution("openai-responses", allow_cli_session=True, allow_paid_api=False)
        self.assertFalse(ok_api)
        self.assertIn("--allow-paid-api", msg_api)

        # Unsupported provider
        ok_bad, _ = authorize_provider_execution("gemini-cli", allow_cli_session=True, allow_paid_api=True)
        self.assertFalse(ok_bad)

    def test_invocation_contracts_redaction_and_subprocess_failure_handling(self):
        self.assertEqual(build_cli_invocation("agy-cli", "/bin/agy", "hi"), ["/bin/agy", "-p", "hi", "--output-format", "json"])
        self.assertEqual(build_cli_invocation("codex-cli", "/bin/codex", "hi"), ["/bin/codex", "exec", "hi"])
        self.assertEqual(build_cli_invocation("claude-cli", "/bin/claude", "hi"), ["/bin/claude", "-p", "hi", "--output-format", "json"])

        with mock.patch.dict(os.environ, {"OPENAI_API_KEY": "sk-secret", "ANTHROPIC_API_KEY": "ant-secret", "GH_TOKEN": "gh-secret"}):
            env = sanitized_cli_env()
            self.assertNotIn("OPENAI_API_KEY", env)
            self.assertNotIn("ANTHROPIC_API_KEY", env)
            self.assertNotIn("GH_TOKEN", env)

        valid_payload = json.dumps({"result": json.dumps({"related_topics": ["edge"], "papers": []})})
        with mock.patch("tools.sota_ai_native.workflow.shutil.which", return_value="/usr/bin/agy"):
            with mock.patch("tools.sota_ai_native.workflow._run_bounded_subprocess", return_value=(0, valid_payload, "")):
                out, usage = run_role("researcher", "prompt", "agy-cli", "default", False, None, allow_cli_session=True)
                self.assertEqual(out["related_topics"], ["edge"])
                self.assertEqual(usage["provenance"], "agy-cli-json")

            with mock.patch("tools.sota_ai_native.workflow._run_bounded_subprocess", return_value=(1, "", "session expired")):
                with self.assertRaises(ProviderBlockedError):
                    run_role("researcher", "prompt", "agy-cli", "default", False, None, allow_cli_session=True)

            with mock.patch("tools.sota_ai_native.workflow.subprocess.run", side_effect=subprocess.TimeoutExpired(cmd=["agy"], timeout=1)):
                with self.assertRaises(ProviderBlockedError):
                    run_role("researcher", "prompt", "agy-cli", "default", False, None, allow_cli_session=True)

    def test_setup_profile_no_menu_and_config_preservation(self):
        with tempfile.TemporaryDirectory() as tmp:
            cfg = pathlib.Path(tmp) / "config.json"
            with mock.patch.dict(os.environ, {"TAKO_CONFIG_PATH": str(cfg)}):
                with mock.patch("builtins.input", side_effect=["", "2"]):
                    buf = io.StringIO()
                    with contextlib.redirect_stdout(buf):
                        rc = cli_main(["setup", "--profile", "free"])
                    self.assertEqual(rc, 0)
                    out = buf.getvalue()
                    self.assertNotIn("Profiles: 1=free", out)
                    saved = json.loads(cfg.read_text(encoding="utf-8"))
                    self.assertEqual(saved["profile"], "free")
                    self.assertEqual(saved["assistant"], "codex")

                # Preserves existing config if overwrite declined
                with mock.patch("builtins.input", side_effect=["", "1", "n"]):
                    buf2 = io.StringIO()
                    with contextlib.redirect_stdout(buf2):
                        rc2 = cli_main(["setup", "--profile", "hybrid"])
                    self.assertEqual(rc2, 0)
                    self.assertIn("SETUP_CANCELLED", buf2.getvalue())
                    self.assertEqual(json.loads(cfg.read_text(encoding="utf-8"))["profile"], "free")

    def test_typo_suggestions_concise_and_verbose_doctor_and_help(self):
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            rc = cli_main(["--verison"])
        self.assertEqual(rc, 2)
        self.assertIn("Did you mean '--version'?", err.getvalue())

        err2 = io.StringIO()
        with contextlib.redirect_stderr(err2):
            rc2 = cli_main(["doctr"])
        self.assertEqual(rc2, 2)
        self.assertIn("Did you mean 'doctor'?", err2.getvalue())

        with mock.patch.dict(os.environ, {"OPENAI_API_KEY": "sk-do-not-leak"}):
            out_short = io.StringIO()
            with contextlib.redirect_stdout(out_short):
                self.assertEqual(cli_main(["doctor"]), 0)
            lines = out_short.getvalue().strip().splitlines()
            self.assertLessEqual(len(lines), 12)
            self.assertNotIn("sk-do-not-leak", out_short.getvalue())

            out_verb = io.StringIO()
            with contextlib.redirect_stdout(out_verb):
                self.assertEqual(cli_main(["doctor", "--verbose"]), 0)
            self.assertIn("agy-cli", out_verb.getvalue())
            self.assertIn("codex-cli", out_verb.getvalue())
            self.assertNotIn("sk-do-not-leak", out_verb.getvalue())

            out_json = io.StringIO()
            with contextlib.redirect_stdout(out_json):
                self.assertEqual(cli_main(["doctor", "--json"]), 0)
            parsed = json.loads(out_json.getvalue())
            self.assertIn("ai_cli_providers", parsed)
            self.assertNotIn("sk-do-not-leak", out_json.getvalue())

        # Updated help
        help_sota = io.StringIO()
        with contextlib.redirect_stdout(help_sota):
            with self.assertRaises(SystemExit) as ctx:
                cli_main(["sota", "--help"])
            self.assertEqual(ctx.exception.code, 0)
        self.assertIn("Multi-Source", help_sota.getvalue())
        self.assertIn("Examples:", help_sota.getvalue())


if __name__ == "__main__":
    unittest.main()
