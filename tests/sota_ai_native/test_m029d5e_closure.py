"""M029D5E targeted qualification closure regression tests."""

import io
import json
import tempfile
import unittest
from unittest import mock

from tools.ai_cli_providers import build_cli_invocation, decode_cli_provider_stdout
from tools.sota_ai_native import workflow
from tools.sota_integrity import aggregate_usage_records, make_usage_record


class TestM029D5EClosure(unittest.TestCase):
    def test_provider_tool_and_sandbox_restrictions_in_invocations(self) -> None:
        self.assertIn("--sandbox", build_cli_invocation("agy-cli", "/bin/agy", "p"))
        self.assertEqual(build_cli_invocation("claude-cli", "/bin/claude", "p")[-2:], ["--tools", ""])
        codex_cmd = build_cli_invocation("codex-cli", "/bin/codex", "p")
        self.assertIn("--sandbox", codex_cmd)
        self.assertIn("read-only", codex_cmd)
        self.assertIn("--skip-git-repo-check", codex_cmd)

    def test_codex_stderr_total_tokens_parsed_without_fabricating_breakdown(self) -> None:
        raw_out = json.dumps({"papers": [{"Title": "EdgeSegNet"}]})
        raw_err = "OpenAI Codex v0.169.0\n...\ntokens used\n4,668\n"
        text, meta = decode_cli_provider_stdout("codex-cli", raw_out, raw_err)
        self.assertEqual(json.loads(text)["papers"][0]["Title"], "EdgeSegNet")
        self.assertEqual(meta["total_tokens"], 4668)
        self.assertEqual(meta["input_tokens"], "UNKNOWN")
        self.assertEqual(meta["output_tokens"], "UNKNOWN")
        self.assertEqual(meta["measurement_availability"], "provider_reported")

        rec = make_usage_record(
            input_tokens=meta["input_tokens"],
            output_tokens=meta["output_tokens"],
            total_tokens=meta["total_tokens"],
            wall_time_seconds=15.491,
            measurement_availability=meta["measurement_availability"],
            provenance=meta["provenance"],
        )
        self.assertEqual(rec["total_tokens"], 4668)
        agg = aggregate_usage_records([rec], wall_time_seconds=15.491)
        self.assertEqual(agg["total_tokens"], 4668)
        self.assertEqual(agg["input_tokens"], "UNKNOWN")
        self.assertEqual(agg["output_tokens"], "UNKNOWN")

    def test_finite_default_provider_budget_for_authenticated_workflows(self) -> None:
        r_json = json.dumps({"related_topics": ["Edge"], "papers": [{"Title": "P1", "Evidence level": "model-recall"}]})
        a_json = json.dumps({"comparative_notes": ["N1"], "papers": [{"Title": "P1", "Evidence level": "model-recall"}]})
        with tempfile.TemporaryDirectory() as td:
            with mock.patch("tools.sota_ai_native.workflow.shutil.which", return_value="/usr/bin/codex"), \
                 mock.patch("tools.sota_ai_native.workflow._run_bounded_subprocess", side_effect=[(0, r_json, ""), (0, a_json, "")]), \
                 mock.patch("sys.stdout", new_callable=io.StringIO) as out:
                rc = workflow.main([
                    "--topic", "Edge AI",
                    "--objective", "Survey",
                    "--provider", "codex-cli",
                    "--allow-cli-session",
                    "--out-dir", td,
                    "--json",
                ])
                self.assertEqual(rc, 0)
                data = json.loads(out.getvalue())
                self.assertEqual(data["provider_call_budget"]["max_provider_calls"], 2)
                self.assertEqual(data["provider_call_budget"]["attempted_calls"], 2)

    def test_free_text_evidence_labels_rejected_to_unknown(self) -> None:
        n1 = workflow.normalize_evidence_level("Explicitly reported in a synthetic abstract.", "copilot-cli", has_verified_source_text=True)
        n2 = workflow.normalize_evidence_level("Synthetic abstract only.", "codex-cli", has_verified_source_text=True)
        self.assertTrue(n1.startswith("UNKNOWN (unsupported_label="))
        self.assertTrue(n2.startswith("UNKNOWN (unsupported_label="))
