"""Comprehensive regression tests for Increment A (dev4) scientific integrity (F01-F06, M017-F06)."""

import csv
import json
import pathlib
import tempfile
import unittest

from tools.sota_ai_native.workflow import (
    extract_copilot_jsonl_response,
    normalize_evidence_level,
    write_ai_native_outputs,
)
from tools.sota_integrity import (
    BIB_STATUS_MISMATCH,
    BIB_STATUS_NOT_VERIFIED,
    BIB_STATUS_UNVERIFIABLE,
    BIB_STATUS_VERIFIED,
    CLAIM_NOT_CHECKED,
    EVIDENCE_ABSTRACT_BASED,
    EVIDENCE_METADATA_ONLY,
    EVIDENCE_MODEL_RECALL,
    EVIDENCE_NOT_VERIFIED,
    HUMAN_PENDING,
    aggregate_usage_records,
    classify_evidence_provenance,
    deduplicate_cue_sentences,
    deduplicate_text_items,
    format_bibtex_authors,
    make_usage_record,
    partition_bibliography,
    render_bibtex_entries,
    verify_paper_identity,
)


def _oa_work(
    *,
    oa_id="https://openalex.org/W100",
    title="Echo State Networks for Nonlinear Prediction",
    year=2004,
    doi="https://doi.org/10.1126/science.1091277",
    authors=("Herbert Jaeger", "Harald Haas"),
    landing="https://example.org/paper",
):
    return {
        "id": oa_id,
        "display_name": title,
        "publication_year": year,
        "doi": doi,
        "authorships": [{"author": {"display_name": a}} for a in authors],
        "primary_location": {"landing_page_url": landing},
    }


class BibliographicIdentityTests(unittest.TestCase):
    def test_f01_positive_and_negative_cases(self):
        seen = set()
        # 1. Correct DOI and matching title
        r1 = verify_paper_identity(
            {"Title": "Echo State Networks for Nonlinear Prediction", "Authors": "Herbert Jaeger; Harald Haas", "Year": 2004, "DOI or source URL": "https://doi.org/10.1126/science.1091277"},
            fetch_by_doi_fn=lambda d: _oa_work(),
            search_fn=lambda q: [],
            seen_identities=seen,
        )
        self.assertEqual(r1["identity_verification_outcome"], BIB_STATUS_VERIFIED)
        self.assertEqual(r1["discrepancy_evidence"], [])

        # 2. Wrong DOI resolving to another work
        r2 = verify_paper_identity(
            {"Title": "Quantum Reservoir Computing", "Authors": "Keisuke Fujii", "Year": 2017, "DOI or source URL": "https://doi.org/10.1126/science.1091277"},
            fetch_by_doi_fn=lambda d: _oa_work(),
            search_fn=lambda q: [],
            seen_identities=set(),
        )
        self.assertEqual(r2["identity_verification_outcome"], BIB_STATUS_MISMATCH)
        self.assertIn("title_mismatch", r2["discrepancy_evidence"])

        # 3. Unresolved DOI
        r3 = verify_paper_identity(
            {"Title": "Unresolved Paper", "Authors": "Alice Smith", "Year": 2022, "DOI or source URL": "https://doi.org/10.9999/does-not-exist"},
            fetch_by_doi_fn=lambda d: None,
            search_fn=lambda q: [],
            seen_identities=set(),
        )
        self.assertNotEqual(r3["identity_verification_outcome"], BIB_STATUS_VERIFIED)
        self.assertIn("doi_unresolved", r3["discrepancy_evidence"])

        # 4. Missing DOI with valid alternative identity (arXiv)
        r4 = verify_paper_identity(
            {"Title": "Preprint Without DOI", "Authors": "Jane Doe", "Year": 2023, "DOI or source URL": "https://arxiv.org/abs/2301.00001"},
            fetch_by_doi_fn=lambda d: None,
            search_fn=lambda q: [_oa_work(oa_id="https://openalex.org/W200", title="Preprint Without DOI", year=2023, doi="", authors=("Jane Doe",))],
            seen_identities=set(),
        )
        self.assertEqual(r4["identity_verification_outcome"], BIB_STATUS_VERIFIED)

        # 5. Title variation (subtitle)
        r5 = verify_paper_identity(
            {"Title": "Echo State Networks for Nonlinear Prediction", "Authors": "Herbert Jaeger", "Year": 2004, "DOI or source URL": "https://doi.org/10.1126/science.1091277"},
            fetch_by_doi_fn=lambda d: _oa_work(title="Echo State Networks for Nonlinear Prediction: Extended Analysis"),
            search_fn=lambda q: [],
            seen_identities=set(),
        )
        self.assertEqual(r5["identity_verification_outcome"], BIB_STATUS_VERIFIED)

        # 6. Author mismatch
        r6 = verify_paper_identity(
            {"Title": "Echo State Networks for Nonlinear Prediction", "Authors": "Wrong Author", "Year": 2004, "DOI or source URL": "https://doi.org/10.1126/science.1091277"},
            fetch_by_doi_fn=lambda d: _oa_work(),
            search_fn=lambda q: [],
            seen_identities=set(),
        )
        self.assertEqual(r6["identity_verification_outcome"], BIB_STATUS_MISMATCH)
        self.assertIn("author_mismatch", r6["discrepancy_evidence"])

        # 7. Year discrepancy
        r7 = verify_paper_identity(
            {"Title": "Echo State Networks for Nonlinear Prediction", "Authors": "Herbert Jaeger", "Year": 2010, "DOI or source URL": "https://doi.org/10.1126/science.1091277"},
            fetch_by_doi_fn=lambda d: _oa_work(),
            search_fn=lambda q: [],
            seen_identities=set(),
        )
        self.assertEqual(r7["identity_verification_outcome"], BIB_STATUS_MISMATCH)
        self.assertIn("year_mismatch", r7["discrepancy_evidence"])

        # 8. Insufficient metadata
        r8 = verify_paper_identity(
            {"Title": "UNKNOWN", "Authors": "UNKNOWN", "Year": "UNKNOWN", "DOI or source URL": "UNKNOWN"},
            fetch_by_doi_fn=lambda d: None,
            search_fn=lambda q: [],
            seen_identities=set(),
        )
        self.assertEqual(r8["identity_verification_outcome"], BIB_STATUS_UNVERIFIABLE)
        self.assertIn("insufficient_metadata:missing_title", r8["discrepancy_evidence"])

        # 9. Multiple candidate identities selects matching title
        r9 = verify_paper_identity(
            {"Title": "Target Paper Title", "Authors": "Bob Lee", "Year": 2021, "DOI or source URL": "UNKNOWN"},
            fetch_by_doi_fn=lambda d: None,
            search_fn=lambda q: [
                _oa_work(oa_id="W1", title="Unrelated First Hit", year=2021, doi="", authors=("Other",)),
                _oa_work(oa_id="W2", title="Target Paper Title", year=2021, doi="https://doi.org/10.1000/target", authors=("Bob Lee",)),
            ],
            seen_identities=set(),
        )
        self.assertEqual(r9["identity_verification_outcome"], BIB_STATUS_VERIFIED)

        # 10. Duplicate record
        r10 = verify_paper_identity(
            {"Title": "Echo State Networks for Nonlinear Prediction", "Authors": "Herbert Jaeger", "Year": 2004, "DOI or source URL": "https://doi.org/10.1126/science.1091277"},
            fetch_by_doi_fn=lambda d: _oa_work(),
            search_fn=lambda q: [],
            seen_identities=seen,  # already contains r1
        )
        self.assertIn("duplicate_record", r10["discrepancy_evidence"])
        self.assertNotEqual(r10["identity_verification_outcome"], BIB_STATUS_VERIFIED)


class BibtexSegregationAndAuthorTests(unittest.TestCase):
    def test_m017_f06_segregation_and_no_wrong_doi_in_candidates(self):
        papers = [
            {"Title": "Verified Paper", "Authors": "Alice Smith; Bob Jones", "Year": 2020, "Publication venue": "Nature", "DOI or source URL": "https://doi.org/10.1000/ok"},
            {"Title": "Mismatched Paper", "Authors": "Carol White", "Year": 2021, "Publication venue": "arXiv", "DOI or source URL": "https://doi.org/10.1000/wrong"},
        ]
        checks = [
            {"identity_verification_outcome": BIB_STATUS_VERIFIED, "corrected_identity": {"url": "https://doi.org/10.1000/ok", "doi": "https://doi.org/10.1000/ok"}, "discrepancy_evidence": []},
            {"identity_verification_outcome": BIB_STATUS_MISMATCH, "discrepancy_evidence": ["title_mismatch"]},
        ]
        acc, cand, excl, acct = partition_bibliography(papers, checks)
        self.assertEqual(acct, {"total": 2, "accepted": 1, "candidates": 1, "excluded": 0})
        used = set()
        acc_bib = render_bibtex_entries(acc, is_candidate=False, used_keys=used)
        cand_bib = render_bibtex_entries(cand, is_candidate=True, used_keys=used)
        self.assertIn("doi = {10.1000/ok}", acc_bib)
        self.assertNotIn("wrong", acc_bib)
        self.assertNotIn("doi = {10.1000/wrong}", cand_bib)
        self.assertIn("CANDIDATE (not accepted)", cand_bib)

    def test_f04_bibtex_author_formatting(self):
        self.assertEqual(format_bibtex_authors("Alice Smith; Bob Jones"), "Alice Smith and Bob Jones")
        self.assertEqual(format_bibtex_authors(["Alice Smith", "Bob Jones"]), "Alice Smith and Bob Jones")
        self.assertEqual(format_bibtex_authors("Smith, Alice"), "Smith, Alice")
        self.assertEqual(format_bibtex_authors("José García-López; Müller, Hans"), "José García-López and Müller, Hans")
        self.assertEqual(format_bibtex_authors("Alice Smith and Bob Jones"), "Alice Smith and Bob Jones")
        self.assertEqual(format_bibtex_authors("Herbert Jaeger et al."), "Herbert Jaeger and others")


class DeduplicationAndProvenanceTests(unittest.TestCase):
    def test_f02_dedup_removes_duplicates_and_preserves_distinct_findings(self):
        c, r, l = deduplicate_cue_sentences(
            "Introduces QRC.",
            "Experiments show 95% accuracy under noise. Latency is 12ms.",
            "Experiments show 95% accuracy under noise! Scaling requires 100 qubits.",
        )
        self.assertIn("95% accuracy", r)
        self.assertIn("Latency is 12ms.", r)
        self.assertNotIn("95% accuracy", l)
        self.assertIn("Scaling requires 100 qubits.", l)
        # Negative: distinct metrics/conditions are preserved
        items = deduplicate_text_items([
            "Dataset A achieves 91% accuracy.",
            "Dataset B achieves 84% accuracy under noise.",
            "dataset a achieves 91% accuracy",
        ])
        self.assertEqual(len(items), 2)

    def test_f03_f06_orthogonal_provenance_and_no_promotion(self):
        # Copilot recall labeled full-text must be clamped to MODEL_RECALL
        self.assertTrue(normalize_evidence_level("full-text", "copilot-cli").startswith("MODEL_RECALL"))
        prov1 = classify_evidence_provenance(
            {"Evidence level": "full-text"},
            provider="copilot-cli",
            bib_check={"identity_verification_outcome": BIB_STATUS_VERIFIED},
        )
        self.assertEqual(prov1["BIBLIOGRAPHIC_STATUS"], BIB_STATUS_VERIFIED)
        self.assertEqual(prov1["EVIDENCE_AVAILABILITY"], EVIDENCE_MODEL_RECALL)
        # DOI verification NEVER establishes claim support
        self.assertEqual(prov1["CLAIM_SUPPORT_STATUS"], CLAIM_NOT_CHECKED)
        self.assertEqual(prov1["HUMAN_REVIEW_STATUS"], HUMAN_PENDING)

        # Metadata-only without retrieved abstract stays METADATA_ONLY
        prov2 = classify_evidence_provenance(
            {"Evidence level": "metadata-only"},
            provider="openalex",
            bib_check=None,
            has_retrieved_abstract=False,
        )
        self.assertEqual(prov2["EVIDENCE_AVAILABILITY"], EVIDENCE_METADATA_ONLY)
        self.assertEqual(prov2["BIBLIOGRAPHIC_STATUS"], BIB_STATUS_NOT_VERIFIED)

        # Abstract-based stays ABSTRACT_BASED when abstract retrieved, never FULL_TEXT
        prov3 = classify_evidence_provenance(
            {"Evidence level": "abstract-based"},
            provider="openalex",
            bib_check=None,
            has_retrieved_abstract=True,
            has_full_text=False,
        )
        self.assertEqual(prov3["EVIDENCE_AVAILABILITY"], EVIDENCE_ABSTRACT_BASED)

        # Unbacked abstract claim from non-fixture provider -> NOT_VERIFIED
        self.assertTrue(normalize_evidence_level("abstract-based", "claude-cli").startswith("NOT_VERIFIED"))
        prov4 = classify_evidence_provenance(
            {"Evidence level": "abstract-based"},
            provider="claude-cli",
            bib_check=None,
            has_retrieved_abstract=False,
        )
        self.assertEqual(prov4["EVIDENCE_AVAILABILITY"], EVIDENCE_NOT_VERIFIED)


class TelemetryTests(unittest.TestCase):
    def test_f05_telemetry_separation(self):
        stream = "\n".join([
            json.dumps({"type": "assistant.message", "data": {"phase": "final_answer", "content": '{"ok":true}'}}),
            json.dumps({"type": "result", "usage": {"input_tokens": 100, "output_tokens": 50, "premiumRequests": 2}}),
        ])
        _, u = extract_copilot_jsonl_response(stream)
        self.assertEqual(u["cost_usd"], "UNKNOWN")
        self.assertEqual(u["premium_requests"], 2)
        self.assertEqual(u["total_tokens"], 150)

        u_fix = make_usage_record(cost_usd=0.0, premium_requests=0, input_tokens=0, output_tokens=0, measurement_availability="fixture_zero", provenance="fixture")
        agg = aggregate_usage_records([u, u_fix], wall_time_seconds=1.25)
        self.assertEqual(agg["cost_usd"], "UNKNOWN")
        self.assertEqual(agg["premium_requests"], 2)
        self.assertEqual(agg["request_count"], 2)

    def test_end_to_end_outputs_emit_candidates_and_orthogonal_columns(self):
        with tempfile.TemporaryDirectory() as tmp:
            outs = write_ai_native_outputs(
                out_dir=pathlib.Path(tmp),
                topic="QRC",
                objective="Verify outputs",
                provider="fixture",
                researcher_out={"related_topics": ["QRC", "qrc"], "papers": []},
                analyst_out={"comparative_notes": ["Note A", "note a"], "papers": [
                    {"Title": "Paper One", "Authors": "Alice Smith; Bob Jones", "Year": 2020, "Evidence level": "abstract-based", "Research contribution": "C", "Reported results": "R1.", "Limitations": "R1. L1."}
                ]},
                reviewer_out=None,
                bib_checks=None,
                usage_totals=make_usage_record(cost_usd=0.0, premium_requests=0, input_tokens=0, output_tokens=0),
                elapsed_seconds=0.1,
            )
            names = {p.name for p in outs}
            self.assertIn("references_candidates.bib", names)
            self.assertEqual((pathlib.Path(tmp) / "references.bib").read_text(encoding="utf-8"), "")
            self.assertIn("Alice Smith and Bob Jones", (pathlib.Path(tmp) / "references_candidates.bib").read_text(encoding="utf-8"))
            with (pathlib.Path(tmp) / "literature_matrix.csv").open(encoding="utf-8") as f:
                rows = list(csv.DictReader(f))
            self.assertEqual(rows[0]["Claim support status"], CLAIM_NOT_CHECKED)
            self.assertEqual(rows[0]["Bibliographic status"], BIB_STATUS_NOT_VERIFIED)


if __name__ == "__main__":
    unittest.main()
