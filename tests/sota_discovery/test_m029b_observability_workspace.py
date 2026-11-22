"""Deterministic tests for workflow events, NO_COLOR/JSON UX, and workspace integration (M029B)."""

from __future__ import annotations

import contextlib
import io
import json
import os
import pathlib
import tempfile
import unittest
from unittest import mock

from tools.cli import main as cli_main
from tools.workflow_events import EVENT_SCHEMA_VERSION, WorkflowEventEmitter, sanitize_error


class M029BObservabilityWorkspaceTests(unittest.TestCase):
    def test_event_schema_ordering_no_color_and_redaction(self):
        with tempfile.TemporaryDirectory() as tmp:
            ev_path = pathlib.Path(tmp) / "events.jsonl"
            with mock.patch.dict(os.environ, {"NO_COLOR": "1", "OPENAI_API_KEY": "sk-secret-val"}):
                buf = io.StringIO()
                with contextlib.redirect_stdout(buf):
                    em = WorkflowEventEmitter(workflow="sota", workspace_id="ws-1", run_id="run-1", events_file=ev_path)
                    em.emit("workflow_started", stage="discovery", status="RUNNING", message="Start")
                    em.emit("stage_started", stage="iterative_search", status="RUNNING", message="Search")
                    em.emit("stage_completed", stage="iterative_search", status="COMPLETED", counters={"http_attempts": 0, "accepted_papers": 1})
                    em.emit("workflow_completed", stage="discovery", status="COMPLETED")
                self.assertNotIn("\033[", buf.getvalue())
            events = [json.loads(line) for line in ev_path.read_text(encoding="utf-8").splitlines()]
            self.assertEqual([e["event_type"] for e in events], ["workflow_started", "stage_started", "stage_completed", "workflow_completed"])
            self.assertTrue(all(e["schema_version"] == EVENT_SCHEMA_VERSION and e["run_id"] == "run-1" for e in events))
            self.assertNotIn("sk-secret-val", sanitize_error("Error with sk-secret-val"))

    def test_workspace_registration_json_collision_stale_and_interrupted(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            ws = root / "ws"
            fx = root / "fx.json"
            fx.write_text(
                json.dumps({
                    "results": [{
                        "id": "https://openalex.org/W1",
                        "title": "Edge Segmentation Baseline",
                        "publication_year": 2024,
                        "cited_by_count": 10,
                        "doi": "https://doi.org/10.1000/edge.1",
                        "authorships": [{"author": {"display_name": "A. Author"}}],
                        "primary_location": {"source": {"display_name": "CVPR"}},
                        "referenced_works": [],
                    }]
                }),
                encoding="utf-8",
            )
            self.assertEqual(cli_main(["init", "--workspace", str(ws), "--topic", "Edge Seg", "--objective", "Survey"]), 0)

            # 1. Run sota --workspace --json without repeating --topic/--objective
            out_json = io.StringIO()
            with contextlib.redirect_stdout(out_json):
                rc = cli_main(["sota", "--workspace", str(ws), "--iterative", "--fixture", str(fx), "--json"])
            self.assertEqual(rc, 0)
            payload = json.loads(out_json.getvalue())
            self.assertEqual(payload["status"], "SOTA_ITERATIVE_COMPLETE")
            self.assertIn("http_attempts", payload["counters"])

            # 2. Output collision without --force must fail
            err_col = io.StringIO()
            with contextlib.redirect_stderr(err_col):
                rc_col = cli_main(["sota", "--workspace", str(ws), "--iterative", "--fixture", str(fx)])
            self.assertEqual(rc_col, 2)
            self.assertIn("OUTPUT_ERROR", err_col.getvalue())

            # 3. tako status reflects COMPLETED discovery stage
            st_buf = io.StringIO()
            with contextlib.redirect_stdout(st_buf):
                self.assertEqual(cli_main(["status", "--workspace", str(ws)]), 0)
            self.assertIn("discovery: COMPLETED", st_buf.getvalue())

            # 4. Deleting a registered output marks stage STALE
            (ws / "sota_output" / "sota_report.md").unlink()
            st_stale = io.StringIO()
            with contextlib.redirect_stdout(st_stale):
                self.assertEqual(cli_main(["status", "--workspace", str(ws)]), 0)
            self.assertIn("discovery: STALE", st_stale.getvalue())

            # 5. Interrupted run marks stage INTERRUPTED (exit 130)
            with mock.patch("tools.sota_discovery.engine.run_iterative_discovery", side_effect=KeyboardInterrupt):
                err_int = io.StringIO()
                with contextlib.redirect_stderr(err_int):
                    rc_int = cli_main(["sota", "--workspace", str(ws), "--iterative", "--fixture", str(fx), "--force"])
                self.assertEqual(rc_int, 130)
            st_int = io.StringIO()
            with contextlib.redirect_stdout(st_int):
                self.assertEqual(cli_main(["status", "--workspace", str(ws)]), 0)
            self.assertIn("discovery: INTERRUPTED", st_int.getvalue())

            # 6. Failed sota-ai run marks stage FAILED
            err_fail = io.StringIO()
            with contextlib.redirect_stderr(err_fail):
                rc_fail = cli_main(["sota-ai", "--workspace", str(ws), "--provider", "claude-cli", "--force"])
            self.assertEqual(rc_fail, 1)
            st_fail = io.StringIO()
            with contextlib.redirect_stdout(st_fail):
                self.assertEqual(cli_main(["status", "--workspace", str(ws)]), 0)
            self.assertIn("discovery: FAILED", st_fail.getvalue())


if __name__ == "__main__":
    unittest.main()
