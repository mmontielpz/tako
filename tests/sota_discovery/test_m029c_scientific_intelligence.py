"""Deterministic tests for foundational discovery, transparent ranking, and structured extraction (M029C)."""

from __future__ import annotations

import json
import unittest
from unittest import mock

from tools.sota_discovery.engine import (
    TERM_MIN_BREADTH_REACHED,
    _extract_core_concept_query,
    build_research_plan,
    compute_relevance_signals,
    run_iterative_discovery,
)
from tools.sota_extraction import (
    STRUCTURED_FIELDS,
    extract_structured_deterministic,
    extract_structured_with_cli,
)


class M029CScientificIntelligenceTests(unittest.TestCase):
    def test_topic_agnostic_concept_decomposition_and_unrelated_domain(self):
        self.assertEqual(
            _extract_core_concept_query("Real-Time Semantic Segmentation for Edge AI"),
            "Real-Time Semantic Segmentation",
        )
        self.assertEqual(
            _extract_core_concept_query("Quantum Reservoir Computing for Financial Time Series"),
            "Quantum Reservoir Computing",
        )
        self.assertEqual(_extract_core_concept_query("Edge AI"), "")

        # Unrelated topic fixture run works identically without segmentation assumptions
        plan = build_research_plan(
            topic="Quantum Reservoir Computing for Financial Time Series",
            objective="Compare physical reservoir architectures and forecasting metrics",
            max_papers=5,
            max_rounds=2,
            prefer_review_first=False,
        )
        fx = [
            {
                "id": "https://openalex.org/W-qrc-1",
                "title": "Quantum Reservoir Computing with Nuclear Spins",
                "publication_year": 2017,
                "cited_by_count": 420,
                "doi": "https://doi.org/10.1000/qrc.2017",
                "authorships": [{"author": {"display_name": "K. Fujii"}}],
                "primary_location": {"source": {"display_name": "Physical Review Applied"}},
                "abstract_inverted_index": {"Quantum": [0], "reservoir": [1], "computing": [2], "architecture": [3], "forecasting": [4], "metrics": [5]},
                "referenced_works": [],
            }
        ]
        res = run_iterative_discovery(plan, search_fn=lambda q, n: fx[:n])
        self.assertEqual(res["termination_reason"], TERM_MIN_BREADTH_REACHED)
        self.assertEqual(len(res["buckets"]["ACCEPTED_CORPUS"]), 1)
        rec = next(iter(res["buckets"]["ACCEPTED_CORPUS"].values()))
        self.assertIn("ranking_signals", rec)
        self.assertIn("topic_relevance", rec["ranking_signals"])

    def test_ranking_signals_integrity_irrelevant_high_cites_and_recent_low_cites(self):
        plan = build_research_plan(
            topic="Real-Time Semantic Segmentation for Edge AI",
            objective="Identify efficient architectures and latency metrics",
            max_papers=5,
            max_rounds=2,
            prefer_review_first=False,
        )
        fx = [
            # Irrelevant high-citation paper must NOT be accepted
            {
                "id": "https://openalex.org/W-irr",
                "title": "Deep Residual Learning for Image Recognition",
                "publication_year": 2016,
                "cited_by_count": 150000,
                "doi": "https://doi.org/10.1109/cvpr.2016.90",
                "authorships": [{"author": {"display_name": "K. He"}}],
                "primary_location": {"source": {"display_name": "CVPR"}},
                "abstract_inverted_index": {"Image": [0], "classification": [1], "residual": [2], "network": [3]},
                "referenced_works": [],
            },
            # Recent low-citation relevant paper must be accepted
            {
                "id": "https://openalex.org/W-rec",
                "title": "Recent Real-Time Semantic Segmentation Architecture for Edge Devices",
                "publication_year": 2025,
                "cited_by_count": 2,
                "doi": "https://doi.org/10.1000/recent.2025",
                "authorships": [{"author": {"display_name": "R. Author"}}],
                "primary_location": {"source": {"display_name": "WACV"}},
                "abstract_inverted_index": {"Real-time": [0], "semantic": [1], "segmentation": [2], "efficient": [3], "latency": [4], "edge": [5]},
                "referenced_works": ["https://openalex.org/W-found"],
            },
            # Foundational highly cited relevant paper
            {
                "id": "https://openalex.org/W-found",
                "title": "Bilateral Network for Real-Time Semantic Segmentation",
                "publication_year": 2018,
                "cited_by_count": 3200,
                "doi": "https://doi.org/10.1000/found.2018",
                "authorships": [{"author": {"display_name": "F. Author"}}],
                "primary_location": {"source": {"display_name": "ECCV"}},
                "abstract_inverted_index": {"Real-time": [0], "semantic": [1], "segmentation": [2], "efficient": [3], "architecture": [4], "latency": [5]},
                "referenced_works": [],
            },
            # Missing abstract metadata-only paper must not enter ACCEPTED_CORPUS
            {
                "id": "https://openalex.org/W-noabs",
                "title": "Real-Time Semantic Segmentation Without Abstract",
                "publication_year": 2024,
                "cited_by_count": 5,
                "doi": "https://doi.org/10.1000/noabs.2024",
                "authorships": [{"author": {"display_name": "M. Author"}}],
                "primary_location": {"source": {"display_name": "Workshop"}},
                "referenced_works": [],
            },
        ]
        res = run_iterative_discovery(plan, search_fn=lambda q, n: fx[:n])
        acc_titles = [r["Title"] for r in res["buckets"]["ACCEPTED_CORPUS"].values()]
        self.assertNotIn("Deep Residual Learning for Image Recognition", acc_titles)
        self.assertNotIn("Real-Time Semantic Segmentation Without Abstract", acc_titles)
        self.assertIn("Recent Real-Time Semantic Segmentation Architecture for Edge Devices", acc_titles)
        self.assertIn("Bilateral Network for Real-Time Semantic Segmentation", acc_titles)

        found_rec = next(r for r in res["buckets"]["ACCEPTED_CORPUS"].values() if "Bilateral" in r["Title"])
        self.assertTrue(found_rec["ranking_signals"]["foundational_candidate"])
        self.assertIn("requires human confirmation", found_rec["ranking_signals"]["seminal_status"])

    def test_structured_extraction_contract_and_cli_boundary(self):
        rec = {
            "Title": "EdgeSegNet",
            "DOI or source URL": "https://doi.org/10.1000/edgeseg",
            "Evidence availability": "ABSTRACT_ONLY",
            "abstract": "We propose EdgeSegNet, a novel architecture evaluated on Cityscapes achieving 74.2% mIoU at 60 FPS. Limitations include TensorRT FP16 quantization sensitivity.",
        }
        ext = extract_structured_deterministic(rec)
        self.assertEqual(ext["evidence_type"], "ABSTRACT_BASED")
        self.assertEqual(ext["verification_status"], "ABSTRACT_ONLY_UNVERIFIED_FOR_FULL_CLAIMS")
        self.assertTrue(set(STRUCTURED_FIELDS).issubset(ext["fields"].keys()))
        self.assertIn("Cityscapes", ext["fields"]["datasets"])
        self.assertIn("research_gaps", ext["missing_information"])
        self.assertIn("research_problem", ext["missing_information"])
        for c in ext["claims"]:
            self.assertEqual(c["human_review_status"], "PENDING_HUMAN_REVIEW")
            self.assertEqual(c["evidence_type"], "ABSTRACT_BASED")

        # CLI extraction fails closed without authorization
        with self.assertRaises(PermissionError):
            extract_structured_with_cli(rec, provider="agy-cli", allow_cli_session=False)

        # Mocked CLI extraction with --allow-cli-session
        mock_stdout = json.dumps({"result": json.dumps({"architecture": "Dual-branch encoder-decoder"})})
        with mock.patch("tools.sota_extraction.shutil.which", return_value="/usr/bin/agy"):
            with mock.patch("tools.sota_extraction.subprocess.run", return_value=mock.Mock(returncode=0, stdout=mock_stdout, stderr="")):
                ext_cli = extract_structured_with_cli(rec, provider="agy-cli", allow_cli_session=True)
                self.assertEqual(ext_cli["fields"]["architecture"], "Dual-branch encoder-decoder")
                self.assertEqual(ext_cli["evidence_type"], "ABSTRACT_BASED")

    def test_post_ranking_corpus_selection_and_citation_sort_forwarding(self):
        plan = build_research_plan(
            topic="Real-Time Semantic Segmentation for Edge AI",
            objective="Identify efficient architectures and latency metrics",
            max_papers=1,
            max_rounds=2,
            prefer_review_first=False,
        )
        seen_sorts: list[str | None] = []

        def fake_search(q: str, n: int, *, sort: str | None = None):
            seen_sorts.append(sort)
            if sort == "cited_by_count:desc":
                return [{
                    "id": "https://openalex.org/W-seminal",
                    "title": "High-Citation Real-Time Semantic Segmentation Architecture",
                    "publication_year": 2021,
                    "cited_by_count": 2500,
                    "doi": "https://doi.org/10.1000/sem.2021",
                    "authorships": [{"author": {"display_name": "S. Author"}}],
                    "primary_location": {"source": {"display_name": "CVPR"}},
                    "abstract_inverted_index": {"Real-time": [0], "semantic": [1], "segmentation": [2], "efficient": [3], "architecture": [4], "latency": [5], "edge": [6]},
                    "referenced_works": [],
                }]
            return [{
                "id": "https://openalex.org/W-low",
                "title": "Low-Citation Real-Time Semantic Segmentation for Edge AI",
                "publication_year": 2025,
                "cited_by_count": 1,
                "doi": "https://doi.org/10.1000/low.2025",
                "authorships": [{"author": {"display_name": "L. Author"}}],
                "primary_location": {"source": {"display_name": "Workshop"}},
                "abstract_inverted_index": {"Real-time": [0], "semantic": [1], "segmentation": [2], "edge": [3]},
                "referenced_works": [],
            }]

        res = run_iterative_discovery(plan, search_fn=fake_search)
        self.assertIn("cited_by_count:desc", seen_sorts)
        acc_titles = [r["Title"] for r in res["buckets"]["ACCEPTED_CORPUS"].values()]
        # Even though W-low arrived first and max_papers=1, W-seminal must win post-ranking selection
        self.assertEqual(acc_titles, ["High-Citation Real-Time Semantic Segmentation Architecture"])


if __name__ == "__main__":
    unittest.main()
