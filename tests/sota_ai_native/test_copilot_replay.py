"""Offline replay tests for Copilot CLI JSONL transport and strict schema validation."""

import json
import unittest

from tools.sota_ai_native.workflow import (
    ProviderBlockedError,
    extract_copilot_jsonl_response,
    parse_provider_json,
    validate_role_payload,
)


class CopilotReplayTests(unittest.TestCase):
    def test_intermediate_narration_multiple_messages_and_tool_events(self):
        valid_researcher = {
            "related_topics": ["Quantum Reservoir Computing"],
            "papers": [{"Title": "Quantum Reservoir Computing Paper"}],
        }
        stream = "\n".join([
            json.dumps({"type": "user.message", "data": {"content": "prompt"}}),
            json.dumps({"type": "assistant.message", "data": {"content": "Let me search for DOIs..."}}),
            json.dumps({"type": "tool.execution_start", "data": {"tool": "web_fetch"}}),
            json.dumps({"type": "tool.execution_complete", "data": {"status": "ok"}}),
            json.dumps({"type": "assistant.message", "data": {"content": json.dumps(valid_researcher)}}),
            json.dumps({"type": "assistant.message", "data": {"content": "Done! Summary above."}}),
            json.dumps({"type": "result", "usage": {"input_tokens": 50, "output_tokens": 25}}),
        ])
        text, usage = extract_copilot_jsonl_response(stream)
        parsed = validate_role_payload("researcher", parse_provider_json(text, "copilot-cli"), "copilot-cli")
        self.assertEqual(parsed["papers"][0]["Title"], "Quantum Reservoir Computing Paper")
        self.assertEqual(usage["input_tokens"], 50)

    def test_missing_or_incomplete_completion_fails_closed(self):
        stream = "\n".join([
            json.dumps({"type": "user.message", "data": {"content": "prompt"}}),
            json.dumps({"type": "tool.execution_start", "data": {"tool": "bash"}}),
        ])
        with self.assertRaises(ProviderBlockedError):
            extract_copilot_jsonl_response(stream)

    def test_empty_malformed_and_competing_json_fail_closed(self):
        with self.assertRaises(ProviderBlockedError):
            parse_provider_json("   ", "copilot-cli")
        with self.assertRaises(ProviderBlockedError):
            parse_provider_json("plain prose without json", "copilot-cli")
        with self.assertRaises(ProviderBlockedError):
            parse_provider_json('{"a": 1} {"b": 2}', "copilot-cli")

    def test_schema_invalid_scientific_output_rejected(self):
        arbitrary = parse_provider_json('{"status": "ok", "value": 42}', "copilot-cli")
        with self.assertRaises(ProviderBlockedError):
            validate_role_payload("researcher", arbitrary, "copilot-cli")
        with self.assertRaises(ProviderBlockedError):
            validate_role_payload("analyst", {"papers": [{"Authors": "Missing Title"}]}, "copilot-cli")
        with self.assertRaises(ProviderBlockedError):
            validate_role_payload("reviewer", {"notes": "not a list"}, "copilot-cli")

    def test_observed_copilot_1_0_114_usage_and_final_answer_shape(self):
        stream = "\n".join([
            json.dumps({"type": "assistant.message", "data": {"phase": "final_answer", "content": '{"status":"ok","value":42}'}}),
            json.dumps({"type": "session.usage_checkpoint", "data": {"totalPremiumRequests": 1}}),
            json.dumps({"type": "result", "exitCode": 0, "usage": {"premiumRequests": 1}}),
        ])
        text, usage = extract_copilot_jsonl_response(stream)
        self.assertEqual(parse_provider_json(text, "copilot-cli"), {"status": "ok", "value": 42})
        self.assertEqual(usage["cost_usd"], "UNKNOWN")
        self.assertEqual(usage["premium_requests"], 1)

    def test_author_list_and_unsupported_evidence_label_normalization(self):
        payload = {
            "related_topics": ["QRC"],
            "papers": [{
                "Title": "Echo State Paper",
                "Authors": ["Herbert Jaeger", "Harald Haas"],
                "Year": "2004",
                "Evidence level": "High",
            }],
        }
        norm = validate_role_payload("researcher", payload, "copilot-cli")
        p = norm["papers"][0]
        self.assertEqual(p["Authors"], "Herbert Jaeger; Harald Haas")
        self.assertEqual(p["Year"], 2004)
        self.assertTrue(p["Evidence level"].startswith(("MODEL_RECALL", "UNKNOWN")))
        self.assertNotIn("abstract-based", p["Evidence level"])
        self.assertNotIn("full-text", p["Evidence level"])

        p2 = validate_role_payload("analyst", {"papers": [{"Title": "T", "Evidence level": "MODEL_RECALL"}]}, "copilot-cli")["papers"][0]
        self.assertEqual(p2["Evidence level"], "MODEL_RECALL")

    def test_write_outputs_handles_list_comparative_notes(self):
        import pathlib, tempfile
        from tools.sota_ai_native.workflow import write_ai_native_outputs
        with tempfile.TemporaryDirectory() as tmp:
            outs = write_ai_native_outputs(
                out_dir=pathlib.Path(tmp),
                topic="QRC",
                objective="Test",
                provider="copilot-cli",
                researcher_out={"related_topics": ["QRC"], "papers": []},
                analyst_out={"comparative_notes": ["Note A", "Note B"], "papers": [{"Title": "P1", "Year": 2017}]},
                reviewer_out={"challenged_claims": ["C1"], "missing_foundational": [], "corrections": []},
                bib_checks=None,
                usage_totals={"input_tokens": "UNKNOWN", "output_tokens": "UNKNOWN", "cost_usd": "premium_requests:3"},
                elapsed_seconds=1.0,
            )
            self.assertTrue(all(p.is_file() for p in outs))
            self.assertIn("- Note A", (pathlib.Path(tmp) / "sota_report.md").read_text(encoding="utf-8"))

    def test_stage_persistence_and_failure_without_success_artifacts(self):
        import pathlib, tempfile
        from unittest import mock
        from tools.sota_ai_native.workflow import main
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = pathlib.Path(tmp) / "run"
            r_json = json.dumps({"type": "assistant.message", "data": {"phase": "final_answer", "content": json.dumps({"related_topics": ["QRC"], "papers": [{"Title": "Paper 1", "Authors": ["A. One"], "Year": 2017, "Evidence level": "High"}]})}})
            a_json = json.dumps({"type": "assistant.message", "data": {"phase": "final_answer", "content": json.dumps({"comparative_notes": ["N1"], "papers": [{"Title": "Paper 1", "Authors": ["A. One"], "Year": 2017, "Evidence level": "MODEL_RECALL"}]})}})
            v_json = json.dumps({"type": "assistant.message", "data": {"phase": "final_answer", "content": json.dumps({"challenged_claims": [{"claim": "X"}], "missing_foundational": [], "corrections": ["C1"]})}})
            with mock.patch("shutil.which", return_value="/usr/bin/copilot"):
                with mock.patch("subprocess.run", side_effect=[
                    mock.Mock(returncode=0, stdout=r_json, stderr=""),
                    mock.Mock(returncode=0, stdout=a_json, stderr=""),
                    mock.Mock(returncode=0, stdout=v_json, stderr=""),
                ]):
                    rc = main(["--topic", "QRC", "--objective", "T", "--provider", "copilot-cli", "--allow-paid-api", "--with-reviewer", "--out-dir", str(out_dir)])
            self.assertEqual(rc, 0)
            for fname in ("stage_01_researcher.json", "stage_03_analyst.json", "stage_04_reviewer.json", "sota_report.md", "literature_matrix.csv", "references.bib", "review_report.md"):
                self.assertTrue((out_dir / fname).is_file(), fname)

            # Failure at Analyst leaves stage_01 persisted and does not write sota_report.md
            fail_dir = pathlib.Path(tmp) / "fail_run"
            with mock.patch("shutil.which", return_value="/usr/bin/copilot"):
                with mock.patch("subprocess.run", side_effect=[
                    mock.Mock(returncode=0, stdout=r_json, stderr=""),
                    mock.Mock(returncode=0, stdout="not json", stderr=""),
                ]):
                    rc2 = main(["--topic", "QRC", "--objective", "T", "--provider", "copilot-cli", "--allow-paid-api", "--out-dir", str(fail_dir)])
            self.assertEqual(rc2, 1)
            self.assertTrue((fail_dir / "stage_01_researcher.json").is_file())
            self.assertFalse((fail_dir / "sota_report.md").exists())


if __name__ == "__main__":
    unittest.main()
