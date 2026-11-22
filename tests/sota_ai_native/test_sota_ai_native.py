"""Offline unit tests for TAKO AI-Native SOTA Discovery workflow."""

import csv
import json
import pathlib
import subprocess
import tempfile
import unittest
from unittest import mock

from tools.sota_ai_native.workflow import main


FIXTURE_PAYLOAD = {
    "researcher": {
        "related_topics": ["Quantum Reservoir Computing", "Echo State Networks"],
        "papers": [
            {
                "Title": "Harnessing Disordered-Ensemble Quantum Dynamics for Machine Learning",
                "Authors": "Keisuke Fujii; Kohei Nakajima",
                "Year": 2017,
                "Publication venue": "Physical Review Applied",
                "DOI or source URL": "https://doi.org/10.1103/PhysRevApplied.8.024030",
                "Seminal work candidate": "YES (candidate)",
                "Evidence level": "abstract-based",
            }
        ],
    },
    "analyst": {
        "comparative_notes": "Single-paper fixture; no cross-paper benchmark comparison performed.",
        "papers": [
            {
                "Title": "Harnessing Disordered-Ensemble Quantum Dynamics for Machine Learning",
                "Authors": "Keisuke Fujii; Kohei Nakajima",
                "Year": 2017,
                "Publication venue": "Physical Review Applied",
                "DOI or source URL": "https://doi.org/10.1103/PhysRevApplied.8.024030",
                "Relevance rationale": "Foundational quantum reservoir computing proposal",
                "Seminal work candidate": "YES (candidate)",
                "Research contribution": "Proposes exploiting natural quantum dynamics as a computational reservoir.",
                "Reported results": "NOT_VERIFIED (full text not parsed in fixture)",
                "Limitations": "Measurement back-action and decoherence constraints.",
                "Evidence level": "abstract-based",
            }
        ],
    },
    "reviewer": {
        "challenged_claims": ["Reported quantitative benchmarks require full-text verification."],
        "missing_foundational": ["Jaeger & Haas (2004) Echo State Networks classical baseline."],
        "corrections": ["Mark quantitative claims NOT_VERIFIED until full-text inspection."],
    },
}


class SotaAiNativeTests(unittest.TestCase):
    def test_fail_closed_without_paid_authorization(self):
        with tempfile.TemporaryDirectory() as tmp:
            rc = main([
                "--topic", "Quantum Reservoir Computing",
                "--objective", "Test",
                "--provider", "openai-responses",
                "--out-dir", tmp,
            ])
            self.assertEqual(rc, 1)

    def test_fixture_researcher_analyst_and_optional_reviewer(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = pathlib.Path(tmp)
            fixture = tmp_path / "fixture.json"
            fixture.write_text(json.dumps(FIXTURE_PAYLOAD), encoding="utf-8")

            out_no_rev = tmp_path / "no_rev"
            rc1 = main([
                "--topic", "Quantum Reservoir Computing",
                "--objective", "Compare foundational works",
                "--provider", "fixture",
                "--fixture", str(fixture),
                "--out-dir", str(out_no_rev),
            ])
            self.assertEqual(rc1, 0)
            self.assertIn("SKIPPED", (out_no_rev / "review_report.md").read_text(encoding="utf-8"))

            out_with_rev = tmp_path / "with_rev"
            rc2 = main([
                "--topic", "Quantum Reservoir Computing",
                "--objective", "Compare foundational works",
                "--provider", "fixture",
                "--fixture", str(fixture),
                "--with-reviewer",
                "--out-dir", str(out_with_rev),
            ])
            self.assertEqual(rc2, 0)
            for fname in ("sota_report.md", "literature_matrix.csv", "references.bib", "review_report.md"):
                self.assertTrue((out_with_rev / fname).is_file())
            self.assertIn("Jaeger & Haas", (out_with_rev / "review_report.md").read_text(encoding="utf-8"))
            with (out_with_rev / "literature_matrix.csv").open(encoding="utf-8") as f:
                rows = list(csv.DictReader(f))
            self.assertEqual(len(rows), 1)
            self.assertIn("FIXTURE MODE", (out_with_rev / "sota_report.md").read_text(encoding="utf-8"))

    def test_bundled_default_fixture_and_copilot_robustness(self):
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = pathlib.Path(tmp) / "bundled"
            rc = main([
                "--topic", "Quantum Reservoir Computing",
                "--objective", "Verify bundled fixture",
                "--provider", "fixture",
                "--with-reviewer",
                "--out-dir", str(out_dir),
            ])
            self.assertEqual(rc, 0)
            self.assertTrue((out_dir / "review_report.md").is_file())

            # Copilot fails closed without --allow-paid-api
            self.assertEqual(main(["--topic", "QRC", "--objective", "T", "--provider", "copilot-cli", "--out-dir", tmp]), 1)

            # Copilot missing executable, timeout, nonzero exit, and invalid JSON
            with mock.patch("shutil.which", return_value=None):
                self.assertEqual(main(["--topic", "QRC", "--objective", "T", "--provider", "copilot-cli", "--allow-paid-api", "--out-dir", tmp]), 1)
            with mock.patch("shutil.which", return_value="/usr/bin/copilot"):
                with mock.patch("subprocess.run", side_effect=subprocess.TimeoutExpired("copilot", 90)):
                    self.assertEqual(main(["--topic", "QRC", "--objective", "T", "--provider", "copilot-cli", "--allow-paid-api", "--out-dir", tmp]), 1)
                with mock.patch("subprocess.run", return_value=mock.Mock(returncode=1, stdout="", stderr="auth required")):
                    self.assertEqual(main(["--topic", "QRC", "--objective", "T", "--provider", "copilot-cli", "--allow-paid-api", "--out-dir", tmp]), 1)
                with mock.patch("subprocess.run", return_value=mock.Mock(returncode=0, stdout="not json", stderr="")):
                    self.assertEqual(main(["--topic", "QRC", "--objective", "T", "--provider", "copilot-cli", "--allow-paid-api", "--out-dir", tmp]), 1)


if __name__ == "__main__":
    unittest.main()
