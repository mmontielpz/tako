"""CLI tests for unified tako entry point."""

import io
import json
import os
import pathlib
import tempfile
import unittest
from contextlib import redirect_stdout
from unittest import mock

from tools.cli import main


class TakoCliTests(unittest.TestCase):
    def test_version_and_help(self):
        buf = io.StringIO()
        with self.assertRaises(SystemExit) as ctx:
            with redirect_stdout(buf):
                main(["--version"])
        self.assertEqual(ctx.exception.code, 0)
        self.assertIn("0.0.1.dev10", buf.getvalue())

        buf2 = io.StringIO()
        with self.assertRaises(SystemExit) as ctx2:
            with redirect_stdout(buf2):
                main(["--help"])
        self.assertEqual(ctx2.exception.code, 0)
        for cmd in ("doctor", "init", "status", "sota", "sota-ai", "evidence", "export"):
            self.assertIn(cmd, buf2.getvalue())

    def test_doctor_reports_status_without_leaking_secrets(self):
        buf = io.StringIO()
        with mock.patch.dict(os.environ, {"OPENAI_API_KEY": "sk-secret-do-not-leak-123456"}):
            with redirect_stdout(buf):
                rc = main(["doctor"])
        self.assertEqual(rc, 0)
        out = buf.getvalue()
        self.assertNotIn("sk-secret-do-not-leak", out)
        self.assertIn("INSTALLED=", out)
        self.assertIn("CONFIGURED=", out)
        self.assertIn("AVAILABLE=", out)
        self.assertIn("LIVE_TESTED=", out)
        self.assertIn("copilot-cli", out)

    def test_sota_and_sota_ai_reject_invalid_max_http_requests(self):
        self.assertEqual(main(["sota", "--topic", "T", "--objective", "O", "--max-http-requests", "0"]), 2)
        self.assertEqual(main(["sota-ai", "--topic", "T", "--objective", "O", "--provider", "fixture", "--max-http-requests", "0"]), 2)

    def test_sota_iterative_cli_enforces_max_http_requests_and_records_telemetry(self):
        transport_calls = []

        class _Resp:
            def __init__(self, payload: dict):
                self.status = 200
                self.headers = {}
                self._body = json.dumps(payload).encode("utf-8")

            def read(self):
                return self._body

            def __enter__(self):
                return self

            def __exit__(self, exc_type, exc, tb):
                return False

        def fake_open(req, timeout=15.0):
            transport_calls.append(req.full_url)
            return _Resp({
                "results": [
                    {
                        "id": f"https://openalex.org/W{len(transport_calls)}",
                        "doi": f"https://doi.org/10.1000/test.{len(transport_calls)}",
                        "display_name": "Graph Neural Networks for Molecular Property Prediction",
                        "publication_year": 2023,
                        "cited_by_count": 50,
                        "authorships": [{"author": {"display_name": "Alice Smith"}}],
                        "primary_location": {"source": {"display_name": "NeurIPS"}},
                        "concepts": [{"display_name": "Graph neural network"}],
                        "referenced_works": ["https://openalex.org/W999"],
                        "cited_by_api_url": "https://api.openalex.org/works?filter=cites:W1",
                        "abstract_inverted_index": {"Graph": [0], "neural": [1], "networks": [2], "predict": [3], "molecules.": [4]},
                    }
                ]
            })

        with tempfile.TemporaryDirectory() as tmp:
            out_dir = pathlib.Path(tmp) / "sota_out"
            with mock.patch("tools.sota_discovery.sota._default_no_redirect_open", side_effect=fake_open):
                with redirect_stdout(io.StringIO()):
                    rc = main([
                        "sota",
                        "--topic", "Graph Neural Networks",
                        "--objective", "Survey molecular property prediction",
                        "--iterative",
                        "--max-rounds", "2",
                        "--max-requests", "4",
                        "--max-http-requests", "2",
                        "--citation-depth", "1",
                        "--out-dir", str(out_dir),
                    ])
            self.assertEqual(rc, 0)
            self.assertEqual(len(transport_calls), 2)
            trace = json.loads((out_dir / "discovery_trace.json").read_text(encoding="utf-8"))
            self.assertEqual(trace["termination_reason"], "BUDGET_EXHAUSTED")
            m = trace["measurements"]
            self.assertIn("logical_operations", m)
            tel = m["observable_http_telemetry"]
            self.assertEqual(tel["max_http_requests"], 2)
            self.assertEqual(tel["http_attempts"], 2)
            self.assertTrue(tel["budget_exhausted"])

    def test_sota_ai_cli_verify_bib_enforces_max_http_requests(self):
        transport_calls = []

        class _Resp:
            def __init__(self, payload: dict):
                self.status = 200
                self.headers = {}
                self._body = json.dumps(payload).encode("utf-8")

            def read(self):
                return self._body

            def __enter__(self):
                return self

            def __exit__(self, exc_type, exc, tb):
                return False

        def fake_open(req, timeout=15.0):
            transport_calls.append(req.full_url)
            return _Resp({"results": []})

        with tempfile.TemporaryDirectory() as tmp:
            out_dir = pathlib.Path(tmp) / "sota_ai_out"
            with mock.patch("tools.sota_discovery.sota._default_no_redirect_open", side_effect=fake_open):
                with redirect_stdout(io.StringIO()):
                    rc = main([
                        "sota-ai",
                        "--topic", "Graph Neural Networks",
                        "--objective", "Verify bib budget",
                        "--provider", "fixture",
                        "--verify-bib",
                        "--max-http-requests", "1",
                        "--out-dir", str(out_dir),
                    ])
            self.assertEqual(rc, 0)
            self.assertEqual(len(transport_calls), 1)
            tel = json.loads((out_dir / "stage_02_bib_verification_telemetry.json").read_text(encoding="utf-8"))
            self.assertEqual(tel["max_http_requests"], 1)
            self.assertEqual(tel["http_attempts"], 1)
            self.assertTrue(tel["budget_exhausted"])
            bib = json.loads((out_dir / "stage_02_bib_verification.json").read_text(encoding="utf-8"))
            self.assertTrue(any("http_budget_exhausted" in (c.get("discrepancy_evidence") or []) for c in bib))


if __name__ == "__main__":
    unittest.main()
