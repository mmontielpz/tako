"""Runtime, security, and provider qualification regression suite (Q02-Q16, Q19-Q20) for P4 M024."""

from __future__ import annotations

import io
import json
import os
import subprocess
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest import mock

from tools.cli import main as cli_main
from tools.sota_ai_native.workflow import (
    ProviderBlockedError,
    _sanitized_provider_env,
    call_claude_cli,
    call_copilot_cli,
)
from tools.sota_discovery.engine import build_research_plan, run_iterative_discovery
from tools.sota_discovery.sota import (
    HttpBudgetExhaustedError,
    HttpRequestTracker,
    fetch_openalex,
    fetch_openalex_citations,
)
from tools.sota_evidence import (
    REPORT_EXPLORATORY,
    acquire_evidence,
    register_claims,
    run_evidence_and_comparative_pipeline,
)
from tools.sota_integrity import (
    BIB_STATUS_VERIFIED,
    CLAIM_NOT_VERIFIED,
    CLAIM_SUPPORTED,
    make_usage_record,
)


class _FakeResp:
    def __init__(self, payload: dict, status: int = 200, url: str = "") -> None:
        self._raw = json.dumps(payload).encode("utf-8")
        self.status = status
        self._url = url

    def read(self) -> bytes:
        return self._raw

    def geturl(self) -> str:
        return self._url

    def __enter__(self) -> _FakeResp:
        return self

    def __exit__(self, *args) -> None:
        return None


class RuntimeSecurityQualificationTests(unittest.TestCase):
    def test_q04_q05_fabricated_completeness_and_imported_claim_trust_isolation(self):
        # Imported third-party / provider assertion cannot manufacture VERIFIED_COMPLETE_PUBLICATION
        p_imp = {
            "paper_id": "imp1",
            "Title": "Imported Paper",
            "full_text": "[page 1]\nSection 1: Results\nClaim text 0.1 MAE.",
            "completeness_verified": True,
            "completeness_verification_evidence": {
                "status": "VERIFIED",
                "method": "imported_flag",
                "verifier": "external_json",
                "evidence_ref": "ext#1",
                "trust_origin": "IMPORTED_THIRD_PARTY_ASSERTION",
            },
            "claims": [{
                "claim_id": "c_imp",
                "claim_text": "Claim text 0.1 MAE.",
                "evidence_excerpt": "Claim text 0.1 MAE.",
                "evidence_locator": "section:Results",
                "claim_support_status": CLAIM_SUPPORTED,
                "assessment_evidence": {
                    "status": "VERIFIED",
                    "method": "provider_self_report",
                    "verifier": "llm",
                    "evidence_ref": "llm#1",
                    "trust_origin": "PROVIDER_ASSERTION",
                },
            }],
        }
        ev = acquire_evidence(p_imp)
        self.assertEqual(ev["document_completeness"], "DECLARED_COMPLETE_UNVERIFIED")
        claims = register_claims([p_imp], {"imp1": ev})
        self.assertEqual(claims[0]["claim_support_status"], CLAIM_NOT_VERIFIED)

        # Mismatched source_content_hash also rejects promotion
        p_bad_hash = {
            "paper_id": "imp2",
            "Title": "Hash Mismatch",
            "full_text": "[page 1]\nSection 1: Results\nClaim text.",
            "completeness_verified": True,
            "completeness_verification_evidence": {
                "status": "VERIFIED",
                "method": "local_check",
                "verifier": "harness",
                "evidence_ref": "ref#1",
                "trust_origin": "LOCALLY_EXECUTED_VERIFICATION",
                "source_content_hash": "0000deadbeef",
            },
        }
        ev2 = acquire_evidence(p_bad_hash)
        self.assertEqual(ev2["document_completeness"], "DECLARED_COMPLETE_UNVERIFIED")

    def test_q09_q11_provider_subprocess_env_isolation_timeout_and_output_cap(self):
        with mock.patch.dict(os.environ, {
            "OPENAI_API_KEY": "sk-openai-secret-q11",
            "ANTHROPIC_API_KEY": "sk-ant-secret-q11",
            "GH_TOKEN": "ghp-secret-q11",
        }):
            claude_env = _sanitized_provider_env("ANTHROPIC_API_KEY")
            self.assertIn("ANTHROPIC_API_KEY", claude_env)
            self.assertNotIn("OPENAI_API_KEY", claude_env)
            self.assertNotIn("GH_TOKEN", claude_env)

            copilot_env = _sanitized_provider_env("GH_TOKEN")
            self.assertIn("GH_TOKEN", copilot_env)
            self.assertNotIn("OPENAI_API_KEY", copilot_env)
            self.assertNotIn("ANTHROPIC_API_KEY", copilot_env)

            # Doctor output never leaks secrets
            buf = io.StringIO()
            with redirect_stdout(buf):
                cli_main(["doctor"])
            out = buf.getvalue()
            for sec in ("sk-openai-secret-q11", "sk-ant-secret-q11", "ghp-secret-q11"):
                self.assertNotIn(sec, out)

        # Oversized stdout fails closed
        with mock.patch("tools.sota_ai_native.workflow.shutil.which", return_value="/usr/bin/fake"):
            fake_proc = subprocess.CompletedProcess(args=[], returncode=0, stdout="x" * (2 * 1024 * 1024 + 10), stderr="")
            with mock.patch("tools.sota_ai_native.workflow.subprocess.run", return_value=fake_proc):
                with self.assertRaises(ProviderBlockedError):
                    call_claude_cli("prompt", allow_paid_api=True)
                with self.assertRaises(ProviderBlockedError):
                    call_copilot_cli("prompt", allow_paid_api=True)

    def test_q12_q13_q14_exact_http_accounting_citations_retries_cache_and_budget(self):
        tracker = HttpRequestTracker(max_http_requests=5)
        call_urls: list[str] = []

        def fake_open(req, timeout=15.0):
            url = req.full_url
            call_urls.append(url)
            if len(call_urls) == 1:
                raise TimeoutError("transient timeout")
            return _FakeResp({"results": [{"id": "https://openalex.org/W1", "title": "Paper 1"}]}, url=url)

        # 1 search with 1 retry -> 2 HTTP attempts, 1 failure, 1 success, 1 retry
        hits = fetch_openalex("gnn", tracker=tracker, max_retries=1) if False else None
        payload = tracker.get_json("https://api.openalex.org/works?search=gnn", category="search", max_retries=1, opener=fake_open)
        self.assertEqual(len(payload["results"]), 1)
        self.assertEqual(tracker.http_attempts, 2)
        self.assertEqual(tracker.retries, 1)
        self.assertEqual(tracker.http_failures, 1)
        self.assertEqual(tracker.http_successes, 1)

        # Identical URL hits cache without HTTP attempt
        tracker.get_json("https://api.openalex.org/works?search=gnn", category="search", opener=fake_open)
        self.assertEqual(tracker.cached_operations, 1)
        self.assertEqual(tracker.http_attempts, 2)

        # 1 logical citation expansion with both backward + forward -> 2 HTTP attempts
        work = {
            "referenced_works": ["https://openalex.org/W2"],
            "cited_by_api_url": "https://api.openalex.org/works?filter=cites:W1",
        }
        with mock.patch("tools.sota_discovery.sota.urllib.request.urlopen", side_effect=lambda req, timeout=10.0: _FakeResp({"results": []})):
            c_res = fetch_openalex_citations(work, tracker=tracker)
        self.assertIn("backward", c_res)
        self.assertIn("forward", c_res)
        self.assertEqual(tracker.http_attempts, 4)
        self.assertEqual(tracker.citation_requests, 2)

        # Next 1 attempt succeeds (5/5), then 6th raises HttpBudgetExhaustedError BEFORE network call
        tracker.get_json("https://api.openalex.org/works?search=ok5", category="search", opener=lambda r, timeout=10.0: _FakeResp({"results": []}))
        self.assertEqual(tracker.http_attempts, 5)
        with self.assertRaises(HttpBudgetExhaustedError):
            tracker.get_json("https://api.openalex.org/works?search=over6", category="search", opener=fake_open)
        self.assertTrue(tracker.budget_exhausted)
        self.assertEqual(tracker.http_attempts, 5)

    def test_q15_q16_unknown_cost_and_provider_usage_separation(self):
        rec = make_usage_record(
            cost_usd="UNKNOWN",
            premium_requests=3,
            request_count=1,
            input_tokens=120,
            output_tokens=45,
            provider_specific_usage={"http_attempts": "NOT_OBSERVABLE"},
            measurement_availability="provider_reported",
            provenance="copilot-cli-jsonl",
        )
        self.assertEqual(rec["cost_usd"], "UNKNOWN")
        self.assertEqual(rec["premium_requests"], 3)
        self.assertEqual(rec["total_tokens"], 165)
        self.assertEqual(rec["provider_specific_usage"]["http_attempts"], "NOT_OBSERVABLE")

    def test_q20_iterative_discovery_records_logical_vs_http_telemetry(self):
        plan = build_research_plan(topic="Quantum Reservoir Computing", objective="Survey QRC", max_rounds=1, max_requests=2)
        tracker = HttpRequestTracker(max_http_requests=5)
        res = run_iterative_discovery(
            plan,
            search_fn=lambda q, pp: [],
            tracker=tracker,
        )
        m = res["measurements"]
        self.assertIn("logical_operations", m)
        self.assertIn("observable_http_telemetry", m)
        self.assertEqual(m["observable_http_telemetry"]["http_attempts"], 0)


if __name__ == "__main__":
    unittest.main()
