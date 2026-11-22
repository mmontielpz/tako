"""Evidence acquisition, claim grounding, comparability, gap assessment, and Tier 1 quality remediation (Increment C)."""

from __future__ import annotations

import argparse
import csv
import difflib
import hashlib
import json
import pathlib
import re
import sys
from typing import Any

from tools.sota_integrity import (
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
    EVIDENCE_UNKNOWN,
    HUMAN_PENDING,
    partition_bibliography,
    render_bibtex_entries,
)

# Textual correspondence outcomes (ResearchOS-adapted Level 2)
CORR_EXACT = "EXACT_MATCH"
CORR_FUZZY = "FUZZY_MATCH"
CORR_NEEDS_REVIEW = "NEEDS_MANUAL_REVIEW"
CORR_UNMATCHED = "UNMATCHED"
CORR_INVALID_LOCATOR = "INVALID_LOCATOR"
CORR_SOURCE_MISMATCH = "SOURCE_MISMATCH"
CORR_NOT_CHECKED = "NOT_CHECKED"

# Comparability classes (M017A 09_PAPER_COMPARISON_SCHEMA)
COMP_DIRECT = "DIRECTLY_COMPARABLE"
COMP_CONDITIONAL = "CONDITIONALLY_COMPARABLE"
COMP_NOT = "NOT_COMPARABLE"

# Gap types
GAP_AUTHOR_LIM = "AUTHOR_REPORTED_LIMITATION"
GAP_CORPUS_LIM = "CORPUS_OBSERVED_LIMITATION"
GAP_MISSING_COMP = "MISSING_COMPARATIVE_EVIDENCE"
GAP_OPPORTUNITY = "POTENTIAL_RESEARCH_OPPORTUNITY"
GAP_HYPOTHESIS = "UNVERIFIED_HYPOTHESIS"
GAP_INSUFFICIENT = "INSUFFICIENT_EVIDENCE"

# Quality measurement states
QM_PASS = "PASS"
QM_FAIL = "FAIL"
QM_NOT_MEASURED = "NOT_MEASURED"
QM_NOT_APPLICABLE = "NOT_APPLICABLE"
QM_INSUFFICIENT = "INSUFFICIENT_EVIDENCE"

# Report qualification
REPORT_EXPLORATORY = "EXPLORATORY_LITERATURE_MAP"
REPORT_GROUNDED = "EVIDENCE_GROUNDED_COMPARATIVE_SOTA"


def _sha256_text(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def _norm_ground_text(text: str) -> str:
    cleaned = re.sub(r"[^\w\s.%+-]", " ", text.lower())
    return re.sub(r"\s+", " ", cleaned).strip()


def acquire_evidence(paper: dict[str, Any]) -> dict[str, Any]:
    """Acquire and normalize evidence provenance without conflating URLs or recall with full text."""
    paper_id = str(paper.get("paper_id") or paper.get("canonical_id") or paper.get("Title") or "unknown").strip()
    local_path = paper.get("local_text_path")
    full_text = str(paper.get("full_text") or "").strip()
    abstract = str(paper.get("abstract") or "").strip()
    model_desc = str(paper.get("model_recall_summary") or "").strip()
    pdf_url = str(paper.get("pdf_url") or "").strip()
    source_url = str(paper.get("DOI or source URL") or paper.get("source_url") or "UNKNOWN").strip()

    limitations: list[str] = []
    if local_path:
        p = pathlib.Path(str(local_path))
        if p.is_file() and p.suffix.lower() in (".txt", ".md"):
            full_text = p.read_text(encoding="utf-8").strip()
            retrieval_method = "local_plain_text_file"
        else:
            retrieval_method = "local_file_unsupported_or_missing"
            limitations.append("PDF parsing not supported without external parser or file unavailable.")
    elif full_text:
        retrieval_method = str(paper.get("retrieval_method") or "provided_plain_text")
    elif abstract:
        retrieval_method = str(paper.get("retrieval_method") or "metadata_abstract")
    elif model_desc:
        retrieval_method = "model_recall_only"
    else:
        retrieval_method = "metadata_only"

    if pdf_url and not full_text:
        limitations.append("pdf_url present without parsed full-text; PDF parsing not supported.")

    if full_text:
        avail = EVIDENCE_FULL_TEXT
        content = full_text
        locator = str(paper.get("evidence_locator") or "full_text")
        ext_method = "plain_text_ingestion"
    elif abstract:
        avail = EVIDENCE_ABSTRACT_BASED
        content = abstract
        locator = str(paper.get("evidence_locator") or "abstract")
        ext_method = "abstract_text"
    elif model_desc:
        avail = EVIDENCE_MODEL_RECALL
        content = ""
        locator = "NONE"
        ext_method = "model_recall"
        limitations.append("Model-generated summary is not retrieved scientific evidence.")
    elif paper.get("Title"):
        avail = EVIDENCE_METADATA_ONLY
        content = ""
        locator = "metadata"
        ext_method = "metadata_only"
    else:
        avail = EVIDENCE_UNAVAILABLE
        content = ""
        locator = "NONE"
        ext_method = "none"

    return {
        "paper_id": paper_id,
        "evidence_source": str(paper.get("evidence_source") or source_url),
        "retrieval_method": retrieval_method,
        "availability_status": avail,
        "source_identifier": source_url,
        "content_hash": _sha256_text(content) if content else None,
        "evidence_locator": locator,
        "extraction_method": ext_method,
        "extraction_limitations": limitations,
        "_text": content,
    }


def normalize_extraction(paper: dict[str, Any], ev_rec: dict[str, Any]) -> dict[str, Any]:
    """Build structured scientific extraction distinguishing author-reported vs missing information."""
    ext = dict(paper.get("extraction") or {})
    avail = ev_rec["availability_status"]
    missing: list[str] = []
    fields = [
        "research_problem", "proposed_contributions", "methodology",
        "datasets_or_materials", "evaluation_protocol", "baselines",
        "experimental_conditions", "environment", "results", "author_limitations",
    ]
    out: dict[str, Any] = {
        "paper_id": ev_rec["paper_id"],
        "provenance": "author_reported" if avail in (EVIDENCE_FULL_TEXT, EVIDENCE_ABSTRACT_BASED) else "unverified_or_missing",
    }
    for f in fields:
        val = ext.get(f) or paper.get(f)
        if val in (None, "", []):
            out[f] = "NOT_VERIFIED"
            missing.append(f)
        else:
            out[f] = val
    metrics = ext.get("reported_metrics") or paper.get("reported_metrics") or []
    out["reported_metrics"] = list(metrics) if isinstance(metrics, list) else []
    if not out["reported_metrics"]:
        missing.append("reported_metrics")
    out["missing_information"] = missing
    return out


def verify_textual_correspondence(
    excerpt: str,
    locator: str,
    ev_rec: dict[str, Any],
    *,
    fuzzy_threshold: float = 0.85,
    review_threshold: float = 0.50,
    claim_paper_id: str | None = None,
) -> tuple[str, float]:
    """Deterministic ResearchOS-adapted Level 2 textual correspondence check."""
    if claim_paper_id and claim_paper_id != ev_rec["paper_id"]:
        return CORR_SOURCE_MISMATCH, 0.0
    if not ev_rec.get("_text"):
        return CORR_NOT_CHECKED, 0.0
    loc = str(locator or "").strip()
    if not loc or loc.upper() in ("NONE", "INVALID", "MISSING"):
        return CORR_INVALID_LOCATOR, 0.0
    # Validate section/page/abstract locator format if structured
    if loc.startswith("page:"):
        try:
            pnum = int(loc.split(":", 1)[1])
            if pnum < 1:
                return CORR_INVALID_LOCATOR, 0.0
        except ValueError:
            return CORR_INVALID_LOCATOR, 0.0

    q_norm = _norm_ground_text(excerpt)
    s_norm = _norm_ground_text(ev_rec["_text"])
    if not q_norm:
        return CORR_UNMATCHED, 0.0
    if q_norm in s_norm:
        return CORR_EXACT, 1.0

    q_words = q_norm.split()
    s_words = s_norm.split()
    if not s_words:
        return CORR_UNMATCHED, 0.0
    win = max(1, min(len(q_words), len(s_words)))
    best = 0.0
    step = max(1, win // 3)
    for i in range(0, max(1, len(s_words) - win + 1), step):
        cand = " ".join(s_words[i:i + win])
        ratio = difflib.SequenceMatcher(None, q_norm, cand).ratio()
        if ratio > best:
            best = ratio
    if best >= fuzzy_threshold:
        return CORR_FUZZY, round(best, 4)
    if best >= review_threshold:
        return CORR_NEEDS_REVIEW, round(best, 4)
    return CORR_UNMATCHED, round(best, 4)


def register_claims(
    papers: list[dict[str, Any]],
    ev_by_id: dict[str, dict[str, Any]],
) -> list[dict[str, Any]]:
    """Register traceable claim records without equating textual match with scientific support."""
    claims: list[dict[str, Any]] = []
    for p in papers:
        pid = str(p.get("paper_id") or p.get("canonical_id") or p.get("Title") or "unknown").strip()
        ev = ev_by_id.get(pid) or {"paper_id": pid, "availability_status": EVIDENCE_UNKNOWN, "content_hash": None, "_text": ""}
        for idx, raw_c in enumerate(p.get("claims") or [], start=1):
            cid = str(raw_c.get("claim_id") or f"{pid}:C{idx}")
            c_paper_id = str(raw_c.get("paper_id") or pid)
            excerpt = str(raw_c.get("evidence_excerpt") or "").strip()
            locator = str(raw_c.get("evidence_locator") or ev.get("evidence_locator") or "NONE").strip()
            target_ev = ev_by_id.get(c_paper_id, ev)
            corr, score = verify_textual_correspondence(
                excerpt,
                locator,
                target_ev,
                claim_paper_id=pid if c_paper_id != pid else None,
            )

            requested_support = str(raw_c.get("claim_support_status") or CLAIM_NOT_CHECKED)
            # Enforce orthogonal separation: textual match alone never establishes SUPPORTED;
            # unsupported/unmatched/invalid locators cannot be SUPPORTED.
            support = requested_support
            if ev["availability_status"] in (EVIDENCE_MODEL_RECALL, EVIDENCE_METADATA_ONLY, EVIDENCE_UNAVAILABLE, EVIDENCE_UNKNOWN):
                if support == CLAIM_SUPPORTED:
                    support = CLAIM_NOT_VERIFIED
            if corr in (CORR_UNMATCHED, CORR_INVALID_LOCATOR, CORR_SOURCE_MISMATCH):
                if support == CLAIM_SUPPORTED:
                    support = CLAIM_UNSUPPORTED

            claims.append({
                "claim_id": cid,
                "paper_id": pid,
                "claim_type": str(raw_c.get("claim_type") or "contribution"),
                "claim_text": str(raw_c.get("claim_text") or "").strip(),
                "evidence_availability": ev["availability_status"],
                "evidence_locator": locator,
                "evidence_excerpt": excerpt,
                "source_content_hash": target_ev.get("content_hash"),
                "extraction_provenance": str(raw_c.get("extraction_provenance") or ev.get("extraction_method") or "UNKNOWN"),
                "textual_correspondence_status": corr,
                "textual_correspondence_score": score,
                "claim_support_status": support,
                "human_review_status": str(raw_c.get("human_review_status") or HUMAN_PENDING),
                "limitations": list(raw_c.get("limitations") or []),
                "requested_claim_support_status": requested_support,
            })
    return claims


def _normalize_metric_unit(val: Any, unit: str) -> tuple[float | None, str]:
    if not isinstance(val, (int, float)) or isinstance(val, bool):
        return None, unit.lower().strip()
    u = unit.lower().strip()
    if u == "ms":
        return float(val) / 1000.0, "s"
    if u in ("s", "sec", "seconds"):
        return float(val), "s"
    if u in ("ratio", "fraction") and 0.0 <= float(val) <= 1.0:
        return float(val) * 100.0, "%"
    if u in ("%", "percent", "percentage"):
        return float(val), "%"
    return float(val), u


def compare_papers(
    papers: list[dict[str, Any]],
    extractions: dict[str, dict[str, Any]],
    ev_by_id: dict[str, dict[str, Any]],
) -> list[dict[str, Any]]:
    """Pairwise comparative validity assessment across scientific dimensions."""
    comparisons: list[dict[str, Any]] = []
    for i in range(len(papers)):
        for j in range(i + 1, len(papers)):
            p1, p2 = papers[i], papers[j]
            id1 = str(p1.get("paper_id") or p1.get("canonical_id") or p1.get("Title"))
            id2 = str(p2.get("paper_id") or p2.get("canonical_id") or p2.get("Title"))
            e1, e2 = extractions[id1], extractions[id2]
            ev1, ev2 = ev_by_id[id1], ev_by_id[id2]

            reasons: list[str] = []
            # Missing conditions are never treated as matching
            if e1["datasets_or_materials"] == "NOT_VERIFIED" or e2["datasets_or_materials"] == "NOT_VERIFIED":
                reasons.append("missing_dataset_or_material")
            elif e1["datasets_or_materials"] != e2["datasets_or_materials"]:
                reasons.append(f"different_dataset:{e1['datasets_or_materials']}!= {e2['datasets_or_materials']}")

            if e1["evaluation_protocol"] == "NOT_VERIFIED" or e2["evaluation_protocol"] == "NOT_VERIFIED":
                reasons.append("missing_evaluation_protocol")
            elif e1["evaluation_protocol"] != e2["evaluation_protocol"]:
                reasons.append("different_evaluation_protocol")

            if e1["experimental_conditions"] == "NOT_VERIFIED" or e2["experimental_conditions"] == "NOT_VERIFIED":
                reasons.append("missing_experimental_conditions")
            elif e1["experimental_conditions"] != e2["experimental_conditions"]:
                reasons.append("different_experimental_conditions")

            m1_list = e1.get("reported_metrics") or []
            m2_list = e2.get("reported_metrics") or []
            metric_pairs: list[dict[str, Any]] = []
            metric_incompatible = False
            for m1 in m1_list:
                for m2 in m2_list:
                    n1 = str(m1.get("name") or "").strip().lower()
                    n2 = str(m2.get("name") or "").strip().lower()
                    if n1 == n2 and n1:
                        v1, u1 = _normalize_metric_unit(m1.get("value"), str(m1.get("unit") or ""))
                        v2, u2 = _normalize_metric_unit(m2.get("value"), str(m2.get("unit") or ""))
                        if u1 == u2:
                            metric_pairs.append({"metric": n1, "unit": u1, "v1": v1, "v2": v2})
                        else:
                            metric_incompatible = True
                            reasons.append(f"incompatible_metric_units:{n1}({u1} vs {u2})")
                    elif n1 and n2 and (n1 in n2 or n2 in n1):
                        metric_incompatible = True
                        reasons.append(f"incompatible_metric_definitions:{n1} vs {n2}")

            if ev1["availability_status"] in (EVIDENCE_METADATA_ONLY, EVIDENCE_MODEL_RECALL) or ev2["availability_status"] in (EVIDENCE_METADATA_ONLY, EVIDENCE_MODEL_RECALL):
                reasons.append("insufficient_evidence_availability")

            if not reasons and metric_pairs:
                comp_class = COMP_DIRECT
            elif any(r.startswith(("different_dataset", "different_evaluation_protocol", "incompatible_metric")) for r in reasons) or metric_incompatible:
                comp_class = COMP_NOT
            elif reasons and any(r.startswith("missing_") for r in reasons):
                comp_class = COMP_NOT if not metric_pairs else COMP_CONDITIONAL
            else:
                comp_class = COMP_CONDITIONAL

            comparisons.append({
                "paper_1": id1,
                "paper_2": id2,
                "comparability_class": comp_class,
                "reasons": reasons or ["matching_dataset_protocol_conditions_and_metrics"],
                "normalized_metric_pairs": metric_pairs,
                "global_ranking_permitted": False,  # Never declare global winner from heterogeneous metrics
            })
    return comparisons


def assess_research_gaps(
    accepted_papers: list[dict[str, Any]],
    extractions: dict[str, dict[str, Any]],
    claims: list[dict[str, Any]],
    comparisons: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    """Generate conservative research-gap records anchored to evidence/claims."""
    if not accepted_papers:
        return [{
            "gap_id": "GAP-ABSTAIN-1",
            "gap_type": GAP_INSUFFICIENT,
            "description": "Insufficient accepted papers to establish corpus-grounded research gaps.",
            "supporting_refs": [],
            "novelty_claimed": False,
        }]

    gaps: list[dict[str, Any]] = []
    # 1. Author-reported limitations
    for p in accepted_papers:
        pid = str(p.get("paper_id") or p.get("canonical_id") or p.get("Title"))
        lim = extractions[pid]["author_limitations"]
        if lim != "NOT_VERIFIED":
            gaps.append({
                "gap_id": f"GAP-AUTH-{pid}",
                "gap_type": GAP_AUTHOR_LIM,
                "description": str(lim),
                "supporting_refs": [pid],
                "novelty_claimed": False,
            })

    # 2. Missing comparative evidence across corpus
    not_comp = [c for c in comparisons if c["comparability_class"] == COMP_NOT]
    if not_comp:
        gaps.append({
            "gap_id": "GAP-COMP-1",
            "gap_type": GAP_MISSING_COMP,
            "description": f"{len(not_comp)} paper pair(s) are NOT_COMPARABLE due to heterogeneous protocols/metrics/conditions.",
            "supporting_refs": [f"{c['paper_1']}<->{c['paper_2']}" for c in not_comp],
            "novelty_claimed": False,
        })

    if not gaps:
        gaps.append({
            "gap_id": "GAP-INSUFF-1",
            "gap_type": GAP_INSUFFICIENT,
            "description": "Available corpus claims do not establish a cross-corpus gap; absence in corpus does not imply absence in literature.",
            "supporting_refs": [c["claim_id"] for c in claims[:5]],
            "novelty_claimed": False,
        })
    return gaps


def evaluate_tier1_quality(
    papers: list[dict[str, Any]],
    accepted_papers: list[dict[str, Any]],
    claims: list[dict[str, Any]],
    comparisons: list[dict[str, Any]],
    bib_checks: list[dict[str, Any]] | None,
    *,
    numerical_checks_executed: bool = True,
) -> tuple[list[dict[str, Any]], str]:
    """Evaluate all 7 canonical M017A Tier 1 mandatory integrity gates."""
    metrics: list[dict[str, Any]] = []
    n_acc = len(accepted_papers)

    # QM-B02: DOI Identity Agreement
    if n_acc == 0:
        metrics.append({"metric_id": "QM-B02", "name": "DOI Identity Agreement", "numerator": 0, "denominator": 0, "observed_value": None, "threshold": "== 1.0", "measurement_status": QM_INSUFFICIENT, "gate_outcome": QM_FAIL, "evidence_ref": "references.bib (0 accepted)"})
    else:
        verified_cnt = sum(1 for p in accepted_papers if p.get("Bibliographic status") == BIB_STATUS_VERIFIED)
        val = verified_cnt / n_acc
        metrics.append({"metric_id": "QM-B02", "name": "DOI Identity Agreement", "numerator": verified_cnt, "denominator": n_acc, "observed_value": round(val, 4), "threshold": "== 1.0", "measurement_status": QM_PASS if val == 1.0 else QM_FAIL, "gate_outcome": QM_PASS if val == 1.0 else QM_FAIL, "evidence_ref": "references.bib"})

    # QM-B03: Invalid Reference Rate (in accepted bibliography)
    if n_acc == 0:
        metrics.append({"metric_id": "QM-B03", "name": "Invalid Reference Rate", "numerator": 0, "denominator": 0, "observed_value": None, "threshold": "== 0.0", "measurement_status": QM_INSUFFICIENT, "gate_outcome": QM_FAIL, "evidence_ref": "references.bib (0 accepted)"})
    else:
        inv_cnt = sum(1 for p in accepted_papers if p.get("Bibliographic status") != BIB_STATUS_VERIFIED)
        val = inv_cnt / n_acc
        metrics.append({"metric_id": "QM-B03", "name": "Invalid Reference Rate", "numerator": inv_cnt, "denominator": n_acc, "observed_value": round(val, 4), "threshold": "== 0.0", "measurement_status": QM_PASS if val == 0.0 else QM_FAIL, "gate_outcome": QM_PASS if val == 0.0 else QM_FAIL, "evidence_ref": "references.bib"})

    # QM-B04: Duplicate Rate
    if n_acc == 0:
        metrics.append({"metric_id": "QM-B04", "name": "Duplicate Rate", "numerator": 0, "denominator": 0, "observed_value": None, "threshold": "== 0.0", "measurement_status": QM_INSUFFICIENT, "gate_outcome": QM_FAIL, "evidence_ref": "accepted_papers"})
    else:
        ids = [str(p.get("paper_id") or p.get("canonical_id") or p.get("Title")) for p in accepted_papers]
        dups = len(ids) - len(set(ids))
        val = dups / n_acc
        metrics.append({"metric_id": "QM-B04", "name": "Duplicate Rate", "numerator": dups, "denominator": n_acc, "observed_value": round(val, 4), "threshold": "== 0.0", "measurement_status": QM_PASS if val == 0.0 else QM_FAIL, "gate_outcome": QM_PASS if val == 0.0 else QM_FAIL, "evidence_ref": "accepted_papers"})

    # QM-E03: Numerical Accuracy
    if not numerical_checks_executed:
        metrics.append({"metric_id": "QM-E03", "name": "Numerical Accuracy", "numerator": None, "denominator": None, "observed_value": None, "threshold": "== 1.0", "measurement_status": QM_NOT_MEASURED, "gate_outcome": QM_FAIL, "evidence_ref": "numerical_checks_not_executed"})
    elif not claims:
        metrics.append({"metric_id": "QM-E03", "name": "Numerical Accuracy", "numerator": 0, "denominator": 0, "observed_value": None, "threshold": "== 1.0", "measurement_status": QM_INSUFFICIENT, "gate_outcome": QM_FAIL, "evidence_ref": "claims (0 claims)"})
    else:
        num_claims = [c for c in claims if c["claim_type"] == "numerical"]
        if not num_claims:
            metrics.append({"metric_id": "QM-E03", "name": "Numerical Accuracy", "numerator": 0, "denominator": 0, "observed_value": None, "threshold": "== 1.0", "measurement_status": QM_NOT_APPLICABLE, "gate_outcome": QM_PASS, "evidence_ref": "no numerical claims"})
        else:
            ok_num = sum(1 for c in num_claims if c["textual_correspondence_status"] in (CORR_EXACT, CORR_FUZZY))
            val = ok_num / len(num_claims)
            metrics.append({"metric_id": "QM-E03", "name": "Numerical Accuracy", "numerator": ok_num, "denominator": len(num_claims), "observed_value": round(val, 4), "threshold": "== 1.0", "measurement_status": QM_PASS if val == 1.0 else QM_FAIL, "gate_outcome": QM_PASS if val == 1.0 else QM_FAIL, "evidence_ref": "claims.json"})

    # QM-G04: Unsupported Claim Rate (claims asserting SUPPORTED without valid grounding/correspondence)
    if not claims:
        metrics.append({"metric_id": "QM-G04", "name": "Unsupported Claim Rate", "numerator": 0, "denominator": 0, "observed_value": None, "threshold": "== 0.0", "measurement_status": QM_INSUFFICIENT, "gate_outcome": QM_FAIL, "evidence_ref": "claims (0 claims)"})
    else:
        bad_claims = sum(
            1 for c in claims
            if c["claim_support_status"] == CLAIM_SUPPORTED
            and (
                c["textual_correspondence_status"] in (CORR_UNMATCHED, CORR_INVALID_LOCATOR, CORR_SOURCE_MISMATCH, CORR_NOT_CHECKED)
                or c["evidence_availability"] in (EVIDENCE_MODEL_RECALL, EVIDENCE_METADATA_ONLY, EVIDENCE_UNAVAILABLE, EVIDENCE_UNKNOWN)
            )
        )
        val = bad_claims / len(claims)
        metrics.append({"metric_id": "QM-G04", "name": "Unsupported Claim Rate", "numerator": bad_claims, "denominator": len(claims), "observed_value": round(val, 4), "threshold": "== 0.0", "measurement_status": QM_PASS if val == 0.0 else QM_FAIL, "gate_outcome": QM_PASS if val == 0.0 else QM_FAIL, "evidence_ref": "claims.json"})

    # QM-G05: Critical Hallucination Count (fabricated quotes or mismatched/unverified accepted claims)
    fab_count = sum(1 for c in claims if c["textual_correspondence_status"] in (CORR_UNMATCHED, CORR_SOURCE_MISMATCH))
    if not claims and n_acc == 0:
        metrics.append({"metric_id": "QM-G05", "name": "Critical Hallucination Count", "numerator": 0, "denominator": 0, "observed_value": None, "threshold": "== 0", "measurement_status": QM_INSUFFICIENT, "gate_outcome": QM_FAIL, "evidence_ref": "empty_corpus"})
    else:
        metrics.append({"metric_id": "QM-G05", "name": "Critical Hallucination Count", "numerator": fab_count, "denominator": max(1, len(claims)), "observed_value": fab_count, "threshold": "== 0", "measurement_status": QM_PASS if fab_count == 0 else QM_FAIL, "gate_outcome": QM_PASS if fab_count == 0 else QM_FAIL, "evidence_ref": "claims.json"})

    # QM-C03: Incompatible Comparison Rate
    if not comparisons:
        metrics.append({"metric_id": "QM-C03", "name": "Incompatible Comparison Rate", "numerator": 0, "denominator": 0, "observed_value": None, "threshold": "== 0.0", "measurement_status": QM_NOT_APPLICABLE if n_acc < 2 else QM_INSUFFICIENT, "gate_outcome": QM_PASS if n_acc < 2 else QM_FAIL, "evidence_ref": "comparability_assessments.json"})
    else:
        bad_comp = sum(1 for c in comparisons if c["comparability_class"] == COMP_DIRECT and any(r != "matching_dataset_protocol_conditions_and_metrics" for r in c["reasons"]))
        val = bad_comp / len(comparisons)
        metrics.append({"metric_id": "QM-C03", "name": "Incompatible Comparison Rate", "numerator": bad_comp, "denominator": len(comparisons), "observed_value": round(val, 4), "threshold": "== 0.0", "measurement_status": QM_PASS if val == 0.0 else QM_FAIL, "gate_outcome": QM_PASS if val == 0.0 else QM_FAIL, "evidence_ref": "comparability_assessments.json"})

    if n_acc == 0 or not claims:
        overall = QM_FAIL
    elif all(m["gate_outcome"] == QM_PASS for m in metrics):
        overall = QM_PASS
    else:
        overall = "PARTIAL"
    return metrics, overall


def run_evidence_and_comparative_pipeline(
    bundle: dict[str, Any],
    *,
    max_remediation_attempts: int = 2,
    numerical_checks_executed: bool = True,
) -> dict[str, Any]:
    """Execute Increment C evidence grounding, comparison, gaps, Tier 1 gates, and bounded remediation."""
    topic = str(bundle.get("topic") or "Research Topic")
    objective = str(bundle.get("objective") or "Evidence-grounded SOTA analysis")
    raw_papers = list(bundle.get("papers") or [])
    bib_checks = bundle.get("bib_checks")

    # 1. Acquire evidence & normalize extractions
    ev_manifest: list[dict[str, Any]] = []
    ev_by_id: dict[str, dict[str, Any]] = {}
    extractions: dict[str, dict[str, Any]] = {}
    for idx, p in enumerate(raw_papers):
        ev = acquire_evidence(p)
        ev_manifest.append({k: v for k, v in ev.items() if k != "_text"})
        ev_by_id[ev["paper_id"]] = ev
        extractions[ev["paper_id"]] = normalize_extraction(p, ev)
        if bib_checks and idx < len(bib_checks):
            p["Bibliographic status"] = bib_checks[idx].get("identity_verification_outcome") or bib_checks[idx].get("openalex_match") or BIB_STATUS_VERIFIED
        elif "Bibliographic status" not in p:
            p["Bibliographic status"] = BIB_STATUS_VERIFIED if (p.get("DOI or source URL") and p.get("Year")) else "UNVERIFIABLE"

    accepted_papers = [p for p in raw_papers if p.get("Bibliographic status") == BIB_STATUS_VERIFIED]
    claims = register_claims(accepted_papers, ev_by_id)
    comparisons = compare_papers(accepted_papers, extractions, ev_by_id)
    gaps = assess_research_gaps(accepted_papers, extractions, claims, comparisons)
    metrics, overall = evaluate_tier1_quality(
        raw_papers, accepted_papers, claims, comparisons, bib_checks,
        numerical_checks_executed=numerical_checks_executed,
    )

    remediation_trace: list[dict[str, Any]] = []
    attempt = 0
    while overall != QM_PASS and attempt < max_remediation_attempts:
        attempt += 1
        actions: list[str] = []
        # Bounded remediation: remove or downgrade ungrounded/fabricated claims so downstream artifacts stay clean
        new_claims: list[dict[str, Any]] = []
        for c in claims:
            if c["textual_correspondence_status"] in (CORR_UNMATCHED, CORR_SOURCE_MISMATCH, CORR_INVALID_LOCATOR):
                c = dict(c)
                c["claim_support_status"] = CLAIM_UNSUPPORTED
                c["remediated"] = True
                actions.append(f"downgraded_claim:{c['claim_id']}->{CLAIM_UNSUPPORTED}")
            new_claims.append(c)
        claims = new_claims

        # Re-run comparisons & gaps & quality after remediation
        comparisons = compare_papers(accepted_papers, extractions, ev_by_id)
        gaps = assess_research_gaps(accepted_papers, extractions, claims, comparisons)
        metrics, new_overall = evaluate_tier1_quality(
            raw_papers, accepted_papers, claims, comparisons, bib_checks,
            numerical_checks_executed=numerical_checks_executed,
        )
        remediation_trace.append({
            "attempt": attempt,
            "actions": actions,
            "verdict_after": new_overall,
        })
        overall = new_overall
        if not actions:
            break

    has_full_text = any(ev["availability_status"] == EVIDENCE_FULL_TEXT for ev in ev_manifest)
    report_qualification = (
        REPORT_GROUNDED if (has_full_text and overall == QM_PASS) else REPORT_EXPLORATORY
    )

    return {
        "topic": topic,
        "objective": objective,
        "papers": raw_papers,
        "accepted_papers": accepted_papers,
        "bib_checks": bib_checks,
        "evidence_manifest": ev_manifest,
        "extractions": extractions,
        "claims": claims,
        "comparisons": comparisons,
        "research_gaps": gaps,
        "quality_measurements": metrics,
        "quality_verdict": overall,
        "remediation_trace": remediation_trace,
        "report_qualification": report_qualification,
    }


def write_evidence_outputs(out_dir: pathlib.Path, res: dict[str, Any]) -> list[pathlib.Path]:
    """Write all Increment C machine-readable artifacts and M017A-reconciled SOTA report."""
    out_dir.mkdir(parents=True, exist_ok=True)
    paths: list[pathlib.Path] = []

    def _dump(name: str, obj: Any) -> pathlib.Path:
        p = out_dir / name
        p.write_text(json.dumps(obj, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        paths.append(p)
        return p

    _dump("evidence_manifest.json", res["evidence_manifest"])
    _dump("claims.json", res["claims"])
    _dump("comparability_assessments.json", res["comparisons"])
    _dump("research_gaps.json", res["research_gaps"])
    _dump("quality_measurements.json", {"verdict": res["quality_verdict"], "metrics": res["quality_measurements"]})
    _dump("remediation_trace.json", res["remediation_trace"])

    # Structured literature_matrix.csv
    matrix_p = out_dir / "literature_matrix.csv"
    fields = ["paper_id", "Title", "Year", "Bibliographic status", "Evidence availability", "Claim support status", "Human review status", "Methodology", "Datasets"]
    ev_map = {e["paper_id"]: e for e in res["evidence_manifest"]}
    with matrix_p.open("w", newline="", encoding="utf-8") as f:
        w = csv.DictWriter(f, fieldnames=fields)
        w.writeheader()
        for p in res["papers"]:
            pid = str(p.get("paper_id") or p.get("canonical_id") or p.get("Title") or "unknown").strip()
            ext = res["extractions"].get(pid, {})
            ev = ev_map.get(pid, {})
            w.writerow({
                "paper_id": pid,
                "Title": p.get("Title", "UNKNOWN"),
                "Year": p.get("Year", "UNKNOWN"),
                "Bibliographic status": p.get("Bibliographic status", "NOT_CHECKED"),
                "Evidence availability": ev.get("availability_status", EVIDENCE_UNKNOWN),
                "Claim support status": CLAIM_NOT_CHECKED,
                "Human review status": HUMAN_PENDING,
                "Methodology": ext.get("methodology", "NOT_VERIFIED"),
                "Datasets": ext.get("datasets_or_materials", "NOT_VERIFIED"),
            })
    paths.append(matrix_p)

    # Segregated accepted/candidate bibliography
    used_keys: set[str] = set()
    accepted_items = [{"paper": p, "bib_check": None, "outcome": BIB_STATUS_VERIFIED} for p in res["accepted_papers"]]
    candidate_items = [{"paper": p, "bib_check": None, "outcome": p.get("Bibliographic status", "UNVERIFIABLE")} for p in res["papers"] if p.get("Bibliographic status") != BIB_STATUS_VERIFIED]
    bib_p = out_dir / "references.bib"
    cand_p = out_dir / "references_candidates.bib"
    bib_p.write_text(render_bibtex_entries(accepted_items, is_candidate=False, used_keys=used_keys), encoding="utf-8")
    cand_p.write_text(render_bibtex_entries(candidate_items, is_candidate=True, used_keys=used_keys), encoding="utf-8")
    paths.extend([bib_p, cand_p])

    # M017A Minimum Viable SOTA Report
    rep_p = out_dir / "sota_report.md"
    lines = [
        f"# State-of-the-Art Report: {res['topic']}",
        "",
        f"- **Report Qualification:** `{res['report_qualification']}`",
        f"- **Quality Gate Verdict:** `{res['quality_verdict']}`",
        f"- **Objective:** {res['objective']}",
        f"- **Accepted Papers:** {len(res['accepted_papers'])} / {len(res['papers'])}",
        "",
        "## 1. Research Objective & Scope",
        res["objective"],
        "",
        "## 2. Verified & Unresolved Bibliographic Identities",
        f"- Accepted (`references.bib`): {len(res['accepted_papers'])}",
        f"- Candidates (`references_candidates.bib`): {len(candidate_items)}",
        "",
        "## 3. Contributions, Methodology, Results & Limitations",
    ]
    for p in res["accepted_papers"]:
        pid = str(p.get("paper_id") or p.get("canonical_id") or p.get("Title") or "unknown").strip()
        ext = res["extractions"].get(pid, {})
        lines.extend([
            f"### {p.get('Title', 'UNKNOWN')} ({p.get('Year', 'UNKNOWN')})",
            f"- **Methodology:** {ext.get('methodology', 'NOT_VERIFIED')}",
            f"- **Results:** {ext.get('results', 'NOT_VERIFIED')}",
            f"- **Limitations:** {ext.get('author_limitations', 'NOT_VERIFIED')}",
            "",
        ])
    lines.extend(["## 4. Qualified Comparative Analysis"])
    for c in res["comparisons"]:
        lines.append(f"- `{c['paper_1']}` vs `{c['paper_2']}`: `{c['comparability_class']}` ({', '.join(c['reasons'])})")
    if not res["comparisons"]:
        lines.append("- No pairwise comparisons available.")
    lines.extend(["", "## 5. Research Gaps"])
    for g in res["research_gaps"]:
        lines.append(f"- [{g['gap_type']}] {g['description']} (refs: {g['supporting_refs']})")
    lines.extend(["", "## 6. Evidence & Quality Gate Summary"])
    for m in res["quality_measurements"]:
        lines.append(f"- `{m['metric_id']}` ({m['name']}): `{m['gate_outcome']}` (`{m['measurement_status']}`)")
    rep_p.write_text("\n".join(lines) + "\n", encoding="utf-8")
    paths.append(rep_p)
    return paths


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="TAKO Evidence & Comparative SOTA (Increment C)")
    parser.add_argument("--input", type=pathlib.Path, required=True, help="Input JSON bundle with topic, objective, papers")
    parser.add_argument("--out-dir", type=pathlib.Path, default=pathlib.Path("sota_evidence_output"), help="Output directory")
    args = parser.parse_args(argv)
    if not args.input.is_file():
        print(f"ERROR: input file not found: {args.input}", file=sys.stderr)
        return 2
    bundle = json.loads(args.input.read_text(encoding="utf-8"))
    res = run_evidence_and_comparative_pipeline(bundle)
    outs = write_evidence_outputs(args.out_dir, res)
    print(f"EVIDENCE_SOTA_COMPLETE qualification={res['report_qualification']} verdict={res['quality_verdict']} outputs={[str(p) for p in outs]}")
    return 0
