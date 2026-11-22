"""Offline unit tests for TAKO SOTA Discovery Mini MVP."""

import csv
import json
import pathlib
import tempfile
import unittest
from unittest import mock

from tools.sota_discovery.sota import RetrievalError, main, reconstruct_abstract


FIXTURE_DATA = {
    "results": [
        {
            "id": "https://openalex.org/W1",
            "doi": "https://doi.org/10.1126/science.1091277",
            "display_name": "Harnessing Nonlinearity: Predicting Chaotic Systems and Saving Energy in Wireless Communication",
            "publication_year": 2004,
            "cited_by_count": 3500,
            "authorships": [{"author": {"display_name": "Herbert Jaeger"}}, {"author": {"display_name": "Harald Haas"}}],
            "primary_location": {"source": {"display_name": "Science"}},
            "concepts": [{"display_name": "Echo state network"}, {"display_name": "Reservoir computing"}],
            "abstract_inverted_index": {
                "We": [0], "propose": [1], "echo": [2], "state": [3], "networks": [4],
                "for": [5], "nonlinear": [6], "prediction.": [7],
                "Experiments": [8], "show": [9], "2400-fold": [10], "accuracy": [11], "improvement.": [12],
                "However,": [13], "scaling": [14], "requires": [15], "careful": [16], "tuning.": [17],
            },
        },
        {
            "id": "https://openalex.org/W2",
            "doi": "https://doi.org/10.1103/PhysRevApplied.8.024030",
            "display_name": "Harnessing Disordered-Ensemble Quantum Dynamics for Machine Learning",
            "publication_year": 2017,
            "cited_by_count": 420,
            "authorships": [{"author": {"display_name": "Keisuke Fujii"}}, {"author": {"display_name": "Kohei Nakajima"}}],
            "primary_location": {"source": {"display_name": "Physical Review Applied"}},
            "concepts": [{"display_name": "Quantum reservoir computing"}, {"display_name": "Quantum computing"}],
            "abstract_inverted_index": None,
        },
    ]
}


class SotaMiniMvpTests(unittest.TestCase):
    def test_reconstruct_abstract(self):
        self.assertEqual(reconstruct_abstract({"hello": [0], "world": [1]}), "hello world")
        self.assertEqual(reconstruct_abstract(None), "")

    def test_fixture_end_to_end_outputs_and_no_fabrication(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = pathlib.Path(tmp)
            fixture = tmp_path / "fixture.json"
            fixture.write_text(json.dumps(FIXTURE_DATA), encoding="utf-8")
            out_dir = tmp_path / "out"

            rc = main([
                "--topic", "Quantum Reservoir Computing",
                "--objective", "Identify foundational works and limitations",
                "--max-papers", "5",
                "--seed", "Echo State Networks",
                "--out-dir", str(out_dir),
                "--fixture", str(fixture),
            ])
            self.assertEqual(rc, 0)
            self.assertTrue((out_dir / "sota_report.md").is_file())
            self.assertTrue((out_dir / "literature_matrix.csv").is_file())
            self.assertTrue((out_dir / "references.bib").is_file())

            with (out_dir / "literature_matrix.csv").open(encoding="utf-8") as f:
                rows = list(csv.DictReader(f))
            self.assertEqual(len(rows), 2)
            by_year = {r["Year"]: r for r in rows}
            self.assertEqual(by_year["2004"]["Evidence level"], "abstract-based")
            self.assertEqual(by_year["2017"]["Evidence level"], "metadata-only")
            self.assertIn("UNKNOWN", by_year["2017"]["Research contribution"])
            self.assertIn("FIXTURE MODE", (out_dir / "sota_report.md").read_text(encoding="utf-8"))

    def test_live_retrieval_failure_reports_explicit_blocker(self):
        with mock.patch("tools.sota_discovery.sota.fetch_openalex", side_effect=RetrievalError("network down")):
            with tempfile.TemporaryDirectory() as tmp:
                rc = main([
                    "--topic", "Quantum Reservoir Computing",
                    "--objective", "Test",
                    "--out-dir", tmp,
                ])
                self.assertEqual(rc, 1)


if __name__ == "__main__":
    unittest.main()
