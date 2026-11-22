"""Deterministic tests for multi-source discovery providers, registry, budgets, and M024B corrective findings."""

from __future__ import annotations

import io
import json
import os
import pathlib
import tempfile
import unittest
import urllib.error
from contextlib import redirect_stdout
from unittest import mock

from tools.cli import main as cli_main
from tools.sota_discovery.engine import (
    SCREEN_IRRELEVANT,
    SCREEN_RELEVANT,
    build_research_plan,
    run_iterative_discovery,
    screen_candidate,
)
from tools.sota_discovery.providers import (
    PROVIDER_REGISTRY,
    ArxivProvider,
    CrossrefProvider,
    GoogleScholarAssistedProvider,
    OpenAlexProvider,
    SemanticScholarProvider,
    UnsupportedProviderOperationError,
    extract_arxiv_id,
    get_provider,
    list_provider_capabilities,
    run_multi_source_search,
)
from tools.sota_discovery.sota import HttpBudgetExhaustedError, HttpRequestTracker, RetrievalError

ARXIV_ATOM_XML = """<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:arxiv="http://arxiv.org/schemas/atom">
  <entry>
    <id>http://arxiv.org/abs/2301.12345v1</id>
    <published>2023-01-29T00:00:00Z</published>
    <title>Graph Neural Networks for Molecular Property Prediction</title>
    <summary>We propose graph neural networks for molecular property prediction on benchmark datasets.</summary>
    <author><name>Alice Researcher</name></author>
    <arxiv:doi>10.48550/arXiv.2301.12345</arxiv:doi>
    <link title="pdf" href="http://arxiv.org/pdf/2301.12345v1" type="application/pdf"/>
    <category term="cs.LG"/>
  </entry>
</feed>
"""


class _FakeResp:
    def __init__(self, body: str | bytes, status: int = 200):
        self.status = status
        self.headers = {}
        self._body = body.encode("utf-8") if isinstance(body, str) else body

    def read(self):
        return self._body

    def __enter__(self):
        return self

    def __exit__(self, *args):
        return False


class MultiSourceProviderTests(unittest.TestCase):
    def test_registry_and_boolean_credential_checks_never_leak_secrets(self):
        self.assertGreaterEqual(len(PROVIDER_REGISTRY), 19)
        with mock.patch.dict(os.environ, {"SEMANTIC_SCHOLAR_API_KEY": "secret-s2-key-999"}):
            rows = list_provider_capabilities()
        by_id = {r["provider_id"]: r for r in rows}
        self.assertTrue(by_id["semantic-scholar"]["credentials_present"]["SEMANTIC_SCHOLAR_API_KEY"])
        serialized = json.dumps(rows)
        self.assertNotIn("secret-s2-key-999", serialized)
        self.assertEqual(by_id["google-scholar-assisted"]["access_modes"], ["MANUAL_ASSISTED", "UNSUPPORTED"])

    def test_arxiv_crossref_s2_and_openalex_adapters_normalize_and_share_tracker(self):
        def fake_open(req, timeout=15.0):
            url = req.full_url
            if "export.arxiv.org" in url:
                return _FakeResp(ARXIV_ATOM_XML)
            if "api.crossref.org" in url:
                return _FakeResp(json.dumps({
                    "message": {
                        "items": [{
                            "DOI": "10.1021/ci.2023.001",
                            "title": ["Molecular Graph Networks in Chemistry"],
                            "author": [{"given": "Bob", "family": "Chemist"}],
                            "published-print": {"date-parts": [[2023]]},
                            "container-title": ["JCIM"],
                            "is-referenced-by-count": 42,
                            "abstract": "<jats:p>Graph neural networks predict molecular solubility.</jats:p>",
                            "type": "journal-article",
                        }]
                    }
                }))
            if "api.semanticscholar.org" in url:
                return _FakeResp(json.dumps({
                    "data": [{
                        "paperId": "s2p1",
                        "externalIds": {"DOI": "10.1021/ci.2023.001", "ArXiv": "2301.12345"},
                        "title": "Molecular Graph Networks in Chemistry",
                        "abstract": "Graph neural networks predict molecular solubility.",
                        "year": 2024,
                        "citationCount": 50,
                        "authors": [{"name": "Bob Chemist"}],
                        "venue": "NeurIPS",
                        "isOpenAccess": True,
                        "openAccessPdf": {"url": "https://example.org/p.pdf"},
                    }]
                }))
            return _FakeResp(json.dumps({"results": []}))

        tracker = HttpRequestTracker(max_http_requests=10)
        providers = [OpenAlexProvider(), ArxivProvider(), CrossrefProvider(), SemanticScholarProvider()]
        with mock.patch("tools.sota_discovery.sota._default_no_redirect_open", side_effect=fake_open):
            hits, errors = run_multi_source_search("graph neural networks", 3, providers, tracker=tracker)
        self.assertEqual(errors, [])
        self.assertEqual(len(hits), 3)
        self.assertEqual(tracker.http_attempts, 4)
        self.assertEqual(tracker.provider_requests, {"openalex": 1, "arxiv": 1, "crossref": 1, "semantic-scholar": 1})

    def test_unsupported_operations_and_google_scholar_assisted_contract(self):
        tracker = HttpRequestTracker(max_http_requests=5)
        arxiv = ArxivProvider()
        with self.assertRaises(UnsupportedProviderOperationError):
            arxiv.expand_citations({}, tracker=tracker)
        gs = GoogleScholarAssistedProvider()
        with self.assertRaises(UnsupportedProviderOperationError):
            gs.search("graph neural networks", 5, tracker=tracker)
        plan = gs.build_assisted_search_plan(["graph neural networks"])
        self.assertFalse(plan["automation_allowed"])
        self.assertIn("scholar.google.com/scholar?q=graph+neural+networks", plan["assisted_links"][0]["search_url"])
        imported = gs.import_json_records([{"title": "Manual Paper", "year": 2022, "doi": "10.1000/x"}])
        self.assertEqual(imported[0]["_provider_source"], "google-scholar-assisted-import")
        with self.assertRaises(UnsupportedProviderOperationError):
            get_provider("scopus")

    def test_rate_limit_429_malformed_response_and_provider_failure_isolation(self):
        def fake_open(req, timeout=15.0):
            url = req.full_url
            if "api.semanticscholar.org" in url:
                raise urllib.error.HTTPError(url, 429, "Too Many Requests", {}, None)
            if "api.crossref.org" in url:
                return _FakeResp("{not-json")
            return _FakeResp(ARXIV_ATOM_XML)

        tracker = HttpRequestTracker(max_http_requests=10)
        providers = [SemanticScholarProvider(), CrossrefProvider(), ArxivProvider()]
        with mock.patch("tools.sota_discovery.sota._default_no_redirect_open", side_effect=fake_open):
            hits, errors = run_multi_source_search("gnn", 2, providers, tracker=tracker)
        self.assertEqual(len(hits), 1)
        self.assertEqual(len(errors), 2)
        self.assertEqual(tracker.rate_limited_responses, 1)
        self.assertEqual(tracker.http_failures, 2)

    def test_global_budget_exhaustion_halts_multi_source_search(self):
        def fake_open(req, timeout=15.0):
            return _FakeResp(ARXIV_ATOM_XML) if "arxiv" in req.full_url else _FakeResp(json.dumps({"results": []}))

        tracker = HttpRequestTracker(max_http_requests=1)
        providers = [OpenAlexProvider(), ArxivProvider()]
        with mock.patch("tools.sota_discovery.sota._default_no_redirect_open", side_effect=fake_open):
            with self.assertRaises(HttpBudgetExhaustedError):
                run_multi_source_search("gnn", 2, providers, tracker=tracker)
        self.assertTrue(tracker.budget_exhausted)
        self.assertEqual(tracker.http_attempts, 1)

    def test_cross_source_dedup_preprint_published_and_conflict_guards(self):
        self.assertEqual(extract_arxiv_id("https://arxiv.org/abs/2301.12345v2"), "2301.12345")
        self.assertEqual(extract_arxiv_id("10.48550/arXiv.2301.12345"), "2301.12345")

        plan = build_research_plan(
            topic="Graph Neural Networks Molecular",
            objective="Compare methods",
            concept_groups={"core_topic": ["graph", "neural"]},
            prefer_review_first=False,
            max_papers=10,
            max_rounds=1,
        )
        # Same work observed as arXiv preprint and Crossref/S2 published version with conflicting year/venue
        w_arxiv = {
            "id": "https://arxiv.org/abs/2301.12345",
            "arxiv_id": "2301.12345",
            "doi": "https://doi.org/10.48550/arXiv.2301.12345",
            "display_name": "Graph Neural Networks for Molecules",
            "publication_year": 2023,
            "primary_location": {"source": {"display_name": "arXiv"}},
            "abstract": "Graph neural networks predict molecular properties.",
            "publication_stage": "preprint",
            "_provider_source": "arxiv",
        }
        w_pub_same_arxiv = {
            "id": "https://www.semanticscholar.org/paper/s2p1",
            "arxiv_id": "2301.12345",
            "doi": "https://doi.org/10.48550/arXiv.2301.12345",
            "display_name": "Graph Neural Networks for Molecules",
            "publication_year": 2024,
            "primary_location": {"source": {"display_name": "NeurIPS"}},
            "abstract": "Graph neural networks predict molecular properties.",
            "publication_stage": "published",
            "_provider_source": "semantic-scholar",
        }
        # Distinct paper with same title but DIFFERENT DOI -> must NOT merge!
        w_distinct_doi = {
            "id": "https://openalex.org/W999",
            "doi": "https://doi.org/10.1000/different.paper",
            "display_name": "Graph Neural Networks for Molecules",
            "publication_year": 2023,
            "primary_location": {"source": {"display_name": "Other"}},
            "abstract": "Graph neural networks predict molecular properties.",
            "_provider_source": "openalex",
        }

        res = run_iterative_discovery(plan, search_fn=lambda q, n: [w_arxiv, w_pub_same_arxiv, w_distinct_doi])
        self.assertEqual(res["state_accounting"]["unique_canonical_identities"], 2)
        self.assertGreaterEqual(res["state_accounting"]["duplicate_observations"], 1)
        merged = next(iter(res["buckets"]["RAW_CANDIDATES"].values()))
        self.assertEqual(set(merged["_providers_observed"]), {"arxiv", "semantic-scholar"})
        self.assertEqual(set(merged["_publication_stages"]), {"preprint", "published"})
        self.assertTrue(any("year_conflict" in c for c in merged["_metadata_conflicts"]))

    def test_f01_corpus_size_f02_query_anchoring_and_f03_screening_precision(self):
        # F02: operational words excluded from objective_focus; generic concepts skipped & anchored
        plan = build_research_plan(
            topic="Graph Neural Networks for Molecular Property Prediction",
            objective="Verify live bibliographic discovery and HTTP accounting",
            max_papers=2,
            max_results_per_query=5,
            max_accepted_papers=2,
            max_rounds=2,
            max_requests=4,
            prefer_review_first=True,
        )
        self.assertNotIn("objective_focus", plan["target_dimensions"])

        # F03: negative control (single generic/overlapping token) vs positive control
        neg_rec = {
            "Title": "Encyclopedia of computer science",
            "_abstract": "A comprehensive reference on computer science and networks.",
            "_concepts": ["Computer science"],
            "Year": 2000,
        }
        pos_rec = {
            "Title": "Neural Message Passing for Quantum Chemistry",
            "_abstract": "We reformulate graph neural networks for molecular property prediction.",
            "_concepts": ["Graph neural network", "Molecular physics"],
            "Year": 2017,
        }
        self.assertEqual(screen_candidate(neg_rec, plan)["outcome"], SCREEN_IRRELEVANT)
        self.assertEqual(screen_candidate(pos_rec, plan)["outcome"], SCREEN_RELEVANT)

        # F01 + F02 combined: review_first skips Workflow/Computer science, anchors Molecular Docking, caps accepted at 2
        rev = {
            "id": "https://openalex.org/W_REV",
            "doi": "https://doi.org/10.1000/rev",
            "display_name": "Review of Graph Neural Networks for Molecular Property Prediction",
            "publication_year": 2023,
            "is_review": True,
            "type": "review",
            "concepts": [{"display_name": "Workflow"}, {"display_name": "Computer science"}, {"display_name": "Molecular Docking"}],
            "abstract": "Survey of graph neural networks for molecular property prediction.",
        }
        p1 = {
            "id": "https://openalex.org/W_P1",
            "doi": "https://doi.org/10.1000/p1",
            "display_name": "Molecular Graph Networks One",
            "publication_year": 2022,
            "abstract": "Graph neural networks for molecular property prediction.",
        }
        p2 = {
            "id": "https://openalex.org/W_P2",
            "doi": "https://doi.org/10.1000/p2",
            "display_name": "Molecular Graph Networks Two",
            "publication_year": 2023,
            "abstract": "Graph neural networks for molecular property prediction.",
        }
        res = run_iterative_discovery(plan, search_fn=lambda q, n: [rev, p1, p2])
        self.assertEqual(res["state_accounting"]["accepted"], 2)
        self.assertEqual(res["state_accounting"]["candidates_for_limited_use"], 1)
        queries = [q["query"] for q in res["executed_queries"]]
        self.assertNotIn("Workflow", queries)
        self.assertNotIn("Computer science", queries)
        self.assertIn("Workflow", res["measurements"]["skipped_generic_concepts"])

    def test_cli_multi_source_end_to_end_with_mocked_transport(self):
        def fake_open(req, timeout=15.0):
            if "export.arxiv.org" in req.full_url:
                return _FakeResp(ARXIV_ATOM_XML)
            return _FakeResp(json.dumps({"results": []}))

        with tempfile.TemporaryDirectory() as tmp:
            out_dir = pathlib.Path(tmp) / "multi_out"
            with mock.patch("tools.sota_discovery.sota._default_no_redirect_open", side_effect=fake_open):
                with redirect_stdout(io.StringIO()):
                    rc = cli_main([
                        "sota",
                        "--topic", "Graph Neural Networks for Molecular Property Prediction",
                        "--objective", "Verify live bibliographic discovery and HTTP accounting",
                        "--iterative",
                        "--sources", "openalex,arxiv",
                        "--max-papers", "5",
                        "--max-rounds", "1",
                        "--max-requests", "2",
                        "--max-http-requests", "6",
                        "--citation-depth", "0",
                        "--out-dir", str(out_dir),
                    ])
            self.assertEqual(rc, 0)
            trace = json.loads((out_dir / "discovery_trace.json").read_text(encoding="utf-8"))
            tel = trace["measurements"]["observable_http_telemetry"]
            self.assertEqual(tel["provider_requests"].get("arxiv"), 2)
            self.assertEqual(tel["provider_requests"].get("openalex"), 2)
            self.assertEqual(trace["state_accounting"]["accepted"], 1)


if __name__ == "__main__":
    unittest.main()
