"""Evidence acquisition, claim grounding, comparability, gap assessment, and Tier 1 quality remediation (Increment C)."""

from __future__ import annotations

import argparse
import csv
import difflib
import hashlib
import json
import math
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

# Minimum Viable SOTA qualification
REPORT_EXPLORATORY = "EXPLORATORY_LITERATURE_MAP"
REPORT_GROUNDED = "EVIDENCE_GROUNDED_COMPARATIVE_SOTA"

SUBSTANTIVE_EXTRACTION_FIELDS = (
    "research_problem",
    "proposed_contributions",
    "methodology",
    "datasets_or_materials",
    "evaluation_protocol",
    "baselines",
    "experimental_conditions",
    "environment",
    "results",
    "author_limitations",
)


def _sha256_text(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def _norm_ground_text(text: str) -> str:
    return re.sub(r"\s+", " ", re.sub(r"[^\w\s]", " ", text.lower())).strip()


def _lexical_match(excerpt: str, target_text: str, *, fuzzy_threshold: float, review_threshold: float) -> tuple[str, float]:
    q_norm = _norm_ground_text(excerpt)
    s_norm = _norm_ground_text(target_text)
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


def acquire_evidence(paper: dict[str, Any]) -> dict[str, Any]:
    """Acquire and normalize evidence provenance without conflating URLs or recall with full text."""
    paper_id = str(paper.get("paper_id") or paper.get("canonical_id") or paper.get("Title") or "unknown").strip()
    local_path = paper.get("local_text_path")
    full_text = str(paper.get("full_text") or "").strip()
    abstract = str(paper.get("abstract") or "").strip()
    model_desc = str(paper.get("model_recall_summary") or "").strip()
    pdf_url = str(paper.get("pdf_url") or "").strip()
    source_url = str(paper.get("DOI or source URL") or paper.get("source_url") or "UNKNOWN").strip()
    explicit_avail = str(paper.get("Evidence availability") or paper.get("availability_status") or "").strip()

    limitations: list[str] = []
    from_local_file = False
    if local_path:
        p = pathlib.Path(str(local_path))
        if p.is_file() and p.suffix.lower() in (".txt", ".md"):
            full_text = p.read_text(encoding="utf-8").strip()
            retrieval_method = "local_plain_text_file"
            from_local_file = True
        else:
            retrieval_method = "local_file_unsupported_or_missing"
            limitations.append("PDF parsing not supported without external parser or file unavailable.")
    elif full_text:
        retrieval_method = str(paper.get("retrieval_method") or "provided_plain_text")
    elif abstract:
        retrieval_method = str(paper.get("retrieval_method") or "metadata_abstract")
    elif model_desc or explicit_avail == EVIDENCE_MODEL_RECALL:
        retrieval_method = "model_recall_only"
    else:
        retrieval_method = "metadata_only"

    if pdf_url and not full_text:
        limitations.append("pdf_url present without parsed full-text; PDF parsing not supported.")

    researcher_declared_completeness = str(paper.get("document_completeness") or "UNDECLARED").strip()
    observed_structure = "NONE"

    if explicit_avail == EVIDENCE_MODEL_RECALL and not full_text:
        avail = EVIDENCE_MODEL_RECALL
        content = ""
        locator = "NONE"
        ext_method = "model_recall"
        ext_origin = "model_recall"
        doc_completeness = "NONE"
        limitations.append("Model-generated summary is not retrieved scientific evidence.")
    elif full_text:
        avail = EVIDENCE_FULL_TEXT
        content = full_text
        locator = str(paper.get("evidence_locator") or "full_text")
        ext_method = "plain_text_ingestion" if from_local_file else "researcher_supplied_text"
        ext_origin = "local_file" if from_local_file else "researcher_supplied_text"
        has_pages = bool(_extract_pages({"_text": content, "pages": paper.get("pages")}))
        has_secs = bool(_extract_sections({"_text": content, "sections": paper.get("sections")}))
        if has_pages and has_secs:
            observed_structure = "PAGINATED_AND_SECTIONED"
        elif has_secs:
            observed_structure = "SECTIONED"
        elif has_pages:
            observed_structure = "PAGINATED"
        else:
            observed_structure = "UNSTRUCTURED_TEXT"
        cve = paper.get("completeness_verification_evidence")
        has_valid_cve = (
            isinstance(cve, dict)
            and str(cve.get("status") or "").strip().upper() == "VERIFIED"
            and bool(str(cve.get("method") or "").strip())
            and bool(str(cve.get("verifier") or "").strip())
            and bool(str(cve.get("evidence_ref") or "").strip())
        )
        if paper.get("completeness_verified") is True and has_valid_cve and observed_structure != "UNSTRUCTURED_TEXT":
            doc_completeness = "VERIFIED_COMPLETE_PUBLICATION"
        elif researcher_declared_completeness == "FULL_PUBLICATION" or paper.get("completeness_verified") is True:
            doc_completeness = "DECLARED_COMPLETE_UNVERIFIED"
            limitations.append("Completeness declared without independent verification evidence; not promoted to VERIFIED_COMPLETE_PUBLICATION.")
        else:
            doc_completeness = "PARTIAL_EXCERPT"
            limitations.append("Supplied full_text is a partial excerpt; complete publication retrieval not verified.")
    elif abstract:
        avail = EVIDENCE_ABSTRACT_BASED
        content = abstract
        locator = str(paper.get("evidence_locator") or "abstract")
        ext_method = "abstract_text"
        ext_origin = "provider_abstract"
        doc_completeness = "ABSTRACT_ONLY"
        observed_structure = "ABSTRACT_ONLY"
    elif model_desc:
        avail = EVIDENCE_MODEL_RECALL
        content = ""
        locator = "NONE"
        ext_method = "model_recall"
        ext_origin = "model_recall"
        doc_completeness = "NONE"
    elif paper.get("Title"):
        avail = EVIDENCE_METADATA_ONLY
        content = ""
        locator = "metadata"
        ext_method = "metadata_only"
        ext_origin = "metadata_only"
        doc_completeness = "NONE"
    else:
        avail = EVIDENCE_UNAVAILABLE
        content = ""
        locator = "NONE"
        ext_method = "none"
        ext_origin = "unavailable"
        doc_completeness = "NONE"

    rec: dict[str, Any] = {
        "paper_id": paper_id,
        "evidence_source": str(paper.get("evidence_source") or source_url),
        "retrieval_method": retrieval_method,
        "availability_status": avail,
        "document_completeness": doc_completeness,
        "researcher_declared_completeness": researcher_declared_completeness,
        "observed_document_structure": observed_structure,
        "extraction_origin": ext_origin,
        "source_identifier": source_url,
        "content_hash": _sha256_text(content) if content else None,
        "evidence_locator": locator,
        "extraction_method": ext_method,
        "extraction_limitations": limitations,
        "_text": content,
    }
    if isinstance(paper.get("pages"), (dict, list)):
        rec["pages"] = paper["pages"]
    if isinstance(paper.get("sections"), dict):
        rec["sections"] = paper["sections"]
    return rec


def _extract_pages(ev_rec: dict[str, Any]) -> dict[int, str]:
    raw_pages = ev_rec.get("pages")
    pages: dict[int, str] = {}
    if isinstance(raw_pages, dict):
        for k, v in raw_pages.items():
            try:
                pages[int(k)] = str(v)
            except (TypeError, ValueError):
                continue
        return pages
    if isinstance(raw_pages, list):
        for idx, v in enumerate(raw_pages, start=1):
            pages[idx] = str(v)
        return pages
    text = str(ev_rec.get("_text") or "")
    matches = list(re.finditer(r"(?im)^(?:\[page\s+(\d+)\]|---\s*page\s+(\d+)\s*---)\s*$", text))
    if matches:
        for i, m in enumerate(matches):
            pnum = int(m.group(1) or m.group(2))
            start = m.end()
            end = matches[i + 1].start() if i + 1 < len(matches) else len(text)
            pages[pnum] = text[start:end].strip()
    return pages


def _extract_sections(ev_rec: dict[str, Any]) -> dict[str, str]:
    raw_secs = ev_rec.get("sections")
    if isinstance(raw_secs, dict):
        return {str(k).strip().lower(): str(v) for k, v in raw_secs.items()}
    text = str(ev_rec.get("_text") or "")
    matches = list(re.finditer(r"(?im)^(?:#{1,3}\s+|section\s+\d*\s*[:.-]\s*)([^\n:]+?):?\s*$", text))
    secs: dict[str, str] = {}
    if matches:
        for i, m in enumerate(matches):
            name = m.group(1).strip().lower()
            start = m.end()
            end = matches[i + 1].start() if i + 1 < len(matches) else len(text)
            secs[name] = text[start:end].strip()
    return secs


def verify_textual_correspondence_detailed(
    excerpt: str,
    locator: str,
    ev_rec: dict[str, Any],
    *,
    fuzzy_threshold: float = 0.85,
    review_threshold: float = 0.50,
    claim_paper_id: str | None = None,
) -> dict[str, Any]:
    """Deterministic ResearchOS-adapted Level 2 textual correspondence with actual locator checks."""
    if claim_paper_id and claim_paper_id != ev_rec["paper_id"]:
        return {
            "correspondence_status": CORR_SOURCE_MISMATCH,
            "similarity": 0.0,
            "global_similarity": 0.0,
            "location_verified": False,
            "location_status": "SOURCE_MISMATCH",
        }
    raw_text = str(ev_rec.get("_text") or "")
    if not raw_text:
        return {
            "correspondence_status": CORR_NOT_CHECKED,
            "similarity": 0.0,
            "global_similarity": 0.0,
            "location_verified": False,
            "location_status": "NO_EVIDENCE_TEXT",
        }

    loc = str(locator or "").strip()
    if not loc or loc.upper() in ("NONE", "INVALID", "MISSING"):
        return {
            "correspondence_status": CORR_INVALID_LOCATOR,
            "similarity": 0.0,
            "global_similarity": 0.0,
            "location_verified": False,
            "location_status": "INVALID_OR_MISSING_LOCATOR",
        }

    _, global_score = _lexical_match(excerpt, raw_text, fuzzy_threshold=fuzzy_threshold, review_threshold=review_threshold)
    target_text = raw_text
    loc_verified = False
    loc_status = "UNSTRUCTURED_GLOBAL_FALLBACK"

    if loc.startswith(("char:", "char_range:")):
        span_str = loc.split(":", 1)[1].strip()
        m = re.fullmatch(r"(\d+)\s*-\s*(\d+)", span_str)
        if not m:
            return {"correspondence_status": CORR_INVALID_LOCATOR, "similarity": 0.0, "global_similarity": global_score, "location_verified": False, "location_status": "INVALID_CHAR_RANGE"}
        start, end = int(m.group(1)), int(m.group(2))
        if start < 0 or end <= start or end > len(raw_text):
            return {"correspondence_status": CORR_INVALID_LOCATOR, "similarity": 0.0, "global_similarity": global_score, "location_verified": False, "location_status": "CHAR_RANGE_OUT_OF_BOUNDS"}
        target_text = raw_text[start:end]
        loc_verified = True
        loc_status = "CHAR_RANGE_VERIFIED"
    elif loc.startswith("line:"):
        span_str = loc.split(":", 1)[1].strip()
        m = re.fullmatch(r"(\d+)(?:\s*-\s*(\d+))?", span_str)
        if not m:
            return {"correspondence_status": CORR_INVALID_LOCATOR, "similarity": 0.0, "global_similarity": global_score, "location_verified": False, "location_status": "INVALID_LINE_RANGE"}
        s_line = int(m.group(1))
        e_line = int(m.group(2) or s_line)
        lines = raw_text.splitlines()
        if s_line < 1 or e_line < s_line or e_line > len(lines):
            return {"correspondence_status": CORR_INVALID_LOCATOR, "similarity": 0.0, "global_similarity": global_score, "location_verified": False, "location_status": "LINE_RANGE_OUT_OF_BOUNDS"}
        target_text = "\n".join(lines[s_line - 1:e_line])
        loc_verified = True
        loc_status = "LINE_RANGE_VERIFIED"
    elif loc.startswith("page:"):
        try:
            pnum = int(loc.split(":", 1)[1].strip())
        except ValueError:
            return {"correspondence_status": CORR_INVALID_LOCATOR, "similarity": 0.0, "global_similarity": global_score, "location_verified": False, "location_status": "INVALID_PAGE_NUMBER"}
        if pnum < 1:
            return {"correspondence_status": CORR_INVALID_LOCATOR, "similarity": 0.0, "global_similarity": global_score, "location_verified": False, "location_status": "INVALID_PAGE_NUMBER"}
        pages = _extract_pages(ev_rec)
        if pages:
            if pnum not in pages:
                return {"correspondence_status": CORR_INVALID_LOCATOR, "similarity": 0.0, "global_similarity": global_score, "location_verified": False, "location_status": "PAGE_NOT_FOUND"}
            target_text = pages[pnum]
            loc_verified = True
            loc_status = "PAGE_VERIFIED"
        else:
            loc_verified = False
            loc_status = "LOCATION_UNVERIFIED_NO_PAGE_METADATA"
    elif loc.startswith("section:"):
        sec_name = loc.split(":", 1)[1].strip().lower()
        if not sec_name:
            return {"correspondence_status": CORR_INVALID_LOCATOR, "similarity": 0.0, "global_similarity": global_score, "location_verified": False, "location_status": "EMPTY_SECTION_NAME"}
        sections = _extract_sections(ev_rec)
        if sections:
            matched_key = next((k for k in sections if sec_name == k or sec_name in k), None)
            if not matched_key:
                return {"correspondence_status": CORR_INVALID_LOCATOR, "similarity": 0.0, "global_similarity": global_score, "location_verified": False, "location_status": "SECTION_NOT_FOUND"}
            target_text = sections[matched_key]
            loc_verified = True
            loc_status = "SECTION_VERIFIED"
        else:
            loc_verified = False
            loc_status = "LOCATION_UNVERIFIED_UNSTRUCTURED"
    elif loc in ("abstract", "full_text"):
        loc_verified = True
        loc_status = f"{loc.upper()}_VERIFIED"
    else:
        return {
            "correspondence_status": CORR_INVALID_LOCATOR,
            "similarity": 0.0,
            "global_similarity": global_score,
            "location_verified": False,
            "location_status": "UNSUPPORTED_LOCATOR_FORMAT",
        }

    status, score = _lexical_match(excerpt, target_text, fuzzy_threshold=fuzzy_threshold, review_threshold=review_threshold)
    return {
        "correspondence_status": status,
        "similarity": score,
        "global_similarity": global_score,
        "location_verified": loc_verified and status in (CORR_EXACT, CORR_FUZZY),
        "location_status": loc_status,
    }


def verify_textual_correspondence(
    excerpt: str,
    locator: str,
    ev_rec: dict[str, Any],
    *,
    fuzzy_threshold: float = 0.85,
    review_threshold: float = 0.50,
    claim_paper_id: str | None = None,
) -> tuple[str, float]:
    """Return (correspondence_status, similarity_score) for a claim excerpt against evidence."""
    d = verify_textual_correspondence_detailed(
        excerpt,
        locator,
        ev_rec,
        fuzzy_threshold=fuzzy_threshold,
        review_threshold=review_threshold,
        claim_paper_id=claim_paper_id,
    )
    return d["correspondence_status"], d["similarity"]


def normalize_extraction(paper: dict[str, Any], ev_rec: dict[str, Any]) -> dict[str, Any]:
    """Normalize supplied scientific extraction fields with per-field provenance and correspondence."""
    ext = dict(paper.get("extraction") or {})
    avail = ev_rec["availability_status"]
    missing: list[str] = []
    field_provenance: dict[str, dict[str, Any]] = {}
    grounded_fields = 0
    unverified_prepop_fields = 0

    out: dict[str, Any] = {
        "paper_id": ev_rec["paper_id"],
        "provenance": "unverified_or_missing",
        "normalization_note": "Normalizes supplied fields only; does not perform automated semantic extraction.",
    }
    for f in SUBSTANTIVE_EXTRACTION_FIELDS:
        val = ext.get(f) or paper.get(f)
        if val in (None, "", []):
            out[f] = "NOT_VERIFIED"
            missing.append(f)
            field_provenance[f] = {
                "source_identity": ev_rec["paper_id"],
                "evidence_reference": ev_rec["source_identifier"],
                "source_content_hash": ev_rec["content_hash"],
                "evidence_locator": "NONE",
                "extraction_origin": "missing",
                "correspondence_status": CORR_NOT_CHECKED,
                "scientific_support_status": CLAIM_NOT_CHECKED,
            }
        else:
            out[f] = val
            val_text = ", ".join(str(x) for x in val) if isinstance(val, list) else str(val)
            if ev_rec.get("_text") and avail in (EVIDENCE_FULL_TEXT, EVIDENCE_ABSTRACT_BASED):
                corr, _ = verify_textual_correspondence(val_text, ev_rec["evidence_locator"], ev_rec)
                if corr in (CORR_EXACT, CORR_FUZZY):
                    origin = "author_reported"
                    grounded_fields += 1
                else:
                    origin = "unverified_prepopulated_field"
                    unverified_prepop_fields += 1
            else:
                corr = CORR_NOT_CHECKED
                origin = "unverified_prepopulated_field"
                unverified_prepop_fields += 1
            field_provenance[f] = {
                "source_identity": ev_rec["paper_id"],
                "evidence_reference": ev_rec["source_identifier"],
                "source_content_hash": ev_rec["content_hash"],
                "evidence_locator": ev_rec["evidence_locator"],
                "extraction_origin": origin,
                "correspondence_status": corr,
                "scientific_support_status": CLAIM_NOT_CHECKED,
            }

    if grounded_fields > 0 and unverified_prepop_fields == 0:
        out["provenance"] = "author_reported"
    elif grounded_fields > 0 and unverified_prepop_fields > 0:
        out["provenance"] = "mixed_unverified_fields"
    else:
        out["provenance"] = "unverified_or_missing"

    metrics = ext.get("reported_metrics") or paper.get("reported_metrics") or []
    out["reported_metrics"] = list(metrics) if isinstance(metrics, list) else []
    if not out["reported_metrics"]:
        missing.append("reported_metrics")
    out["missing_information"] = missing
    out["field_provenance"] = field_provenance
    return out


def _has_valid_assessment_evidence(raw_c: dict[str, Any]) -> bool:
    ae = raw_c.get("assessment_evidence")
    if not isinstance(ae, dict):
        return False
    return (
        str(ae.get("status") or "").strip().upper() == "VERIFIED"
        and bool(str(ae.get("method") or "").strip())
        and bool(str(ae.get("verifier") or "").strip())
        and bool(str(ae.get("evidence_ref") or "").strip())
    )


def register_claims(
    papers: list[dict[str, Any]],
    ev_by_id: dict[str, dict[str, Any]],
) -> list[dict[str, Any]]:
    """Register traceable claim records without equating textual match or caller strings with support."""
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
            corr_det = verify_textual_correspondence_detailed(
                excerpt,
                locator,
                target_ev,
                claim_paper_id=pid if c_paper_id != pid else None,
            )
            corr = corr_det["correspondence_status"]
            score = corr_det["similarity"]

            requested_support = str(raw_c.get("claim_support_status") or CLAIM_NOT_CHECKED)
            has_assess = _has_valid_assessment_evidence(raw_c)

            support = requested_support
            if ev["availability_status"] in (EVIDENCE_MODEL_RECALL, EVIDENCE_METADATA_ONLY, EVIDENCE_UNAVAILABLE, EVIDENCE_UNKNOWN):
                if support == CLAIM_SUPPORTED:
                    support = CLAIM_NOT_VERIFIED
            elif corr in (CORR_UNMATCHED, CORR_INVALID_LOCATOR, CORR_SOURCE_MISMATCH):
                if support == CLAIM_SUPPORTED:
                    support = CLAIM_UNSUPPORTED
            elif support == CLAIM_SUPPORTED and not has_assess:
                # Caller-supplied SUPPORTED without independent assessment_evidence is not verified
                support = CLAIM_NOT_VERIFIED

            claims.append({
                "claim_id": cid,
                "paper_id": pid,
                "claim_type": str(raw_c.get("claim_type") or "contribution"),
                "claim_text": str(raw_c.get("claim_text") or "").strip(),
                "evidence_availability": ev["availability_status"],
                "evidence_locator": locator,
                "location_verified": corr_det["location_verified"],
                "location_status": corr_det["location_status"],
                "global_similarity": corr_det["global_similarity"],
                "evidence_excerpt": excerpt,
                "source_content_hash": target_ev.get("content_hash"),
                "extraction_provenance": str(raw_c.get("extraction_provenance") or ev.get("extraction_method") or "UNKNOWN"),
                "textual_correspondence_status": corr,
                "textual_correspondence_score": score,
                "claim_support_status": support,
                "human_review_status": str(raw_c.get("human_review_status") or HUMAN_PENDING),
                "assessment_evidence": raw_c.get("assessment_evidence"),
                "claimed_value": raw_c.get("claimed_value"),
                "claimed_unit": raw_c.get("claimed_unit"),
                "metric_definition": raw_c.get("metric_definition"),
                "limitations": list(raw_c.get("limitations") or []),
                "requested_claim_support_status": requested_support,
            })
    return claims


def _normalize_metric_unit(val: Any, unit: str) -> tuple[float | None, str]:
    if isinstance(val, bool) or not isinstance(val, (int, float)):
        return None, unit.lower().strip()
    fval = float(val)
    if not math.isfinite(fval):
        return None, unit.lower().strip()
    u = unit.lower().strip()
    if u == "ms":
        return fval / 1000.0, "s"
    if u in ("s", "sec", "seconds"):
        return fval, "s"
    if u in ("ratio", "fraction") and 0.0 <= fval <= 1.0:
        return fval * 100.0, "%"
    if u in ("%", "percent", "percentage"):
        return fval, "%"
    return fval, u


def compare_papers(
    papers: list[dict[str, Any]],
    extractions: dict[str, dict[str, Any]],
    ev_by_id: dict[str, dict[str, Any]],
) -> list[dict[str, Any]]:
    """Pairwise comparative validity assessment across all applicable scientific dimensions."""
    comparisons: list[dict[str, Any]] = []
    for i in range(len(papers)):
        for j in range(i + 1, len(papers)):
            p1, p2 = papers[i], papers[j]
            id1 = str(p1.get("paper_id") or p1.get("canonical_id") or p1.get("Title"))
            id2 = str(p2.get("paper_id") or p2.get("canonical_id") or p2.get("Title"))
            e1, e2 = extractions[id1], extractions[id2]
            ev1, ev2 = ev_by_id[id1], ev_by_id[id2]

            reasons: list[str] = []
            for dim, label in (
                ("research_problem", "objective_or_task"),
                ("datasets_or_materials", "dataset_or_material"),
                ("evaluation_protocol", "evaluation_protocol"),
                ("experimental_conditions", "experimental_conditions"),
                ("environment", "environment_or_hardware"),
            ):
                if e1[dim] == "NOT_VERIFIED" or e2[dim] == "NOT_VERIFIED":
                    reasons.append(f"missing_{label}")
                elif e1[dim] != e2[dim]:
                    if dim in ("research_problem", "datasets_or_materials"):
                        reasons.append(f"different_{label}:{e1[dim]}!={e2[dim]}")
                    else:
                        reasons.append(f"different_{label}")

            m1_list = e1.get("reported_metrics") or []
            m2_list = e2.get("reported_metrics") or []
            metric_pairs: list[dict[str, Any]] = []
            metric_incompatible = False
            for m1 in m1_list:
                for m2 in m2_list:
                    n1 = str(m1.get("name") or "").strip().lower()
                    n2 = str(m2.get("name") or "").strip().lower()
                    d1 = str(m1.get("definition") or "").strip().lower()
                    d2 = str(m2.get("definition") or "").strip().lower()
                    if n1 == n2 and n1:
                        if d1 and d2 and d1 != d2:
                            metric_incompatible = True
                            reasons.append(f"ambiguous_metric_definition:{n1}")
                            continue
                        v1, u1 = _normalize_metric_unit(m1.get("value"), str(m1.get("unit") or ""))
                        v2, u2 = _normalize_metric_unit(m2.get("value"), str(m2.get("unit") or ""))
                        if v1 is None or v2 is None:
                            metric_incompatible = True
                            reasons.append(f"invalid_metric_value:{n1}")
                        elif u1 == u2:
                            metric_pairs.append({"metric": n1, "unit": u1, "v1": v1, "v2": v2})
                        else:
                            metric_incompatible = True
                            reasons.append(f"incompatible_metric_units:{n1}({u1} vs {u2})")
                    elif n1 and n2 and (n1 in n2 or n2 in n1):
                        metric_incompatible = True
                        reasons.append(f"incompatible_metric_definitions:{n1} vs {n2}")

            if ev1["availability_status"] in (EVIDENCE_METADATA_ONLY, EVIDENCE_MODEL_RECALL, EVIDENCE_UNAVAILABLE) or ev2["availability_status"] in (EVIDENCE_METADATA_ONLY, EVIDENCE_MODEL_RECALL, EVIDENCE_UNAVAILABLE):
                reasons.append("insufficient_evidence_availability")

            structural_reasons = [r for r in reasons if not r.startswith(("unverified_extraction_provenance", "insufficient_evidence_availability"))]
            structural_compat = "STRUCTURALLY_COMPATIBLE" if (not structural_reasons and metric_pairs) else "STRUCTURALLY_INCOMPATIBLE"

            if e1["provenance"] != "author_reported" or e2["provenance"] != "author_reported":
                reasons.append("unverified_extraction_provenance")

            if not reasons and metric_pairs:
                comp_class = COMP_DIRECT
            elif any(r.startswith(("different_objective", "different_dataset", "different_evaluation_protocol", "incompatible_metric", "ambiguous_metric", "invalid_metric")) for r in reasons) or metric_incompatible:
                comp_class = COMP_NOT
            elif reasons and any(r.startswith("missing_") for r in reasons):
                comp_class = COMP_NOT if not metric_pairs else COMP_CONDITIONAL
            elif not metric_pairs:
                comp_class = COMP_NOT
            else:
                comp_class = COMP_CONDITIONAL

            eligible_publish = (comp_class == COMP_DIRECT and len(metric_pairs) > 0 and e1["provenance"] == "author_reported" and e2["provenance"] == "author_reported")

            comparisons.append({
                "paper_1": id1,
                "paper_2": id2,
                "comparability_class": comp_class,
                "structural_compatibility": structural_compat,
                "verified_scientific_comparability": comp_class,
                "eligible_to_publish_numerical_comparisons": eligible_publish,
                "reasons": reasons or ["matching_dataset_protocol_conditions_and_metrics"],
                "normalized_metric_pairs": metric_pairs,
                "global_ranking_permitted": False,
            })
    return comparisons


def assess_research_gaps(
    accepted_papers: list[dict[str, Any]],
    extractions: dict[str, dict[str, Any]],
    claims: list[dict[str, Any]],
    comparisons: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    """Generate conservative research-gap records anchored to evidence and supported claims."""
    if not accepted_papers:
        return [{
            "gap_id": "GAP-ABSTAIN-1",
            "gap_type": GAP_INSUFFICIENT,
            "description": "Insufficient accepted papers to establish corpus-grounded research gaps.",
            "supporting_refs": [],
            "supporting_claim_ids": [],
            "provisional": True,
            "verification_status": "INSUFFICIENT_EVIDENCE",
            "novelty_claimed": False,
        }]

    claims_by_paper: dict[str, list[dict[str, Any]]] = {}
    for c in claims:
        claims_by_paper.setdefault(c["paper_id"], []).append(c)

    gaps: list[dict[str, Any]] = []
    for p in accepted_papers:
        pid = str(p.get("paper_id") or p.get("canonical_id") or p.get("Title"))
        ext = extractions[pid]
        lim = ext["author_limitations"]
        if lim != "NOT_VERIFIED":
            fp = (ext.get("field_provenance") or {}).get("author_limitations", {})
            grounded = fp.get("extraction_origin") == "author_reported"
            p_claims = [c["claim_id"] for c in claims_by_paper.get(pid, []) if c["claim_support_status"] == CLAIM_SUPPORTED]
            gaps.append({
                "gap_id": f"GAP-AUTH-{pid}",
                "gap_type": GAP_AUTHOR_LIM,
                "description": str(lim),
                "supporting_refs": [fp.get("evidence_reference") or pid],
                "supporting_claim_ids": p_claims,
                "provisional": not grounded,
                "verification_status": "EVIDENCE_GROUNDED" if grounded else "PROVISIONAL_UNVERIFIED",
                "novelty_claimed": False,
            })

    not_comp = [c for c in comparisons if c["comparability_class"] == COMP_NOT]
    if not_comp:
        gaps.append({
            "gap_id": "GAP-COMP-1",
            "gap_type": GAP_MISSING_COMP,
            "description": f"{len(not_comp)} paper pair(s) are NOT_COMPARABLE due to heterogeneous protocols/metrics/conditions.",
            "supporting_refs": [f"{c['paper_1']}<->{c['paper_2']}" for c in not_comp],
            "supporting_claim_ids": [],
            "provisional": False,
            "verification_status": "CORPUS_OBSERVED",
            "novelty_claimed": False,
        })

    if not gaps:
        gaps.append({
            "gap_id": "GAP-INSUFF-1",
            "gap_type": GAP_INSUFFICIENT,
            "description": "Available corpus claims do not establish a cross-corpus gap; absence in corpus does not imply absence in literature.",
            "supporting_refs": [c["claim_id"] for c in claims[:5]],
            "supporting_claim_ids": [c["claim_id"] for c in claims if c["claim_support_status"] == CLAIM_SUPPORTED][:5],
            "provisional": True,
            "verification_status": "INSUFFICIENT_EVIDENCE",
            "novelty_claimed": False,
        })
    return gaps


def _audit_numerical_claims(
    claims: list[dict[str, Any]],
    extractions: dict[str, dict[str, Any]] | None,
    ev_by_id: dict[str, dict[str, Any]] | None,
) -> list[dict[str, Any]]:
    audits: list[dict[str, Any]] = []
    for c in claims:
        if c.get("claim_type") != "numerical":
            continue
        pid = c["paper_id"]
        ext = (extractions or {}).get(pid, {})
        ev = (ev_by_id or {}).get(pid, {})
        reported_metrics = ext.get("reported_metrics") or []
        c_nums = re.findall(r"[-+]?\d*\.?\d+(?:[eE][-+]?\d+)?", c.get("claim_text") or "")
        claimed_val = c.get("claimed_value") if c.get("claimed_value") is not None else (c_nums[0] if c_nums else None)
        ev_val = None
        unit = c.get("claimed_unit") or "UNKNOWN"
        mdef = c.get("metric_definition") or "UNKNOWN"
        if reported_metrics:
            first_m = reported_metrics[0]
            ev_val = first_m.get("value")
            unit = str(first_m.get("unit") or unit)
            mdef = str(first_m.get("definition") or first_m.get("name") or mdef)

        corr_ok = c.get("textual_correspondence_status") in (CORR_EXACT, CORR_FUZZY)
        num_in_ev = claimed_val is not None and str(claimed_val) in str(ev.get("_text") or "")
        if corr_ok and (num_in_ev or (ev_val is not None and claimed_val is not None and str(claimed_val) == str(ev_val))):
            outcome = QM_PASS
        else:
            outcome = QM_FAIL

        audits.append({
            "claim_id": c["claim_id"],
            "claimed_value": claimed_val,
            "evidence_value": ev_val,
            "metric_definition": mdef,
            "unit": unit,
            "experimental_context": ext.get("evaluation_protocol", "NOT_VERIFIED"),
            "verification_method": "lexical_and_metric_cross_check",
            "verification_outcome": outcome,
            "evidence_ref": f"{ev.get('source_identifier', pid)}#{c.get('evidence_locator', 'NONE')}",
            "claim_support_status": c.get("claim_support_status", CLAIM_NOT_CHECKED),
        })
    return audits


def evaluate_tier1_quality(
    papers: list[dict[str, Any]],
    accepted_papers: list[dict[str, Any]],
    claims: list[dict[str, Any]],
    comparisons: list[dict[str, Any]],
    bib_checks: list[dict[str, Any]] | None,
    *,
    numerical_checks_executed: bool = True,
    extractions: dict[str, dict[str, Any]] | None = None,
    ev_by_id: dict[str, dict[str, Any]] | None = None,
) -> tuple[list[dict[str, Any]], str]:
    """Evaluate all 7 canonical M017A Tier 1 mandatory integrity gates plus completeness invariant."""
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

    # QM-E03: Numerical Accuracy (with per-claim audit records)
    num_audits = _audit_numerical_claims(claims, extractions, ev_by_id)
    if not numerical_checks_executed:
        metrics.append({"metric_id": "QM-E03", "name": "Numerical Accuracy", "numerator": None, "denominator": None, "observed_value": None, "threshold": "== 1.0", "measurement_status": QM_NOT_MEASURED, "gate_outcome": QM_FAIL, "evidence_ref": "numerical_checks_not_executed", "audit_records": []})
    elif not claims:
        metrics.append({"metric_id": "QM-E03", "name": "Numerical Accuracy", "numerator": 0, "denominator": 0, "observed_value": None, "threshold": "== 1.0", "measurement_status": QM_INSUFFICIENT, "gate_outcome": QM_FAIL, "evidence_ref": "claims (0 claims)", "audit_records": []})
    elif not num_audits:
        metrics.append({"metric_id": "QM-E03", "name": "Numerical Accuracy", "numerator": 0, "denominator": 0, "observed_value": None, "threshold": "== 1.0", "measurement_status": QM_NOT_APPLICABLE, "gate_outcome": QM_PASS, "evidence_ref": "no numerical claims", "audit_records": []})
    else:
        ok_num = sum(1 for a in num_audits if a["verification_outcome"] == QM_PASS)
        val = ok_num / len(num_audits)
        metrics.append({"metric_id": "QM-E03", "name": "Numerical Accuracy", "numerator": ok_num, "denominator": len(num_audits), "observed_value": round(val, 4), "threshold": "== 1.0", "measurement_status": QM_PASS if val == 1.0 else QM_FAIL, "gate_outcome": QM_PASS if val == 1.0 else QM_FAIL, "evidence_ref": "claims.json", "audit_records": num_audits})

    # QM-G04: Unsupported Claim Rate (canonical M017A: claims asserting SUPPORTED without valid grounding)
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

    # QM-G05: Critical Hallucination Count (fabricated quotes / mismatched sources; requires assessed corpus)
    supported_cnt = sum(1 for c in claims if c["claim_support_status"] == CLAIM_SUPPORTED)
    unassessed_cnt = sum(1 for c in claims if c["claim_support_status"] in (CLAIM_NOT_CHECKED, CLAIM_NOT_VERIFIED, CLAIM_PARTIALLY_SUPPORTED))
    fab_count = sum(1 for c in claims if c["textual_correspondence_status"] in (CORR_UNMATCHED, CORR_SOURCE_MISMATCH))
    if not claims or n_acc == 0:
        metrics.append({"metric_id": "QM-G05", "name": "Critical Hallucination Count", "numerator": 0, "denominator": 0, "observed_value": None, "threshold": "== 0", "measurement_status": QM_INSUFFICIENT, "gate_outcome": QM_FAIL, "evidence_ref": "empty_corpus_or_claims"})
    elif fab_count > 0:
        metrics.append({"metric_id": "QM-G05", "name": "Critical Hallucination Count", "numerator": fab_count, "denominator": len(claims), "observed_value": fab_count, "threshold": "== 0", "measurement_status": QM_FAIL, "gate_outcome": QM_FAIL, "evidence_ref": "claims.json"})
    elif supported_cnt == 0:
        metrics.append({"metric_id": "QM-G05", "name": "Critical Hallucination Count", "numerator": 0, "denominator": len(claims), "observed_value": None, "threshold": "== 0", "measurement_status": QM_NOT_MEASURED, "gate_outcome": QM_FAIL, "evidence_ref": "claims.json (0 scientifically supported claims; lexical check alone insufficient)"})
    else:
        metrics.append({"metric_id": "QM-G05", "name": "Critical Hallucination Count", "numerator": 0, "denominator": len(claims), "observed_value": 0, "threshold": "== 0", "measurement_status": QM_PASS, "gate_outcome": QM_PASS, "evidence_ref": "claims.json"})

    # QM-C03: Incompatible Comparison Rate (penalizes only published invalid direct comparisons, not abstention)
    if not comparisons:
        metrics.append({"metric_id": "QM-C03", "name": "Incompatible Comparison Rate", "numerator": 0, "denominator": 0, "observed_value": None, "threshold": "== 0.0", "measurement_status": QM_NOT_APPLICABLE if n_acc < 2 else QM_INSUFFICIENT, "gate_outcome": QM_PASS if n_acc < 2 else QM_FAIL, "evidence_ref": "comparability_assessments.json"})
    else:
        bad_comp = sum(
            1 for c in comparisons
            if c["comparability_class"] == COMP_DIRECT
            and (
                any(r != "matching_dataset_protocol_conditions_and_metrics" for r in c["reasons"])
                or c.get("global_ranking_permitted", False)
            )
        )
        val = bad_comp / len(comparisons)
        metrics.append({"metric_id": "QM-C03", "name": "Incompatible Comparison Rate", "numerator": bad_comp, "denominator": len(comparisons), "observed_value": round(val, 4), "threshold": "== 0.0", "measurement_status": QM_PASS if val == 0.0 else QM_FAIL, "gate_outcome": QM_PASS if val == 0.0 else QM_FAIL, "evidence_ref": "comparability_assessments.json"})

    if n_acc == 0 or not claims or supported_cnt == 0:
        overall = QM_INSUFFICIENT
    elif all(m["gate_outcome"] == QM_PASS for m in metrics) and unassessed_cnt == 0:
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
    raw_papers = [dict(p) for p in (bundle.get("papers") or [])]
    bib_checks = bundle.get("bib_checks")

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
        extractions=extractions, ev_by_id=ev_by_id,
    )

    remediation_trace: list[dict[str, Any]] = []
    quarantined_claims: list[dict[str, Any]] = []
    attempt = 0
    while overall != QM_PASS and attempt < max_remediation_attempts:
        attempt += 1
        actions: list[str] = []
        new_claims: list[dict[str, Any]] = []
        for c in claims:
            if c["textual_correspondence_status"] in (CORR_UNMATCHED, CORR_SOURCE_MISMATCH, CORR_INVALID_LOCATOR):
                qc = dict(c)
                qc["claim_support_status"] = CLAIM_UNSUPPORTED
                qc["remediated"] = True
                qc["quarantined"] = True
                quarantined_claims.append(qc)
                actions.append(f"quarantined_claim:{c['claim_id']}->{CLAIM_UNSUPPORTED}")
            else:
                new_claims.append(c)
        claims = new_claims

        comparisons = compare_papers(accepted_papers, extractions, ev_by_id)
        gaps = assess_research_gaps(accepted_papers, extractions, claims, comparisons)
        metrics, new_overall = evaluate_tier1_quality(
            raw_papers, accepted_papers, claims, comparisons, bib_checks,
            numerical_checks_executed=numerical_checks_executed,
            extractions=extractions, ev_by_id=ev_by_id,
        )
        remediation_trace.append({
            "attempt": attempt,
            "actions": actions,
            "verdict_after": new_overall,
        })
        overall = new_overall
        if not actions:
            break

    supported_cnt = sum(1 for c in claims if c["claim_support_status"] == CLAIM_SUPPORTED)
    completeness_ratio = round(supported_cnt / len(claims), 4) if claims else 0.0
    has_full_text = any(ev["availability_status"] == EVIDENCE_FULL_TEXT for ev in ev_manifest)
    report_qualification = (
        REPORT_GROUNDED
        if (has_full_text and overall == QM_PASS and len(accepted_papers) > 0 and len(claims) > 0 and supported_cnt == len(claims))
        else REPORT_EXPLORATORY
    )

    return {
        "topic": topic,
        "objective": objective,
        "discovery_provenance": bundle.get("discovery_provenance"),
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
        "claim_support_completeness": {
            "supported_claims": supported_cnt,
            "total_claims": len(claims),
            "completeness_ratio": completeness_ratio,
        },
        "remediation_trace": remediation_trace,
        "quarantined_claims": quarantined_claims,
        "report_qualification": report_qualification,
    }


def load_bundle_from_workflow_dir(workflow_dir: pathlib.Path) -> dict[str, Any]:
    """Adapt existing tako sota --iterative or tako sota-ai output directories into an evidence bundle."""
    matrix_p = workflow_dir / "literature_matrix.csv"
    if not matrix_p.is_file():
        raise FileNotFoundError(f"literature_matrix.csv not found in {workflow_dir}")

    papers: list[dict[str, Any]] = []
    with matrix_p.open(encoding="utf-8", newline="") as fh:
        for idx, row in enumerate(csv.DictReader(fh), start=1):
            title = str(row.get("Title") or "").strip()
            doi_url = str(row.get("DOI or source URL") or "").strip()
            avail = str(row.get("Evidence availability") or EVIDENCE_METADATA_ONLY).strip()
            contrib = str(row.get("Research contribution") or "").strip()
            res_text = str(row.get("Reported results") or "").strip()
            lim_text = str(row.get("Limitations") or "").strip()

            abstract_text = ""
            if avail == EVIDENCE_ABSTRACT_BASED:
                abstract_text = " ".join(x for x in (contrib, res_text) if x and x != "NOT_VERIFIED").strip()

            papers.append({
                "paper_id": f"wf_{idx}",
                "Title": title or "UNKNOWN",
                "Authors": [a.strip() for a in str(row.get("Authors") or "").split(";") if a.strip()] or ["UNKNOWN"],
                "Year": str(row.get("Year") or "UNKNOWN").strip(),
                "Venue": str(row.get("Publication venue") or "UNKNOWN").strip(),
                "DOI or source URL": doi_url or "UNKNOWN",
                "Bibliographic status": str(row.get("Bibliographic status") or "UNVERIFIABLE").strip(),
                "Evidence availability": avail,
                "abstract": abstract_text,
                "model_recall_summary": contrib if avail == EVIDENCE_MODEL_RECALL else "",
                "extraction": {
                    "proposed_contributions": "" if contrib in ("", "NOT_VERIFIED") else contrib,
                    "results": "" if res_text in ("", "NOT_VERIFIED") else res_text,
                    "author_limitations": "" if lim_text in ("", "NOT_VERIFIED") else lim_text,
                },
                "claims": [],
            })

    disc_prov: dict[str, Any] = {"source_workflow_dir": str(workflow_dir)}
    topic = "Workflow Imported SOTA Analysis"
    objective = "Evidence assessment of existing TAKO workflow artifacts"
    plan_p = workflow_dir / "research_plan.json"
    if plan_p.is_file():
        plan = json.loads(plan_p.read_text(encoding="utf-8"))
        topic = str(plan.get("topic") or topic)
        objective = str(plan.get("objective") or objective)
        disc_prov["research_plan"] = plan
    cov_p = workflow_dir / "coverage_report.json"
    if cov_p.is_file():
        disc_prov["coverage_report"] = json.loads(cov_p.read_text(encoding="utf-8"))
    scr_p = workflow_dir / "screening_decisions.csv"
    if scr_p.is_file():
        with scr_p.open(encoding="utf-8", newline="") as fh:
            disc_prov["screening_count"] = sum(1 for _ in csv.DictReader(fh))

    return {
        "topic": topic,
        "objective": objective,
        "discovery_provenance": disc_prov,
        "papers": papers,
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
    _dump("quality_measurements.json", {
        "verdict": res["quality_verdict"],
        "report_qualification": res["report_qualification"],
        "claim_support_completeness": res["claim_support_completeness"],
        "metrics": res["quality_measurements"],
    })
    _dump("remediation_trace.json", res["remediation_trace"])

    claims_by_paper: dict[str, list[dict[str, Any]]] = {}
    for c in res["claims"]:
        claims_by_paper.setdefault(c["paper_id"], []).append(c)

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
            p_claims = claims_by_paper.get(pid, [])
            c_status = p_claims[0]["claim_support_status"] if len(p_claims) == 1 else (
                "MIXED" if len({c["claim_support_status"] for c in p_claims}) > 1 else (p_claims[0]["claim_support_status"] if p_claims else CLAIM_NOT_CHECKED)
            )
            w.writerow({
                "paper_id": pid,
                "Title": p.get("Title", "UNKNOWN"),
                "Year": p.get("Year", "UNKNOWN"),
                "Bibliographic status": p.get("Bibliographic status", "NOT_CHECKED"),
                "Evidence availability": ev.get("availability_status", EVIDENCE_UNKNOWN),
                "Claim support status": c_status,
                "Human review status": p_claims[0]["human_review_status"] if p_claims else HUMAN_PENDING,
                "Methodology": ext.get("methodology", "NOT_VERIFIED"),
                "Datasets": ext.get("datasets_or_materials", "NOT_VERIFIED"),
            })
    paths.append(matrix_p)

    used_keys: set[str] = set()
    accepted_items = [{"paper": p, "bib_check": None, "outcome": BIB_STATUS_VERIFIED} for p in res["accepted_papers"]]
    candidate_items = [{"paper": p, "bib_check": None, "outcome": p.get("Bibliographic status", "UNVERIFIABLE")} for p in res["papers"] if p.get("Bibliographic status") != BIB_STATUS_VERIFIED]
    bib_p = out_dir / "references.bib"
    cand_p = out_dir / "references_candidates.bib"
    bib_p.write_text(render_bibtex_entries(accepted_items, is_candidate=False, used_keys=used_keys), encoding="utf-8")
    cand_p.write_text(render_bibtex_entries(candidate_items, is_candidate=True, used_keys=used_keys), encoding="utf-8")
    paths.extend([bib_p, cand_p])

    rep_p = out_dir / "sota_report.md"
    comp_stat = res["claim_support_completeness"]
    lines = [
        f"# State-of-the-Art Report: {res['topic']}",
        "",
        f"- **Report Qualification:** `{res['report_qualification']}`",
        f"- **Quality Gate Verdict:** `{res['quality_verdict']}`",
        f"- **Claim Support Completeness:** `{comp_stat['supported_claims']}/{comp_stat['total_claims']}` (`{comp_stat['completeness_ratio']}`)",
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
            f"- **Extraction Provenance:** `{ext.get('provenance', 'unverified_or_missing')}`",
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
        lines.append(f"- [{g['gap_type']}] ({g.get('verification_status', 'PROVISIONAL')}) {g['description']} (refs: {g['supporting_refs']})")
    lines.extend(["", "## 6. Evidence & Quality Gate Summary"])
    for m in res["quality_measurements"]:
        lines.append(f"- `{m['metric_id']}` ({m['name']}): `{m['gate_outcome']}` (`{m['measurement_status']}`)")
    if res["remediation_trace"]:
        lines.extend(["", "## 7. Remediation History"])
        for rt in res["remediation_trace"]:
            lines.append(f"- Attempt {rt['attempt']}: verdict=`{rt['verdict_after']}`, actions={rt['actions']}")
    rep_p.write_text("\n".join(lines) + "\n", encoding="utf-8")
    paths.append(rep_p)
    return paths


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="TAKO Evidence & Comparative SOTA (Increment C)")
    src = parser.add_mutually_exclusive_group(required=True)
    src.add_argument("--input", type=pathlib.Path, help="Input JSON bundle with topic, objective, papers")
    src.add_argument("--from-workflow-dir", type=pathlib.Path, help="Existing tako sota --iterative or tako sota-ai output directory")
    parser.add_argument("--out-dir", type=pathlib.Path, default=pathlib.Path("sota_evidence_output"), help="Output directory")
    args = parser.parse_args(argv)

    if args.input:
        if not args.input.is_file():
            print(f"ERROR: input file not found: {args.input}", file=sys.stderr)
            return 2
        bundle = json.loads(args.input.read_text(encoding="utf-8"))
    else:
        if not args.from_workflow_dir.is_dir():
            print(f"ERROR: workflow directory not found: {args.from_workflow_dir}", file=sys.stderr)
            return 2
        bundle = load_bundle_from_workflow_dir(args.from_workflow_dir)

    res = run_evidence_and_comparative_pipeline(bundle)
    outs = write_evidence_outputs(args.out_dir, res)
    print(f"EVIDENCE_SOTA_COMPLETE qualification={res['report_qualification']} verdict={res['quality_verdict']} outputs={[str(p) for p in outs]}")
    return 0
