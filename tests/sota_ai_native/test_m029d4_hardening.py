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
from tools.sota_discovery.sota import HttpRequestTracker
from tools.sota_extraction import extract_structured_deterministic, extract_structured_with_cli


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


class TestM029D4ProviderNormalizationAndTelemetry(unittest.TestCase):
    def test_codex_invocation_uses_read_only_sandbox_and_skip_git_check(self) -> None:
        cmd = build_cli_invocation("codex-cli", "/usr/bin/codex", "hello")
        self.assertEqual(cmd, ["/usr/bin/codex", "exec", "--sandbox", "read-only", "--skip-git-repo-check", "hello"])

    def test_agy_envelope_unwrapped_and_wall_time_measured(self) -> None:
        agy_stdout = json.dumps({"response": json.dumps({"papers": [{"Title": "EdgeSegNet"}]}), "usage": {"input_tokens": 10, "output_tokens": 20}})
        with mock.patch("tools.sota_ai_native.workflow.shutil.which", return_value="/usr/bin/agy"), \
             mock.patch("tools.sota_ai_native.workflow._run_bounded_subprocess", return_value=(0, agy_stdout, "")):
            payload, usage = workflow.run_role("analyst", "p", "agy-cli", "", False, None, allow_cli_session=True)
            self.assertEqual(payload["papers"][0]["Title"], "EdgeSegNet")
            self.assertGreaterEqual(usage["wall_time_seconds"], 0.0)
            self.assertEqual(usage["input_tokens"], 10)

    def test_extract_structured_with_cli_handles_copilot_jsonl_and_codex_markdown(self) -> None:
        rec = {"title": "EdgeSegNet", "abstract": "Dual-branch CNN achieves 78.4% mIoU on Cityscapes val."}
        copilot_jsonl = (
            '{"type":"assistant.message","data":{"content":"{\\"architecture\\":\\"Dual-branch CNN\\"}"}}\n'
            '{"type":"result","usage":{"premiumRequests":1}}\n'
        )
        with mock.patch("tools.sota_extraction.shutil.which", return_value="/usr/bin/copilot"), \
             mock.patch("tools.sota_extraction.subprocess.run", return_value=mock.Mock(returncode=0, stdout=copilot_jsonl, stderr="")):
            res = extract_structured_with_cli(rec, provider="copilot-cli", allow_cli_session=True, allow_unisolated_cli=True)
            self.assertEqual(res["fields"]["architecture"], "Dual-branch CNN")

        codex_md = 'Here is the extraction:\n```json\n{"methodology": "Gated attention refinement"}\n```'
        with mock.patch("tools.sota_extraction.shutil.which", return_value="/usr/bin/codex"), \
             mock.patch("tools.sota_extraction.subprocess.run", return_value=mock.Mock(returncode=0, stdout=codex_md, stderr="")):
            res2 = extract_structured_with_cli(rec, provider="codex-cli", allow_cli_session=True)
            self.assertEqual(res2["fields"]["methodology"], "Gated attention refinement")

    def test_extract_structured_with_cli_rejects_competing_json_and_exhausted_budget(self) -> None:
        rec = {"title": "EdgeSegNet", "abstract": "Some abstract."}
        competing = '{"architecture": "A"} {"architecture": "B"}'
        with mock.patch("tools.sota_extraction.shutil.which", return_value="/usr/bin/codex"), \
             mock.patch("tools.sota_extraction.subprocess.run", return_value=mock.Mock(returncode=0, stdout=competing, stderr="")):
            with self.assertRaises(Exception):
                extract_structured_with_cli(rec, provider="codex-cli", allow_cli_session=True)

        b0 = ProviderCallBudget(max_calls=0)
        with mock.patch("tools.sota_extraction.shutil.which", return_value="/usr/bin/claude"):
            with self.assertRaises(BudgetExhaustedError):
                extract_structured_with_cli(rec, provider="claude-cli", allow_cli_session=True, budget=b0)


class TestM029D4ScientificIntegrityAndRateLimit(unittest.TestCase):
    def test_normalize_evidence_level_uses_provenance_not_provider_identity(self) -> None:
        self.assertEqual(workflow.normalize_evidence_level("abstract-based", "copilot-cli", has_verified_source_text=True), "abstract-based")
        self.assertEqual(workflow.normalize_evidence_level("abstract-based", "claude-cli", has_verified_source_text=True), "abstract-based")
        self.assertTrue(workflow.normalize_evidence_level("abstract-based", "copilot-cli", has_verified_source_text=False).startswith("MODEL_RECALL"))

    def test_deterministic_abstention_on_synthetic_abstract(self) -> None:
        rec = {
            "title": "EdgeSegNet",
            "abstract": "Evaluated on Cityscapes val using TensorRT FP16, EdgeSegNet achieves 78.4% mIoU at 62.5 FPS. On CamVid test, it reaches 76.1% mIoU at 94.0 FPS.",
        }
        out = extract_structured_deterministic(rec)
        self.assertEqual(out["evidence_type"], "ABSTRACT_BASED")
        self.assertIn("methodology", out["missing_information"])
        self.assertEqual(out["fields"]["methodology"], "NOT_AVAILABLE")

    def test_http_429_and_503_bounded_backoff_counts_toward_budget(self) -> None:
        sleeps: list[float] = []
        tracker = HttpRequestTracker(max_http_requests=5, provider_min_intervals={"openalex": 0.0}, sleep_fn=sleeps.append)
        calls = {"n": 0}

        class _FakeResp:
            def __init__(self, status: int, body: str = '{"ok": true}') -> None:
                self.status = status
                self._body = body.encode("utf-8")
                self.headers = {}

            def read(self) -> bytes:
                return self._body

            def __enter__(self):
                return self

            def __exit__(self, *args):
                return False

        def fake_open(req, timeout=15.0):
            calls["n"] += 1
            if calls["n"] == 1:
                return _FakeResp(429)
            if calls["n"] == 2:
                return _FakeResp(503)
            return _FakeResp(200)

        with mock.patch("tools.sota_discovery.sota.urllib.request.urlopen", side_effect=fake_open):
            res = tracker.get_json("https://api.openalex.org/works", max_retries=2, provider="openalex")
        self.assertEqual(res, {"ok": True})
        self.assertEqual(tracker.http_attempts, 3)
        self.assertEqual(tracker.rate_limited_responses, 1)
        self.assertEqual(len(sleeps), 2)
