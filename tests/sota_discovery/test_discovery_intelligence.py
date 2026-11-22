"""Offline regression tests for Increment B discovery intelligence (D01-D15)."""

import json
import pathlib
import tempfile
import unittest

from tools.sota_discovery.engine import (
    RF_APPLICABLE,
    RF_UNAVAILABLE,
    SCREEN_INSUFFICIENT_EVIDENCE,
    SCREEN_IRRELEVANT,
    SCREEN_RELEVANT,
    TERM_BUDGET_EXHAUSTED,
    TERM_INSUFFICIENT_EVIDENCE,
    TERM_PROVIDER_FAILURE,
    TERM_SATURATION,
    TERM_STAGNATION,
    build_research_plan,
    run_iterative_discovery,
    write_discovery_artifacts,
)
from tools.sota_discovery.sota import main as sota_main


def _work(
    wid: str,
    title: str,
    year: int = 2022,
    doi: str | None = None,
    abstract: str = "We propose a quantum reservoir computing method. Experiments show 95% accuracy.",
    authors=("Alice Smith",),
    concepts=("Quantum reservoir computing",),
    wtype: str = "article",
    is_review: bool = False,
):
    return {
        "id": wid,
        "doi": doi or f"https://doi.org/10.1000/{wid.rsplit('/', 1)[-1].lower()}",
        "display_name": title,
        "publication_year": year,
        "cited_by_count": 50,
        "type": wtype,
        "is_review": is_review,
        "abstract": abstract,
        "authorships": [{"author": {"display_name": a}} for a in authors],
        "primary_location": {"source": {"display_name": "Journal of Test"}},
        "concepts": [{"display_name": c} for c in concepts],
    }


class DiscoveryIntelligenceTests(unittest.TestCase):
    def test_invalid_research_plan_inputs_rejected(self):
        with self.assertRaises(ValueError):
            build_research_plan(topic="", objective="Valid")
        with self.assertRaises(ValueError):
            build_research_plan(topic="Valid", objective="   ")
        with self.assertRaises(ValueError):
            build_research_plan(topic="Valid", objective="Valid", max_rounds=0)

    def test_d01_relevant_initial_search_saturates(self):
        plan = build_research_plan(
            topic="Quantum Reservoir Computing",
            objective="Identify methods",
            concept_groups={"core_topic": ["quantum", "reservoir"]},
            prefer_review_first=False,
        )
        w1 = _work("W1", "Quantum Reservoir Computing Basics")
        res = run_iterative_discovery(plan, search_fn=lambda q, n: [w1])
        self.assertEqual(res["termination_reason"], TERM_SATURATION)
        self.assertEqual(res["state_accounting"]["accepted"], 1)

    def test_d02_duplicate_records_across_queries(self):
        plan = build_research_plan(
            topic="Quantum Reservoir Computing",
            objective="Survey",
            seeds=["Echo State"],
            concept_groups={"core_topic": ["quantum"], "seed": ["echo"]},
            prefer_review_first=False,
        )
        w1 = _work("W1", "Quantum Echo State Networks", abstract="Quantum echo state network achieves 92% accuracy.")
        res = run_iterative_discovery(plan, search_fn=lambda q, n: [w1, w1])
        self.assertEqual(res["state_accounting"]["unique_canonical_identities"], 1)
        self.assertGreaterEqual(res["state_accounting"]["duplicate_observations"], 1)

    def test_d03_irrelevant_initial_results_excluded_and_no_coverage_inflation(self):
        plan = build_research_plan(
            topic="Quantum Reservoir Computing",
            objective="Survey",
            concept_groups={"core_topic": ["quantum", "reservoir"]},
            prefer_review_first=False,
            max_rounds=2,
        )
        bad = _work("W_IRR", "Medieval Agriculture Practices", abstract="We study medieval crop rotation.", concepts=("Agriculture",))
        res = run_iterative_discovery(plan, search_fn=lambda q, n: [bad])
        self.assertEqual(res["state_accounting"]["accepted"], 0)
        self.assertEqual(res["state_accounting"]["excluded"], 1)
        self.assertFalse(res["coverage_report"]["coverage_satisfied"])

    def test_d04_and_d05_review_first_applicable_and_unavailable_fallback(self):
        plan = build_research_plan(
            topic="Quantum Computing",
            objective="Survey",
            concept_groups={"core_topic": ["quantum"]},
            prefer_review_first=True,
        )
        rev = _work("W_REV", "A Review of Quantum Computing", wtype="review", is_review=True, concepts=("Spin Qubits",))
        res_app = run_iterative_discovery(plan, search_fn=lambda q, n: [rev])
        self.assertEqual(res_app["review_first_status"], RF_APPLICABLE)
        self.assertTrue(any(q["origin"] == "review_first_concept" for q in res_app["executed_queries"]))

        # Title contains "review" but type is article (not review) -> fallback RF_UNAVAILABLE
        non_rev = _work("W_ART", "Reviewing Quantum Computing Benchmarks", wtype="article", is_review=False)
        res_unavail = run_iterative_discovery(plan, search_fn=lambda q, n: [non_rev])
        self.assertEqual(res_unavail["review_first_status"], RF_UNAVAILABLE)

    def test_d06_and_d07_positive_gain_insufficient_coverage_triggers_refinement(self):
        plan = build_research_plan(
            topic="Quantum Reservoir",
            objective="Compare noise and scaling",
            concept_groups={"core": ["quantum"], "noise_dim": ["decoherence"]},
            prefer_review_first=False,
            max_rounds=3,
        )
        w1 = _work("W1", "Quantum Reservoir Basics", abstract="Quantum reservoir computing baseline.")
        w2 = _work("W2", "Decoherence in Quantum Reservoirs", abstract="Decoherence limits quantum reservoir fidelity.")

        def search_fn(q, n):
            if "decoherence" in q.lower():
                return [w2]
            return [w1]

        res = run_iterative_discovery(plan, search_fn=search_fn)
        self.assertTrue(any(q["origin"] == "refinement:noise_dim" for q in res["executed_queries"]))
        self.assertEqual(res["termination_reason"], TERM_SATURATION)
        self.assertEqual(res["state_accounting"]["accepted"], 2)

    def test_d08_repeated_queries_prevented_and_stagnation(self):
        plan = build_research_plan(
            topic="Quantum Reservoir",
            objective="Survey",
            seeds=["Quantum Reservoir"],  # duplicate of topic
            concept_groups={"core": ["quantum"], "missing": ["photonic"]},
            prefer_review_first=False,
            max_rounds=4,
        )
        w1 = _work("W1", "Quantum Reservoir", abstract="Quantum reservoir only.")
        res = run_iterative_discovery(plan, search_fn=lambda q, n: [w1])
        self.assertGreaterEqual(res["measurements"]["repeated_queries_prevented"], 1)
        self.assertEqual(res["termination_reason"], TERM_STAGNATION)

    def test_d09_bounded_citation_graph_with_cycles(self):
        plan = build_research_plan(
            topic="Quantum Reservoir",
            objective="Survey",
            concept_groups={"core": ["quantum"], "missing": ["spin"]},
            prefer_review_first=False,
            max_rounds=2,
            citation_depth=1,
            max_citation_nodes=2,
        )
        w1 = _work("W1", "Quantum Paper One", abstract="Quantum reservoir computing.")
        w2 = _work("W2", "Spin Quantum Paper Two", abstract="Spin qubits for quantum reservoir.")

        def cite_fn(raw):
            if raw["id"] == "W1":
                return {"backward": [w2], "forward": [w1]}  # self-loop cycle
            return {"backward": [w1], "forward": [w2]}

        res = run_iterative_discovery(plan, search_fn=lambda q, n: [w1], citation_fn=cite_fn)
        self.assertGreaterEqual(res["citation_exploration"]["cycles_prevented"], 1)
        self.assertLessEqual(res["citation_exploration"]["nodes_visited"], 2)
        self.assertEqual(res["state_accounting"]["accepted"], 2)

    def test_d10_budget_exhausted(self):
        plan = build_research_plan(
            topic="Quantum",
            objective="Survey",
            seeds=["SeedA", "SeedB", "SeedC"],
            concept_groups={"core": ["quantum"], "missing": ["photonic"]},
            prefer_review_first=False,
            max_requests=2,
            max_rounds=3,
        )
        w1 = _work("W1", "Quantum Intro", abstract="Quantum intro.")
        res = run_iterative_discovery(plan, search_fn=lambda q, n: [w1])
        self.assertEqual(res["termination_reason"], TERM_BUDGET_EXHAUSTED)
        self.assertLessEqual(res["measurements"]["search_requests"], 2)

    def test_d11_provider_failure_and_partial_results(self):
        plan = build_research_plan(
            topic="Quantum",
            objective="Survey",
            concept_groups={"core": ["quantum"]},
            prefer_review_first=False,
            max_rounds=1,
        )
        def fail_fn(q, n):
            raise RuntimeError("network down")
        res = run_iterative_discovery(plan, search_fn=fail_fn)
        self.assertEqual(res["termination_reason"], TERM_PROVIDER_FAILURE)
        self.assertEqual(res["state_accounting"]["accepted"], 0)

    def test_d12_no_accepted_identities_and_insufficient_evidence(self):
        plan = build_research_plan(
            topic="Quantum",
            objective="Survey",
            concept_groups={"core": ["quantum"]},
            prefer_review_first=False,
            max_rounds=1,
        )
        meta_only = _work("W_META", "Quantum Metadata Only", abstract="")
        res = run_iterative_discovery(plan, search_fn=lambda q, n: [meta_only])
        self.assertEqual(res["state_accounting"]["accepted"], 0)
        self.assertEqual(res["state_accounting"]["candidates_for_limited_use"], 1)
        self.assertEqual(res["termination_reason"], TERM_INSUFFICIENT_EVIDENCE)

    def test_d13_wrong_doi_in_relevant_candidate_not_accepted(self):
        plan = build_research_plan(
            topic="Quantum Reservoir",
            objective="Survey",
            concept_groups={"core": ["quantum"]},
            prefer_review_first=False,
        )
        w_bad_doi = _work("W_BAD", "Quantum Reservoir Paper", doi="https://doi.org/10.1000/wrong")
        res = run_iterative_discovery(
            plan,
            search_fn=lambda q, n: [w_bad_doi],
            verify_doi_fn=lambda d: {"id": "W_OTHER", "display_name": "Completely Different Paper", "publication_year": 1999, "doi": d, "authorships": []},
        )
        self.assertEqual(res["state_accounting"]["accepted"], 0)
        self.assertEqual(res["state_accounting"]["candidates_for_limited_use"], 1)
        with tempfile.TemporaryDirectory() as tmp:
            outs = write_discovery_artifacts(pathlib.Path(tmp), res)
            self.assertEqual((pathlib.Path(tmp) / "references.bib").read_text(encoding="utf-8"), "")
            self.assertIn("CANDIDATE (not accepted)", (pathlib.Path(tmp) / "references_candidates.bib").read_text(encoding="utf-8"))

    def test_d14_and_d15_search_citation_convergence_and_deterministic_repeatability(self):
        plan = build_research_plan(
            topic="Quantum Reservoir",
            objective="Survey",
            concept_groups={"core": ["quantum"], "spin": ["spin"]},
            prefer_review_first=False,
        )
        w1 = _work("W1", "Quantum Reservoir Search", abstract="Quantum reservoir search.")
        w2 = _work("W2", "Spin Reservoir Citation", abstract="Spin reservoir citation.")
        kwargs = dict(
            search_fn=lambda q, n: [w1],
            citation_fn=lambda raw: {"backward": [w2], "forward": []} if raw["id"] == "W1" else {},
        )
        r1 = run_iterative_discovery(plan, **kwargs)
        r2 = run_iterative_discovery(plan, **kwargs)
        self.assertEqual(r1["termination_reason"], r2["termination_reason"])
        self.assertEqual(r1["state_accounting"], r2["state_accounting"])
        self.assertEqual(sorted(r1["buckets"]["ACCEPTED_CORPUS"].keys()), sorted(r2["buckets"]["ACCEPTED_CORPUS"].keys()))

    def test_cli_iterative_end_to_end_with_fixture(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp_p = pathlib.Path(tmp)
            fx = tmp_p / "fx.json"
            fx.write_text(json.dumps({"results": [_work("W1", "Quantum Reservoir Computing Paper")]}), encoding="utf-8")
            out_dir = tmp_p / "out"
            rc = sota_main([
                "--topic", "Quantum Reservoir Computing",
                "--objective", "Survey methods",
                "--iterative",
                "--fixture", str(fx),
                "--out-dir", str(out_dir),
            ])
            self.assertEqual(rc, 0)
            for fn in ["research_plan.json", "discovery_trace.json", "screening_decisions.csv", "coverage_report.json", "references.bib", "references_candidates.bib", "sota_report.md"]:
                self.assertTrue((out_dir / fn).is_file(), f"Missing {fn}")


if __name__ == "__main__":
    unittest.main()
