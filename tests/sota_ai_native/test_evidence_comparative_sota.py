"""Offline regression suite (E01-E20 + M022-R01..R06 + Sec11) for Increment C."""

from __future__ import annotations

import csv
import json
import tempfile
import unittest
from pathlib import Path

from tools.cli import main as cli_main
from tools.sota_evidence import (
    COMP_CONDITIONAL,
    COMP_DIRECT,
    COMP_NOT,
    CORR_EXACT,
    CORR_FUZZY,
    CORR_INVALID_LOCATOR,
    CORR_NEEDS_REVIEW,
    CORR_NOT_CHECKED,
    CORR_SOURCE_MISMATCH,
    CORR_UNMATCHED,
    GAP_AUTHOR_LIM,
    GAP_INSUFFICIENT,
    GAP_MISSING_COMP,
    QM_FAIL,
    QM_INSUFFICIENT,
    QM_NOT_MEASURED,
    QM_PASS,
    REPORT_EXPLORATORY,
    REPORT_GROUNDED,
    acquire_evidence,
    assess_research_gaps,
    compare_papers,
    evaluate_tier1_quality,
    normalize_extraction,
    register_claims,
    run_evidence_and_comparative_pipeline,
    verify_textual_correspondence,
    verify_textual_correspondence_detailed,
)
from tools.sota_integrity import (
    BIB_STATUS_MISMATCH,
    BIB_STATUS_VERIFIED,
    CLAIM_NOT_CHECKED,
    CLAIM_NOT_VERIFIED,
    CLAIM_PARTIALLY_SUPPORTED,
    CLAIM_SUPPORTED,
    CLAIM_UNSUPPORTED,
    EVIDENCE_ABSTRACT_BASED,
    EVIDENCE_FULL_TEXT,
    EVIDENCE_METADATA_ONLY,
    EVIDENCE_MODEL_RECALL,
    EVIDENCE_UNAVAILABLE,
)


def _synth_assess(ref: str = "synthetic_check#1") -> dict:
    return {
        "status": "VERIFIED",
        "method": "synthetic_offline_control",
        "verifier": "test_harness_synthetic",
        "evidence_ref": ref,
    }


def _paper(
    pid: str,
    *,
    title: str = "Paper Title",
    year: int = 2024,
    doi: str = "",
    bib_status: str = BIB_STATUS_VERIFIED,
    full_text: str = "",
    abstract: str = "",
    provider: str = "openalex",
    research_problem: str = "molecular property prediction",
    methodology: str = "GraphNet",
    datasets_or_materials: str = "QM9",
    evaluation_protocol: str = "standard-split-v1",
    experimental_conditions: str = "default-hyperparams",
    baselines: str = "SchNet",
    environment: str = "PyTorch 2.x",
    results: str = "0.012 MAE",
    reported_metrics: list[dict] | None = None,
    author_limitations: str = "Evaluated on small molecules only.",
    claims: list[dict] | None = None,
) -> dict:
    return {
        "paper_id": pid,
        "Title": title,
        "Authors": ["Ada Lovelace", "Alan Turing"],
        "Year": year,
        "DOI or source URL": doi or f"https://doi.org/10.1000/{pid}",
        "Venue": "NeurIPS",
        "Bibliographic status": bib_status,
        "provider": provider,
        "full_text": full_text,
        "abstract": abstract,
        "extraction": {
            "research_problem": research_problem,
            "proposed_contributions": f"{methodology} architecture",
            "methodology": methodology,
            "datasets_or_materials": datasets_or_materials,
            "evaluation_protocol": evaluation_protocol,
            "baselines": baselines,
            "experimental_conditions": experimental_conditions,
            "environment": environment,
            "results": results,
            "author_limitations": author_limitations,
            "reported_metrics": reported_metrics if reported_metrics is not None else [{"name": "MAE", "value": 0.012, "unit": "eV"}],
        },
        "claims": claims if claims is not None else [],
    }


def _gate_map(metrics: list[dict]) -> dict[str, dict]:
    return {m["metric_id"]: m for m in metrics}


class EvidenceComparativeSotaTests(unittest.TestCase):
    def test_e01_full_text_local_acquisition(self):
        p = _paper("p1", full_text="GraphNet achieves 0.012 MAE on QM9.", abstract="Short abstract.")
        ev = acquire_evidence(p)
        self.assertEqual(ev["availability_status"], EVIDENCE_FULL_TEXT)
        self.assertIn("0.012 MAE", ev["_text"])
        self.assertTrue(ev["content_hash"])

    def test_e02_abstract_only_no_silent_full_text_fallback(self):
        p = _paper("p2", full_text="", abstract="We evaluate on QM9 with MAE 0.015.")
        ev = acquire_evidence(p)
        self.assertEqual(ev["availability_status"], EVIDENCE_ABSTRACT_BASED)
        self.assertIn("0.015", ev["_text"])

    def test_e03_metadata_only_and_unavailable_acquisition(self):
        p_meta = _paper("p3a", full_text="", abstract="", provider="openalex")
        ev_meta = acquire_evidence(p_meta)
        self.assertEqual(ev_meta["availability_status"], EVIDENCE_METADATA_ONLY)

        p_unavail = _paper("p3b", full_text="", abstract="", provider="manual")
        p_unavail["Title"] = ""
        p_unavail["DOI or source URL"] = ""
        ev_unavail = acquire_evidence(p_unavail)
        self.assertEqual(ev_unavail["availability_status"], EVIDENCE_UNAVAILABLE)

    def test_e04_model_recall_abstention(self):
        p = _paper(
            "p4",
            provider="copilot-cli",
            full_text="",
            abstract="",
            claims=[{
                "claim_id": "c4",
                "claim_text": "Method X beats Y",
                "evidence_excerpt": "Method X beats Y",
                "evidence_locator": "section:Recall",
                "claim_support_status": CLAIM_SUPPORTED,
            }],
        )
        p["model_recall_summary"] = "Model recalled summary."
        ev = acquire_evidence(p)
        self.assertEqual(ev["availability_status"], EVIDENCE_MODEL_RECALL)
        claims = register_claims([p], {"p4": ev})
        self.assertEqual(len(claims), 1)
        self.assertEqual(claims[0]["claim_support_status"], CLAIM_NOT_VERIFIED)

    def test_e05_extraction_normalization_preserves_absent_values(self):
        p = _paper(
            "p5",
            full_text="Study text.",
            datasets_or_materials="",
            evaluation_protocol="",
            reported_metrics=[],
        )
        ev = acquire_evidence(p)
        ext = normalize_extraction(p, ev)
        self.assertEqual(ext["datasets_or_materials"], "NOT_VERIFIED")
        self.assertEqual(ext["evaluation_protocol"], "NOT_VERIFIED")
        self.assertIn("datasets_or_materials", ext["missing_information"])
        self.assertIn("reported_metrics", ext["missing_information"])

    def test_e06_textual_correspondence_exact_match_and_locator(self):
        text = "Section 1: Intro.\nGraphNet achieves 0.012 MAE on QM9 under standard-split-v1."
        p = _paper("p6", full_text=text)
        ev = acquire_evidence(p)
        corr, score = verify_textual_correspondence(
            "GraphNet achieves 0.012 MAE on QM9 under standard-split-v1.",
            "page:1",
            ev,
            claim_paper_id="p6",
        )
        self.assertEqual(corr, CORR_EXACT)
        self.assertEqual(score, 1.0)

    def test_e07_textual_correspondence_thresholds_fuzzy_review_unmatched(self):
        text = "GraphNet achieves 0.012 MAE on the QM9 benchmark dataset under standard split."
        p = _paper("p7", full_text=text)
        ev = acquire_evidence(p)

        corr_f, score_f = verify_textual_correspondence(
            "GraphNet achieves 0.012 MAE on the QM9 benchmark dataset under the standard split",
            "section:Results",
            ev,
            claim_paper_id="p7",
        )
        self.assertEqual(corr_f, CORR_FUZZY)
        self.assertGreaterEqual(score_f, 0.85)

        corr_r, score_r = verify_textual_correspondence(
            "GraphNet achieves 0.012 MAE on QM9 dataset under evaluation",
            "section:Results",
            ev,
            claim_paper_id="p7",
        )
        self.assertEqual(corr_r, CORR_NEEDS_REVIEW)
        self.assertGreaterEqual(score_r, 0.50)
        self.assertLess(score_r, 0.85)

        corr_u, score_u = verify_textual_correspondence(
            "Completely unrelated quantum chromodynamics lattice simulation result.",
            "section:Results",
            ev,
            claim_paper_id="p7",
        )
        self.assertEqual(corr_u, CORR_UNMATCHED)
        self.assertLess(score_u, 0.50)

    def test_e08_textual_correspondence_invalid_locator_and_source_mismatch(self):
        p = _paper("p8", full_text="GraphNet achieves 0.012 MAE on QM9.")
        ev = acquire_evidence(p)

        corr_inv, _ = verify_textual_correspondence(
            "GraphNet achieves 0.012 MAE on QM9.",
            "page:-1",
            ev,
            claim_paper_id="p8",
        )
        self.assertEqual(corr_inv, CORR_INVALID_LOCATOR)

        corr_mis, _ = verify_textual_correspondence(
            "GraphNet achieves 0.012 MAE on QM9.",
            "section:Results",
            ev,
            claim_paper_id="other_paper",
        )
        self.assertEqual(corr_mis, CORR_SOURCE_MISMATCH)

    def test_e09_correspondence_never_auto_promotes_to_supported(self):
        p = _paper(
            "p9",
            full_text="GraphNet achieves 0.012 MAE on QM9.",
            claims=[
                {
                    "claim_id": "c9",
                    "claim_text": "GraphNet achieves 0.012 MAE on QM9.",
                    "evidence_excerpt": "GraphNet achieves 0.012 MAE on QM9.",
                    "evidence_locator": "section:Results",
                    "claim_support_status": CLAIM_NOT_CHECKED,
                }
            ],
        )
        ev = acquire_evidence(p)
        claims = register_claims([p], {"p9": ev})
        self.assertEqual(claims[0]["textual_correspondence_status"], CORR_EXACT)
        self.assertEqual(claims[0]["claim_support_status"], CLAIM_NOT_CHECKED)

    def test_e10_pairwise_directly_comparable(self):
        t1 = "molecular property prediction M1 M1 architecture QM9 standard-split-v1 SchNet default-hyperparams PyTorch 2.x 0.012 MAE Evaluated on small molecules only."
        t2 = "molecular property prediction M2 M2 architecture QM9 standard-split-v1 SchNet default-hyperparams PyTorch 2.x 0.012 MAE Evaluated on small molecules only."
        p1 = _paper("p10a", full_text=t1, methodology="M1")
        p2 = _paper("p10b", full_text=t2, methodology="M2")
        evs = {"p10a": acquire_evidence(p1), "p10b": acquire_evidence(p2)}
        exts = {"p10a": normalize_extraction(p1, evs["p10a"]), "p10b": normalize_extraction(p2, evs["p10b"])}
        comps = compare_papers([p1, p2], exts, evs)
        self.assertEqual(len(comps), 1)
        self.assertEqual(comps[0]["comparability_class"], COMP_DIRECT)

    def test_e11_pairwise_conditionally_comparable(self):
        t1 = "molecular property prediction GraphNet GraphNet architecture QM9 standard-split-v1 SchNet batch_size=32 PyTorch 2.x 0.012 MAE Evaluated on small molecules only."
        t2 = "molecular property prediction GraphNet GraphNet architecture QM9 standard-split-v1 SchNet batch_size=128 PyTorch 2.x 0.012 MAE Evaluated on small molecules only."
        p1 = _paper("p11a", full_text=t1, experimental_conditions="batch_size=32")
        p2 = _paper("p11b", full_text=t2, experimental_conditions="batch_size=128")
        evs = {"p11a": acquire_evidence(p1), "p11b": acquire_evidence(p2)}
        exts = {"p11a": normalize_extraction(p1, evs["p11a"]), "p11b": normalize_extraction(p2, evs["p11b"])}
        comps = compare_papers([p1, p2], exts, evs)
        self.assertEqual(len(comps), 1)
        self.assertEqual(comps[0]["comparability_class"], COMP_CONDITIONAL)

    def test_e12_pairwise_not_comparable(self):
        p1 = _paper(
            "p12a",
            full_text="A",
            datasets_or_materials="PDBbind",
            reported_metrics=[{"name": "RMSE", "value": 1.2, "unit": "kcal/mol"}],
        )
        p2 = _paper(
            "p12b",
            full_text="B",
            datasets_or_materials="QM9",
            reported_metrics=[{"name": "MAE", "value": 0.012, "unit": "eV"}],
        )
        evs = {"p12a": acquire_evidence(p1), "p12b": acquire_evidence(p2)}
        exts = {"p12a": normalize_extraction(p1, evs["p12a"]), "p12b": normalize_extraction(p2, evs["p12b"])}
        comps = compare_papers([p1, p2], exts, evs)
        self.assertEqual(len(comps), 1)
        self.assertEqual(comps[0]["comparability_class"], COMP_NOT)
        self.assertTrue(comps[0]["reasons"])

    def test_e13_research_gap_taxonomic_separation(self):
        p1 = _paper("p13a", full_text="A", author_limitations="Small sample size on rare scaffolds.")
        p2 = _paper(
            "p13b",
            full_text="B",
            datasets_or_materials="PDBbind",
            reported_metrics=[{"name": "RMSE", "value": 1.2, "unit": "kcal/mol"}],
            author_limitations="Evaluated only on rigid receptors.",
        )
        evs = {"p13a": acquire_evidence(p1), "p13b": acquire_evidence(p2)}
        exts = {"p13a": normalize_extraction(p1, evs["p13a"]), "p13b": normalize_extraction(p2, evs["p13b"])}
        claims = register_claims([p1, p2], evs)
        comps = compare_papers([p1, p2], exts, evs)
        gaps = assess_research_gaps([p1, p2], exts, claims, comps)
        types = {g["gap_type"] for g in gaps}
        self.assertIn(GAP_AUTHOR_LIM, types)
        self.assertIn(GAP_MISSING_COMP, types)

    def test_e14_tier1_bib_gates_qm_b02_b03_b04(self):
        p_ok = _paper(
            "p14a",
            bib_status=BIB_STATUS_VERIFIED,
            full_text="Exact claim text.",
            claims=[{
                "claim_id": "c1",
                "claim_text": "Exact claim text.",
                "evidence_excerpt": "Exact claim text.",
                "evidence_locator": "section:Res",
                "claim_support_status": CLAIM_SUPPORTED,
                "assessment_evidence": _synth_assess("p14a#c1"),
            }],
        )
        p_bad = _paper(
            "p14b",
            bib_status=BIB_STATUS_MISMATCH,
            full_text="Unverified paper.",
            claims=[{
                "claim_id": "c2",
                "claim_text": "Bad claim.",
                "evidence_excerpt": "Bad claim.",
                "evidence_locator": "section:Res",
            }],
        )
        evs = {"p14a": acquire_evidence(p_ok), "p14b": acquire_evidence(p_bad)}
        claims = register_claims([p_ok, p_bad], evs)
        metrics, _ = evaluate_tier1_quality([p_ok, p_bad], [p_ok, p_bad], claims, [], None)
        gates = _gate_map(metrics)
        self.assertEqual(gates["QM-B02"]["gate_outcome"], QM_FAIL)
        self.assertEqual(gates["QM-B03"]["gate_outcome"], QM_FAIL)
        self.assertEqual(gates["QM-B04"]["gate_outcome"], QM_PASS)

    def test_e15_tier1_numerical_gate_qm_e03(self):
        p = _paper(
            "p15",
            full_text="GraphNet achieves 0.012 MAE.",
            claims=[{
                "claim_id": "c15",
                "claim_type": "numerical",
                "claim_text": "GraphNet achieves 0.012 MAE.",
                "evidence_excerpt": "GraphNet achieves 0.012 MAE.",
                "evidence_locator": "section:Res",
                "claim_support_status": CLAIM_SUPPORTED,
                "assessment_evidence": _synth_assess("p15#c15"),
            }],
        )
        evs = {"p15": acquire_evidence(p)}
        exts = {"p15": normalize_extraction(p, evs["p15"])}
        claims = register_claims([p], evs)
        m_un, _ = evaluate_tier1_quality([p], [p], claims, [], None, numerical_checks_executed=False, extractions=exts, ev_by_id=evs)
        self.assertEqual(_gate_map(m_un)["QM-E03"]["measurement_status"], QM_NOT_MEASURED)

        m_ok, _ = evaluate_tier1_quality([p], [p], claims, [], None, numerical_checks_executed=True, extractions=exts, ev_by_id=evs)
        self.assertEqual(_gate_map(m_ok)["QM-E03"]["gate_outcome"], QM_PASS)
        self.assertEqual(len(_gate_map(m_ok)["QM-E03"]["audit_records"]), 1)

    def test_e16_tier1_gap_comparability_and_leakage_gates(self):
        p1 = _paper(
            "p16a",
            full_text="Text 1.",
            claims=[{
                "claim_id": "c1",
                "claim_text": "Text 1.",
                "evidence_excerpt": "Text 1.",
                "evidence_locator": "section:Res",
                "claim_support_status": CLAIM_SUPPORTED,
                "assessment_evidence": _synth_assess("p16a#c1"),
            }],
        )
        p2 = _paper(
            "p16b",
            full_text="Text 2.",
            claims=[{
                "claim_id": "c2",
                "claim_text": "Text 2.",
                "evidence_excerpt": "Text 2.",
                "evidence_locator": "section:Res",
                "claim_support_status": CLAIM_SUPPORTED,
                "assessment_evidence": _synth_assess("p16b#c2"),
            }],
        )
        evs = {"p16a": acquire_evidence(p1), "p16b": acquire_evidence(p2)}
        exts = {"p16a": normalize_extraction(p1, evs["p16a"]), "p16b": normalize_extraction(p2, evs["p16b"])}
        claims = register_claims([p1, p2], evs)
        comps = compare_papers([p1, p2], exts, evs)
        metrics, verdict = evaluate_tier1_quality([p1, p2], [p1, p2], claims, comps, None, extractions=exts, ev_by_id=evs)
        gates = _gate_map(metrics)
        for gid in ("QM-B02", "QM-B03", "QM-B04", "QM-E03", "QM-G04", "QM-G05", "QM-C03"):
            self.assertEqual(gates[gid]["gate_outcome"], QM_PASS, f"Expected {gid} to pass")
        self.assertEqual(verdict, QM_PASS)

    def test_e17_bounded_remediation_regenerates_downstream_artifacts(self):
        bundle = {
            "topic": "Graph Neural Networks for Molecular Properties",
            "papers": [
                _paper(
                    "p17a",
                    full_text="GraphNet achieves 0.012 MAE on QM9.",
                    claims=[{
                        "claim_id": "c17a",
                        "claim_text": "GraphNet achieves 0.012 MAE on QM9.",
                        "evidence_excerpt": "GraphNet achieves 0.012 MAE on QM9.",
                        "evidence_locator": "section:Results",
                        "claim_support_status": CLAIM_SUPPORTED,
                        "assessment_evidence": _synth_assess("p17a#c17a"),
                    }],
                ),
                _paper(
                    "p17b",
                    full_text="EquiNet achieves 0.010 MAE on QM9.",
                    claims=[{
                        "claim_id": "c17b",
                        "claim_text": "EquiNet achieves 0.010 MAE on QM9.",
                        "evidence_excerpt": "EquiNet achieves 0.010 MAE on QM9.",
                        "evidence_locator": "section:Results",
                        "claim_support_status": CLAIM_SUPPORTED,
                        "assessment_evidence": _synth_assess("p17b#c17b"),
                    }],
                ),
            ],
        }
        bundle["papers"][1]["claims"].append({
            "claim_id": "c17_bad",
            "claim_text": "Unsupported claim",
            "evidence_excerpt": "Completely fabricated quote about quantum gravity in organic chemistry.",
            "evidence_locator": "section:Results",
            "claim_support_status": CLAIM_SUPPORTED,
        })
        res = run_evidence_and_comparative_pipeline(bundle, max_remediation_attempts=2)
        self.assertGreaterEqual(len(res["remediation_trace"]), 1)
        bad = [c for c in res["quarantined_claims"] if c["claim_id"] == "c17_bad"][0]
        self.assertEqual(bad["claim_support_status"], CLAIM_UNSUPPORTED)
        self.assertTrue(bad.get("remediated"))
        self.assertEqual(res["quality_verdict"], QM_PASS)

    def test_e18_report_qualification_exploratory_map(self):
        bundle = {
            "topic": "Sparse Topic",
            "papers": [
                _paper("p18", full_text="", abstract="", provider="openalex", bib_status=BIB_STATUS_VERIFIED),
            ],
        }
        res = run_evidence_and_comparative_pipeline(bundle)
        self.assertEqual(res["report_qualification"], REPORT_EXPLORATORY)

    def test_e19_positive_synthetic_control_evidence_grounded_sota(self):
        bundle = {
            "topic": "QM9 Molecular Property Prediction",
            "papers": [
                _paper(
                    "p19a",
                    full_text="GraphNet achieves 0.012 MAE on QM9.",
                    claims=[{
                        "claim_id": "c19a",
                        "claim_text": "GraphNet achieves 0.012 MAE on QM9.",
                        "evidence_excerpt": "GraphNet achieves 0.012 MAE on QM9.",
                        "evidence_locator": "section:Results",
                        "claim_support_status": CLAIM_SUPPORTED,
                        "assessment_evidence": _synth_assess("p19a#c19a"),
                    }],
                ),
                _paper(
                    "p19b",
                    full_text="EquiNet achieves 0.010 MAE on QM9.",
                    claims=[{
                        "claim_id": "c19b",
                        "claim_text": "EquiNet achieves 0.010 MAE on QM9.",
                        "evidence_excerpt": "EquiNet achieves 0.010 MAE on QM9.",
                        "evidence_locator": "section:Results",
                        "claim_support_status": CLAIM_SUPPORTED,
                        "assessment_evidence": _synth_assess("p19b#c19b"),
                    }],
                ),
            ],
        }
        res = run_evidence_and_comparative_pipeline(bundle)
        self.assertEqual(res["report_qualification"], REPORT_GROUNDED)
        self.assertEqual(res["quality_verdict"], QM_PASS)

    def test_e20_cli_end_to_end_outputs(self):
        bundle = {
            "topic": "End-to-End Evidence SOTA",
            "papers": [
                _paper(
                    "p20a",
                    full_text="Model A scores 0.88 F1 on BioNLP.",
                    datasets_or_materials="BioNLP",
                    reported_metrics=[{"name": "F1", "value": 0.88, "unit": "ratio"}],
                    claims=[{
                        "claim_id": "c20a",
                        "claim_text": "Model A scores 0.88 F1 on BioNLP.",
                        "evidence_excerpt": "Model A scores 0.88 F1 on BioNLP.",
                        "evidence_locator": "section:Results",
                        "claim_support_status": CLAIM_SUPPORTED,
                        "assessment_evidence": _synth_assess("p20a#c20a"),
                    }],
                ),
                _paper(
                    "p20b",
                    full_text="Model B scores 0.91 F1 on BioNLP.",
                    datasets_or_materials="BioNLP",
                    reported_metrics=[{"name": "F1", "value": 0.91, "unit": "ratio"}],
                    claims=[{
                        "claim_id": "c20b",
                        "claim_text": "Model B scores 0.91 F1 on BioNLP.",
                        "evidence_excerpt": "Model B scores 0.91 F1 on BioNLP.",
                        "evidence_locator": "section:Results",
                        "claim_support_status": CLAIM_SUPPORTED,
                        "assessment_evidence": _synth_assess("p20b#c20b"),
                    }],
                ),
            ],
        }
        with tempfile.TemporaryDirectory() as tmp:
            in_path = Path(tmp) / "bundle.json"
            out_dir = Path(tmp) / "out"
            in_path.write_text(json.dumps(bundle), encoding="utf-8")
            rc = cli_main(["evidence", "--input", str(in_path), "--out-dir", str(out_dir)])
            self.assertEqual(rc, 0)

            for name in (
                "evidence_manifest.json",
                "claims.json",
                "comparability_assessments.json",
                "research_gaps.json",
                "quality_measurements.json",
                "remediation_trace.json",
                "literature_matrix.csv",
                "references.bib",
                "references_candidates.bib",
                "sota_report.md",
            ):
                self.assertTrue((out_dir / name).is_file(), f"Missing output {name}")

            with (out_dir / "literature_matrix.csv").open(encoding="utf-8") as fh:
                rows = list(csv.DictReader(fh))
            self.assertEqual(len(rows), 2)
            q_data = json.loads((out_dir / "quality_measurements.json").read_text(encoding="utf-8"))
            # Imported JSON files via CLI cannot self-certify SUPPORTED without local execution
            self.assertEqual(q_data["verdict"], QM_INSUFFICIENT)

    # --- M022A Targeted Integrity Corrections (M022-R01 .. M022-R06 + Sec 11) ---

    def test_r01_not_checked_and_caller_supported_never_yield_grounded_pass(self):
        # Unassessed claims (NOT_CHECKED) even with EXACT_MATCH must NOT yield PASS
        bundle_unassessed = {
            "topic": "M022 Sample Reproduction",
            "papers": [
                _paper(
                    "g1",
                    full_text="EquiMP achieves 0.010 eV MAE on QM9.",
                    claims=[{
                        "claim_id": "C1",
                        "claim_type": "numerical",
                        "claim_text": "EquiMP achieves 0.010 eV MAE on QM9.",
                        "evidence_excerpt": "EquiMP achieves 0.010 eV MAE on QM9.",
                        "evidence_locator": "section:Results",
                        "claim_support_status": CLAIM_NOT_CHECKED,
                    }],
                ),
            ],
        }
        res_un = run_evidence_and_comparative_pipeline(bundle_unassessed)
        self.assertEqual(res_un["report_qualification"], REPORT_EXPLORATORY)
        self.assertNotEqual(res_un["quality_verdict"], QM_PASS)

        # Caller-supplied SUPPORTED without assessment_evidence is clamped to NOT_VERIFIED
        bundle_caller = {
            "topic": "Caller Supplied Supported",
            "papers": [
                _paper(
                    "g2",
                    full_text="EquiMP achieves 0.010 eV MAE on QM9.",
                    claims=[{
                        "claim_id": "C2",
                        "claim_text": "EquiMP achieves 0.010 eV MAE on QM9.",
                        "evidence_excerpt": "EquiMP achieves 0.010 eV MAE on QM9.",
                        "evidence_locator": "section:Results",
                        "claim_support_status": CLAIM_SUPPORTED,
                    }],
                ),
            ],
        }
        res_caller = run_evidence_and_comparative_pipeline(bundle_caller)
        self.assertEqual(res_caller["claims"][0]["claim_support_status"], CLAIM_NOT_VERIFIED)
        self.assertEqual(res_caller["report_qualification"], REPORT_EXPLORATORY)
        self.assertNotEqual(res_caller["quality_verdict"], QM_PASS)

        # PARTIALLY_SUPPORTED also blocks EVIDENCE_GROUNDED_COMPARATIVE_SOTA
        bundle_partial = {
            "topic": "Partial Support",
            "papers": [
                _paper(
                    "g3",
                    full_text="EquiMP achieves 0.010 eV MAE on QM9.",
                    claims=[{
                        "claim_id": "C3",
                        "claim_text": "EquiMP achieves 0.010 eV MAE on QM9.",
                        "evidence_excerpt": "EquiMP achieves 0.010 eV MAE on QM9.",
                        "evidence_locator": "section:Results",
                        "claim_support_status": CLAIM_PARTIALLY_SUPPORTED,
                    }],
                ),
            ],
        }
        res_part = run_evidence_and_comparative_pipeline(bundle_partial)
        self.assertEqual(res_part["report_qualification"], REPORT_EXPLORATORY)
        self.assertNotEqual(res_part["quality_verdict"], QM_PASS)

    def test_r02_field_level_provenance_rejects_injected_absent_fields(self):
        p = _paper(
            "p_r02",
            full_text="EquiMP achieves 0.010 eV MAE on QM9.",
            methodology="Injected Secret Transformer",
            environment="8x H100 Cluster",
            baselines="Injected Baseline",
        )
        ev = acquire_evidence(p)
        self.assertEqual(ev["document_completeness"], "PARTIAL_EXCERPT")
        ext = normalize_extraction(p, ev)
        self.assertNotEqual(ext["provenance"], "author_reported")
        self.assertEqual(ext["field_provenance"]["methodology"]["extraction_origin"], "unverified_prepopulated_field")
        self.assertEqual(ext["field_provenance"]["environment"]["extraction_origin"], "unverified_prepopulated_field")

    def test_r03_locator_verification_actual_span_page_section_and_mismatch(self):
        structured_text = (
            "[page 1]\n"
            "Section 1: Introduction\n"
            "We study molecular graphs.\n"
            "[page 2]\n"
            "Section 2: Results\n"
            "EquiMP achieves 0.010 eV MAE on QM9."
        )
        p = _paper("p_r03", full_text=structured_text)
        ev = acquire_evidence(p)

        # Valid quote at correct page & section
        d_ok_page = verify_textual_correspondence_detailed("EquiMP achieves 0.010 eV MAE on QM9.", "page:2", ev)
        self.assertEqual(d_ok_page["correspondence_status"], CORR_EXACT)
        self.assertTrue(d_ok_page["location_verified"])

        d_ok_sec = verify_textual_correspondence_detailed("EquiMP achieves 0.010 eV MAE on QM9.", "section:Results", ev)
        self.assertEqual(d_ok_sec["correspondence_status"], CORR_EXACT)
        self.assertTrue(d_ok_sec["location_verified"])

        # Valid quote at DIFFERENT page/section
        d_wrong_page = verify_textual_correspondence_detailed("EquiMP achieves 0.010 eV MAE on QM9.", "page:1", ev)
        self.assertEqual(d_wrong_page["correspondence_status"], CORR_UNMATCHED)
        self.assertFalse(d_wrong_page["location_verified"])
        self.assertEqual(d_wrong_page["global_similarity"], 1.0)

        # Nonexistent page & nonexistent section
        d_no_page = verify_textual_correspondence_detailed("EquiMP achieves 0.010 eV MAE on QM9.", "page:99", ev)
        self.assertEqual(d_no_page["correspondence_status"], CORR_INVALID_LOCATOR)
        d_no_sec = verify_textual_correspondence_detailed("EquiMP achieves 0.010 eV MAE on QM9.", "section:AppendixZ", ev)
        self.assertEqual(d_no_sec["correspondence_status"], CORR_INVALID_LOCATOR)

        # Unsupported locator format
        d_unsup = verify_textual_correspondence_detailed("EquiMP", "figure:3", ev)
        self.assertEqual(d_unsup["correspondence_status"], CORR_INVALID_LOCATOR)

        # Deterministic char: and line: spans on unstructured text
        plain = "Line one text.\nLine two target 0.010 MAE."
        ev_plain = acquire_evidence(_paper("p_plain", full_text=plain))
        d_line = verify_textual_correspondence_detailed("Line two target 0.010 MAE.", "line:2", ev_plain)
        self.assertEqual(d_line["correspondence_status"], CORR_EXACT)
        self.assertTrue(d_line["location_verified"])
        d_char = verify_textual_correspondence_detailed("Line one text.", "char:0-14", ev_plain)
        self.assertEqual(d_char["correspondence_status"], CORR_EXACT)
        self.assertTrue(d_char["location_verified"])

    def test_r04_comparative_and_gap_integrity_negative_controls(self):
        p1 = _paper(
            "c1",
            full_text="Paper 1 text.",
            reported_metrics=[{"name": "Accuracy", "definition": "top-1", "value": True, "unit": "%"}],
        )
        p2 = _paper(
            "c2",
            full_text="Paper 2 text.",
            reported_metrics=[{"name": "Accuracy", "definition": "macro-f1-equiv", "value": 90.0, "unit": "%"}],
        )
        evs = {"c1": acquire_evidence(p1), "c2": acquire_evidence(p2)}
        exts = {"c1": normalize_extraction(p1, evs["c1"]), "c2": normalize_extraction(p2, evs["c2"])}
        comps = compare_papers([p1, p2], exts, evs)
        self.assertEqual(comps[0]["comparability_class"], COMP_NOT)
        self.assertFalse(comps[0]["global_ranking_permitted"])
        self.assertTrue(any("ambiguous_metric_definition" in r for r in comps[0]["reasons"]))

        gaps = assess_research_gaps([p1, p2], exts, [], comps)
        auth_gaps = [g for g in gaps if g["gap_type"] == GAP_AUTHOR_LIM]
        self.assertTrue(all(g["provisional"] for g in auth_gaps))

    def test_r05_numerical_audit_and_abstention_not_penalized(self):
        p1 = _paper("n1", full_text="A", datasets_or_materials="QM9", reported_metrics=[{"name": "MAE", "value": 0.012, "unit": "eV"}])
        p2 = _paper("n2", full_text="B", datasets_or_materials="PDBbind", reported_metrics=[{"name": "RMSE", "value": 1.2, "unit": "kcal/mol"}])
        evs = {"n1": acquire_evidence(p1), "n2": acquire_evidence(p2)}
        exts = {"n1": normalize_extraction(p1, evs["n1"]), "n2": normalize_extraction(p2, evs["n2"])}
        comps = compare_papers([p1, p2], exts, evs)
        self.assertEqual(comps[0]["comparability_class"], COMP_NOT)
        metrics, _ = evaluate_tier1_quality([p1, p2], [p1, p2], [], comps, None, extractions=exts, ev_by_id=evs)
        gates = _gate_map(metrics)
        # Abstaining (NOT_COMPARABLE) is NOT penalized by QM-C03
        self.assertEqual(gates["QM-C03"]["gate_outcome"], QM_PASS)

    def test_r06_interoperability_sota_iterative_and_sota_ai_to_evidence(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp_p = Path(tmp)
            # Path A: tako sota --iterative --fixture <fx.json> -> tako evidence --from-workflow-dir
            fx = tmp_p / "fx.json"
            fx.write_text(json.dumps({
                "results": [{
                    "id": "https://openalex.org/W1",
                    "doi": "https://doi.org/10.1000/gnn.1",
                    "title": "Graph Neural Networks Survey",
                    "publication_year": 2024,
                    "primary_location": {"source": {"display_name": "NeurIPS"}},
                    "authorships": [{"author": {"display_name": "Ada Lovelace"}}],
                    "abstract_inverted_index": {"Graph": [0], "neural": [1], "networks": [2], "study.": [3]},
                    "referenced_works": [],
                    "cited_by_count": 5,
                }]
            }), encoding="utf-8")
            iter_dir = tmp_p / "iter_out"
            rc1 = cli_main([
                "sota", "--topic", "Graph Neural Networks", "--objective", "Survey GNNs",
                "--iterative", "--fixture", str(fx), "--out-dir", str(iter_dir),
            ])
            self.assertEqual(rc1, 0)
            ev_iter_dir = tmp_p / "ev_iter_out"
            rc1_ev = cli_main(["evidence", "--from-workflow-dir", str(iter_dir), "--out-dir", str(ev_iter_dir)])
            self.assertEqual(rc1_ev, 0)
            q_iter = json.loads((ev_iter_dir / "quality_measurements.json").read_text(encoding="utf-8"))
            self.assertEqual(q_iter["report_qualification"], REPORT_EXPLORATORY)

            # Path B: tako sota-ai --provider fixture -> tako evidence --from-workflow-dir
            ai_dir = tmp_p / "ai_out"
            rc2 = cli_main([
                "sota-ai", "--topic", "Graph Neural Networks", "--objective", "Survey GNNs",
                "--provider", "fixture", "--out-dir", str(ai_dir),
            ])
            self.assertEqual(rc2, 0)
            ev_ai_dir = tmp_p / "ev_ai_out"
            rc2_ev = cli_main(["evidence", "--from-workflow-dir", str(ai_dir), "--out-dir", str(ev_ai_dir)])
            self.assertEqual(rc2_ev, 0)
            q_ai = json.loads((ev_ai_dir / "quality_measurements.json").read_text(encoding="utf-8"))
            self.assertEqual(q_ai["report_qualification"], REPORT_EXPLORATORY)

    def test_sec11_zero_papers_zero_claims_exhausted_remediation_and_repeatability(self):
        # Zero accepted papers & zero claims
        res_empty = run_evidence_and_comparative_pipeline({"topic": "Empty", "papers": []})
        self.assertEqual(res_empty["quality_verdict"], QM_INSUFFICIENT)
        self.assertEqual(res_empty["report_qualification"], REPORT_EXPLORATORY)

        # Exhausted remediation budget (PARTIALLY_SUPPORTED / NEEDS_MANUAL_REVIEW remains unresolved)
        b_exh = {
            "topic": "Exhausted Remediation",
            "papers": [
                _paper(
                    "ex1",
                    full_text="EquiMP achieves 0.010 eV MAE on QM9.",
                    claims=[{
                        "claim_id": "cex1",
                        "claim_text": "EquiMP achieves 0.010 eV MAE on QM9.",
                        "evidence_excerpt": "EquiMP achieves 0.010 eV MAE on QM9.",
                        "evidence_locator": "section:Results",
                        "claim_support_status": CLAIM_NOT_CHECKED,
                    }],
                ),
            ],
        }
        res1 = run_evidence_and_comparative_pipeline(b_exh, max_remediation_attempts=2)
        res2 = run_evidence_and_comparative_pipeline(b_exh, max_remediation_attempts=2)
        self.assertEqual(res1["report_qualification"], REPORT_EXPLORATORY)
        self.assertEqual(res1["quality_measurements"], res2["quality_measurements"])
        self.assertEqual(res1["claims"], res2["claims"])


if __name__ == "__main__":
    unittest.main()
