"""M029D4 risk-based hardening regression tests (security, budgets, normalization, integrity, telemetry)."""

import io
import json
import os
import pathlib
import tempfile
import unittest
from unittest import mock

from tools.ai_cli_providers import (
    BudgetExhaustedError,
    ProviderCallBudget,
    authorize_provider_execution,
    build_cli_invocation,
    sanitized_cli_env,
)
from tools.sota_ai_native import workflow


class TestM029D4SecurityAndBudgets(unittest.TestCase):
    def test_sanitized_cli_env_strips_all_known_api_keys(self) -> None:
        dirty = {
            "PATH": "/usr/bin",
            "HOME": "/home/tester",
            "OPENAI_API_KEY": "sk-test",
            "ANTHROPIC_API_KEY": "ant-test",
            "GH_TOKEN": "ghp_test",
            "GITHUB_TOKEN": "gho_test",
            "GEMINI_API_KEY": "gem-test",
            "GOOGLE_API_KEY": "goog-test",
            "AZURE_OPENAI_API_KEY": "az-test",
            "UNRELATED_SECRET": "secret-val",
        }
        with mock.patch.dict(os.environ, dirty, clear=True):
            clean = sanitized_cli_env()
            self.assertEqual(clean.get("PATH"), "/usr/bin")
            self.assertEqual(clean.get("HOME"), "/home/tester")
            for k in (
                "OPENAI_API_KEY",
                "ANTHROPIC_API_KEY",
                "GH_TOKEN",
                "GITHUB_TOKEN",
                "GEMINI_API_KEY",
                "GOOGLE_API_KEY",
                "AZURE_OPENAI_API_KEY",
                "UNRELATED_SECRET",
            ):
                self.assertNotIn(k, clean)

    def test_provider_call_budget_validation_and_exhaustion(self) -> None:
        with self.assertRaises(ValueError):
            ProviderCallBudget(max_calls=-1)

        b0 = ProviderCallBudget(max_calls=0)
        with self.assertRaises(BudgetExhaustedError):
            b0.consume("researcher")
        self.assertEqual(b0.attempted_calls, 0)

        b1 = ProviderCallBudget(max_calls=1)
        b1.consume("researcher")
        b1.record_success()
        with self.assertRaises(BudgetExhaustedError):
            b1.consume("analyst")
        self.assertEqual(b1.summary()["attempted_calls"], 1)

    def test_workflow_max_provider_calls_zero_blocks_before_subprocess(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            with mock.patch("tools.sota_ai_native.workflow._run_bounded_subprocess") as sub_mock, \
                 mock.patch("sys.stderr", new_callable=io.StringIO):
                rc = workflow.main([
                    "--topic", "Edge AI",
                    "--objective", "Test budget",
                    "--provider", "claude-cli",
                    "--allow-cli-session",
                    "--max-provider-calls", "0",
                    "--out-dir", td,
                ])
                self.assertEqual(rc, 1)
                sub_mock.assert_not_called()

    def test_workflow_max_provider_calls_negative_rejected(self) -> None:
        with mock.patch("sys.stderr", new_callable=io.StringIO):
            rc = workflow.main([
                "--topic", "Edge AI",
                "--objective", "Test negative budget",
                "--provider", "fixture",
                "--max-provider-calls", "-1",
            ])
            self.assertEqual(rc, 2)

    def test_workflow_max_provider_calls_one_stops_before_analyst(self) -> None:
        r_payload = json.dumps({
            "related_topics": ["Edge"],
            "papers": [{"Title": "Paper A", "Year": 2025, "Authors": "A", "Evidence level": "model-recall"}],
        })
        with tempfile.TemporaryDirectory() as td:
            with mock.patch("tools.sota_ai_native.workflow.shutil.which", return_value="/usr/bin/claude"), \
                 mock.patch("tools.sota_ai_native.workflow._run_bounded_subprocess", return_value=(0, json.dumps({"result": r_payload}), "")) as sub_mock, \
                 mock.patch("sys.stderr", new_callable=io.StringIO) as err:
                rc = workflow.main([
                    "--topic", "Edge AI",
                    "--objective", "Test partial budget",
                    "--provider", "claude-cli",
                    "--allow-cli-session",
                    "--max-provider-calls", "1",
                    "--out-dir", td,
                ])
                self.assertEqual(rc, 1)
                self.assertEqual(sub_mock.call_count, 1)
                self.assertIn("budget exhausted", err.getvalue().lower())
                self.assertFalse((pathlib.Path(td) / "sota_report.md").exists())

    def test_fixture_ignores_provider_call_budget_for_offline_mode(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            with mock.patch("sys.stdout", new_callable=io.StringIO):
                rc = workflow.main([
                    "--topic", "Edge AI",
                    "--objective", "Fixture budget",
                    "--provider", "fixture",
                    "--with-reviewer",
                    "--max-provider-calls", "0",
                    "--out-dir", td,
                    "--json",
                ])
                self.assertEqual(rc, 0)

    def test_authorize_boundary_still_blocks_paid_api_under_cli_session(self) -> None:
        ok, _ = authorize_provider_execution("openai-responses", allow_cli_session=True, allow_paid_api=False)
        self.assertFalse(ok)
        ok_cli, _ = authorize_provider_execution("claude-cli", allow_cli_session=False, allow_paid_api=False)
        self.assertFalse(ok_cli)
