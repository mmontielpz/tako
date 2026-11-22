"""Shared scientific integrity, provenance, bibliography, and telemetry helpers (Increment A / dev4)."""

from __future__ import annotations

import re
import unicodedata
import urllib.parse
from typing import Any

from tools.sota_discovery.sota import make_bib_key

BIB_STATUS_VERIFIED = "VERIFIED"
BIB_STATUS_MISMATCH = "MISMATCH"
BIB_STATUS_UNVERIFIABLE = "UNVERIFIABLE"
BIB_STATUS_NOT_CHECKED = "NOT_CHECKED"
BIB_STATUS_UNKNOWN = "UNKNOWN"
# Backward-compatible aliases
BIB_STATUS_UNAVAILABLE = BIB_STATUS_UNKNOWN
BIB_STATUS_NOT_VERIFIED = BIB_STATUS_NOT_CHECKED

EVIDENCE_MODEL_RECALL = "MODEL_RECALL"
EVIDENCE_METADATA_ONLY = "METADATA_ONLY"
EVIDENCE_ABSTRACT_BASED = "ABSTRACT_BASED"
EVIDENCE_FULL_TEXT = "FULL_TEXT"
EVIDENCE_UNAVAILABLE = "UNAVAILABLE"
EVIDENCE_UNKNOWN = "UNKNOWN"

CLAIM_SUPPORTED = "SUPPORTED"
CLAIM_PARTIALLY_SUPPORTED = "PARTIALLY_SUPPORTED"
CLAIM_UNSUPPORTED = "UNSUPPORTED"
CLAIM_NOT_VERIFIED = "NOT_VERIFIED"
CLAIM_NOT_CHECKED = "NOT_CHECKED"

HUMAN_PENDING = "PENDING"
HUMAN_ACCEPTED = "ACCEPTED"
HUMAN_REJECTED = "REJECTED"
HUMAN_NEEDS_CORRECTION = "NEEDS_CORRECTION"

_DOI_RE = re.compile(r"(10\.\d{4,9}/[-._;()/:A-Za-z0-9]+)", re.IGNORECASE)
_ARXIV_RE = re.compile(r"arxiv\.org/(?:abs|pdf)/([0-9]{4}\.[0-9]{4,5}(?:v\d+)?)", re.IGNORECASE)


def normalize_identifier(raw_id: Any) -> dict[str, str]:
    """Classify and normalize a reported DOI, URL, or arXiv identifier."""
    raw = str(raw_id or "").strip()
    if not raw or raw.upper() in ("UNKNOWN", "NOT_VERIFIED", "NONE", "N/A"):
        return {"kind": "none", "raw": raw, "normalized": ""}

    low = raw.lower()
    if "doi.org/" in low or low.startswith("doi:") or raw.startswith("10."):
        m = _DOI_RE.search(raw)
        if m:
            doi = m.group(1).rstrip(".,;)").lower()
            return {"kind": "doi", "raw": raw, "normalized": f"https://doi.org/{doi}"}

    arxiv_m = _ARXIV_RE.search(raw)
    if arxiv_m:
        aid = arxiv_m.group(1).lower()
        return {"kind": "arxiv", "raw": raw, "normalized": f"https://arxiv.org/abs/{aid}"}

    if low.startswith(("http://", "https://")):
        parsed = urllib.parse.urlsplit(raw)
        norm = urllib.parse.urlunsplit((
            parsed.scheme.lower(),
            parsed.netloc.lower(),
            parsed.path.rstrip("/") or "/",
            parsed.query,
            "",
        ))
        return {"kind": "url", "raw": raw, "normalized": norm}

    return {"kind": "other", "raw": raw, "normalized": raw}


def _norm_words(text: str) -> list[str]:
    ascii_text = unicodedata.normalize("NFKD", text).encode("ascii", "ignore").decode("ascii")
    return re.findall(r"[a-z0-9]+", ascii_text.lower())


def titles_match(reported_title: str, auth_title: str) -> bool:
    """Conservative title matching handling Unicode, punctuation, and subtitle variations."""
    a_words = _norm_words(reported_title)
    b_words = _norm_words(auth_title)
    if not a_words or not b_words:
        return False
    a_str = " ".join(a_words)
    b_str = " ".join(b_words)
    if a_str == b_str:
        return True
    # Subtitle match: only when shorter title has >= 3 words AND longer continues after a subtitle delimiter.
    shorter_raw, longer_raw = (reported_title.strip(), auth_title.strip()) if len(a_words) <= len(b_words) else (auth_title.strip(), reported_title.strip())
    shorter, longer = (a_str, b_str) if len(a_words) <= len(b_words) else (b_str, a_str)
    if len(shorter.split()) >= 3 and longer.startswith(shorter):
        # Check whether the raw longer title has a subtitle delimiter (:, -, —, () around the boundary.
        norm_prefix_pat = r"^\W*" + r"\W+".join(re.escape(w) for w in shorter.split()) + r"\s*[:\-—(]"
        ascii_longer = unicodedata.normalize("NFKD", longer_raw).encode("ascii", "ignore").decode("ascii").lower()
        if re.match(norm_prefix_pat, ascii_longer):
            return True
    a_set, b_set = set(a_words), set(b_words)
    jaccard = len(a_set & b_set) / max(1, len(a_set | b_set))
    return jaccard >= 0.75


def _extract_surnames(raw_authors: Any) -> set[str]:
    if isinstance(raw_authors, list):
        parts = [str(a).strip() for a in raw_authors if str(a).strip()]
    else:
        text = str(raw_authors or "").strip()
        if not text or text.upper() in ("UNKNOWN", "NOT_VERIFIED"):
            return set()
        if ";" in text:
            parts = [p.strip() for p in text.split(";") if p.strip()]
        elif " and " in text:
            parts = [p.strip() for p in text.split(" and ") if p.strip()]
        else:
            parts = [text]
    surnames: set[str] = set()
    for p in parts:
        clean = re.sub(r"\bet al\.?$", "", p, flags=re.IGNORECASE).strip()
        if not clean:
            continue
        if "," in clean:
            surname = clean.split(",", 1)[0].strip()
        else:
            tokens = clean.split()
            surname = tokens[-1] if tokens else ""
        ascii_surname = unicodedata.normalize("NFKD", surname).encode("ascii", "ignore").decode("ascii")
        norm = re.sub(r"[^a-z0-9]", "", ascii_surname.lower())
        if norm and norm not in ("unknown", "others"):
            surnames.add(norm)
    return surnames


def authors_match(reported_authors: Any, auth_authors: Any) -> tuple[bool, str]:
    """Check surname overlap when both reported and authoritative authors exist."""
    rep = _extract_surnames(reported_authors)
    auth = _extract_surnames(auth_authors)
    if not auth:
        return True, "no_authoritative_authors"
    if not rep:
        return True, "no_reported_authors"
    if rep & auth:
        return True, "matched"
    return False, "author_mismatch"


def _extract_oa_authors(oa_work: dict[str, Any]) -> list[str]:
    out: list[str] = []
    for a in oa_work.get("authorships") or []:
        name = ((a or {}).get("author") or {}).get("display_name") or ""
        if str(name).strip():
            out.append(str(name).strip())
    return out


def verify_paper_identity(
    paper: dict[str, Any],
    fetch_by_doi_fn: Any,
    search_fn: Any,
    seen_identities: set[str] | None = None,
) -> dict[str, Any]:
    """Independently verify paper bibliographic identity without fabricating identifiers."""
    title = str(paper.get("Title") or "").strip()
    year = paper.get("Year")
    raw_id = str(paper.get("DOI or source URL") or "").strip()
    norm_id = normalize_identifier(raw_id)
    discrepancy: list[str] = []

    base_record: dict[str, Any] = {
        "Title": title,
        "returned_year": year,
        "url": raw_id,
        "original_reported_identifier": raw_id,
        "normalized_identifier": norm_id,
        "retrieved_authoritative_metadata": None,
        "identity_verification_outcome": BIB_STATUS_UNVERIFIABLE,
        "discrepancy_evidence": discrepancy,
        "corrected_identity": None,
        "openalex_match": BIB_STATUS_UNVERIFIABLE,
    }

    if not title or title.upper() in ("UNKNOWN", "NOT_VERIFIED"):
        discrepancy.append("insufficient_metadata:missing_title")
        return base_record

    try:
        best: dict[str, Any] | None = None
        doi_resolved = False

        if norm_id["kind"] == "doi":
            best = fetch_by_doi_fn(norm_id["normalized"])
            if best:
                doi_resolved = True
            else:
                discrepancy.append("doi_unresolved")

        if best is None:
            hits = list(search_fn(title) or [])
            title_matches = [
                h for h in hits
                if titles_match(title, str(h.get("display_name") or h.get("title") or ""))
            ]
            if len(title_matches) > 1:
                discrepancy.append("ambiguous_multiple_matches")
                base_record["identity_verification_outcome"] = BIB_STATUS_UNVERIFIABLE
                base_record["openalex_match"] = BIB_STATUS_UNVERIFIABLE
                return base_record
            if len(title_matches) == 1:
                best = title_matches[0]
            elif hits:
                best = hits[0]

        if not best:
            discrepancy.append("no_authoritative_match")
            return base_record

        oa_title = str(best.get("display_name") or best.get("title") or "").strip()
        oa_year = best.get("publication_year")
        oa_doi_norm = normalize_identifier(best.get("doi") or "")
        oa_doi = oa_doi_norm["normalized"] or str(best.get("doi") or "").strip()
        oa_id = str(best.get("id") or "").strip()
        oa_authors = _extract_oa_authors(best)
        oa_landing = str(((best.get("primary_location") or {}).get("landing_page_url")) or "").strip()

        auth_meta = {
            "oa_id": oa_id,
            "oa_title": oa_title,
            "oa_year": oa_year,
            "oa_doi": oa_doi,
            "oa_authors": oa_authors,
            "oa_landing_page_url": oa_landing,
        }
        base_record["retrieved_authoritative_metadata"] = auth_meta
        base_record["oa_title"] = oa_title
        base_record["oa_year"] = oa_year
        base_record["oa_doi"] = oa_doi

        title_ok = titles_match(title, oa_title)
        if not title_ok:
            discrepancy.append("title_mismatch")

        year_ok = str(year) == str(oa_year) and oa_year is not None
        if not year_ok:
            if year in (None, "", "UNKNOWN", "NOT_VERIFIED") or oa_year is None:
                discrepancy.append("insufficient_metadata:missing_year")
            else:
                discrepancy.append("year_mismatch")

        auth_ok, auth_reason = authors_match(paper.get("Authors"), oa_authors)
        if not auth_ok:
            discrepancy.append(auth_reason)
        elif norm_id["kind"] != "doi" and auth_reason != "matched":
            # Without a matching DOI, missing author metadata on either side cannot establish positive identity.
            discrepancy.append(f"insufficient_metadata:{auth_reason}")
            auth_ok = False

        # Identifier consistency check
        id_ok = False
        if norm_id["kind"] == "doi":
            if doi_resolved and oa_doi_norm["normalized"] == norm_id["normalized"]:
                id_ok = True
            else:
                discrepancy.append("doi_mismatch")
        elif norm_id["kind"] in ("arxiv", "url"):
            # Legitimate non-DOI work verified by authoritative alternative metadata when title+year+author match.
            id_ok = title_ok and year_ok and auth_ok and bool(oa_id or oa_landing or oa_doi)
        elif norm_id["kind"] == "none":
            # Missing DOI/URL: only verifiable if authoritative record has matching title+year+author and an authoritative ID.
            id_ok = title_ok and year_ok and auth_ok and bool(oa_id or oa_doi)

        # Duplicate check across verified/seen identities
        canonical_key = oa_doi_norm["normalized"] or oa_id or f"{' '.join(_norm_words(oa_title))}:{oa_year}"
        is_dup = False
        if seen_identities is not None and canonical_key:
            if canonical_key in seen_identities:
                is_dup = True
                discrepancy.append("duplicate_record")
            else:
                seen_identities.add(canonical_key)

        if title_ok and year_ok and auth_ok and id_ok and not is_dup:
            outcome = BIB_STATUS_VERIFIED
            base_record["corrected_identity"] = {
                "title": oa_title,
                "year": oa_year,
                "doi": oa_doi or None,
                "openalex_id": oa_id or None,
                "url": oa_doi or oa_landing or oa_id or norm_id["normalized"],
                "authors": oa_authors,
            }
        elif is_dup and title_ok and year_ok and id_ok:
            outcome = BIB_STATUS_MISMATCH
        elif any(d in ("doi_mismatch", "title_mismatch", "year_mismatch", "author_mismatch") for d in discrepancy):
            outcome = BIB_STATUS_MISMATCH
        else:
            outcome = BIB_STATUS_UNVERIFIABLE

        base_record["identity_verification_outcome"] = outcome
        base_record["openalex_match"] = outcome
        return base_record
    except Exception as exc:
        if type(exc).__name__ == "HttpBudgetExhaustedError":
            raise
        discrepancy.append(f"verification_unavailable:{exc}")
        base_record["identity_verification_outcome"] = BIB_STATUS_UNKNOWN
        base_record["openalex_match"] = f"UNAVAILABLE ({exc})"
        return base_record


def format_bibtex_authors(raw_authors: Any) -> str:
    """Serialize authors using BibTeX 'and' separator without splitting on commas."""
    if isinstance(raw_authors, list):
        parts = [str(a).strip() for a in raw_authors if str(a).strip()]
    else:
        text = str(raw_authors or "").strip()
        if not text or text.upper() in ("UNKNOWN", "NOT_VERIFIED"):
            return "UNKNOWN"
        if ";" in text:
            parts = [p.strip() for p in text.split(";") if p.strip()]
        else:
            parts = [text]

    cleaned: list[str] = []
    for p in parts:
        if p.lower() in ("et al.", "et al"):
            cleaned.append("others")
        elif p.lower().endswith(" et al.") or p.lower().endswith(" et al"):
            head = re.sub(r"\s+et al\.?$", "", p, flags=re.IGNORECASE).strip()
            if head:
                cleaned.append(head)
            cleaned.append("others")
        else:
            cleaned.append(p)
    return " and ".join(cleaned) if cleaned else "UNKNOWN"


def _norm_sentence_key(s: str) -> str:
    return re.sub(r"\s+", " ", s.strip().rstrip(".;,!?")).lower()


def deduplicate_cue_sentences(
    contribution: str,
    results: str,
    limitations: str,
) -> tuple[str, str, str]:
    """Remove duplicate sentences across cue-extracted fields without losing distinct findings."""
    seen_res: set[str] = set()
    res_sents = [s.strip() for s in re.split(r"(?<=[.!?])\s+", results.strip()) if s.strip()]
    unique_res: list[str] = []
    for s in res_sents:
        k = _norm_sentence_key(s)
        if k not in seen_res:
            seen_res.add(k)
            unique_res.append(s)

    lim_sents = [s.strip() for s in re.split(r"(?<=[.!?])\s+", limitations.strip()) if s.strip()]
    unique_lim: list[str] = []
    seen_lim: set[str] = set()
    for s in lim_sents:
        k = _norm_sentence_key(s)
        if k in seen_lim or k in seen_res:
            continue
        seen_lim.add(k)
        unique_lim.append(s)

    final_res = " ".join(unique_res) if unique_res else results
    final_lim = (
        " ".join(unique_lim)
        if unique_lim
        else ("NOT_VERIFIED (no distinct limitation beyond reported results)" if lim_sents else limitations)
    )
    return contribution, final_res, final_lim


def deduplicate_text_items(items: list[Any]) -> list[Any]:
    """Deduplicate exact/whitespace-normalized items while preserving distinct findings."""
    seen: set[str] = set()
    out: list[Any] = []
    for item in items:
        key = _norm_sentence_key(str(item))
        if not key or key in seen:
            continue
        seen.add(key)
        out.append(item)
    return out


def classify_evidence_provenance(
    paper: dict[str, Any],
    provider: str,
    bib_check: dict[str, Any] | None,
    has_retrieved_abstract: bool = False,
    has_full_text: bool = False,
    human_review_status: str = HUMAN_PENDING,
) -> dict[str, str]:
    """Compute orthogonal M017A provenance statuses from system evidence, ignoring unsupported LLM claims."""
    raw_bib = (
        str(bib_check.get("identity_verification_outcome") or bib_check.get("openalex_match") or BIB_STATUS_NOT_CHECKED)
        if bib_check is not None
        else BIB_STATUS_NOT_CHECKED
    )
    if raw_bib.startswith("UNAVAILABLE"):
        bib_status = BIB_STATUS_UNKNOWN
    elif raw_bib == "NOT_VERIFIED":
        bib_status = BIB_STATUS_NOT_CHECKED
    elif raw_bib in (BIB_STATUS_VERIFIED, BIB_STATUS_MISMATCH, BIB_STATUS_UNVERIFIABLE, BIB_STATUS_NOT_CHECKED, BIB_STATUS_UNKNOWN):
        bib_status = raw_bib
    else:
        bib_status = BIB_STATUS_UNKNOWN

    prov = provider.lower()
    raw_label = str(paper.get("Evidence level") or "").strip().lower()

    if prov == "copilot-cli":
        ev_avail = EVIDENCE_MODEL_RECALL
    elif prov == "openalex":
        ev_avail = EVIDENCE_ABSTRACT_BASED if has_retrieved_abstract else EVIDENCE_METADATA_ONLY
    else:
        if has_full_text:
            ev_avail = EVIDENCE_FULL_TEXT
        elif has_retrieved_abstract:
            ev_avail = EVIDENCE_ABSTRACT_BASED
        elif raw_label in ("metadata-only", "metadata_only"):
            ev_avail = EVIDENCE_METADATA_ONLY
        elif raw_label in ("model-recall", "model_recall"):
            ev_avail = EVIDENCE_MODEL_RECALL
        elif raw_label == "unavailable":
            ev_avail = EVIDENCE_UNAVAILABLE
        elif raw_label == "unknown":
            ev_avail = EVIDENCE_UNKNOWN
        elif prov in ("claude-cli", "openai-responses"):
            # Unbacked LLM claim without retrieved source text is model recall, never abstract/full-text.
            ev_avail = EVIDENCE_MODEL_RECALL
        else:
            ev_avail = EVIDENCE_UNKNOWN

    valid_human = (HUMAN_PENDING, HUMAN_ACCEPTED, HUMAN_REJECTED, HUMAN_NEEDS_CORRECTION)
    hr_status = human_review_status if human_review_status in valid_human else HUMAN_PENDING

    return {
        "BIBLIOGRAPHIC_STATUS": bib_status,
        "EVIDENCE_AVAILABILITY": ev_avail,
        "CLAIM_SUPPORT_STATUS": CLAIM_NOT_CHECKED,
        "HUMAN_REVIEW_STATUS": hr_status,
    }


def partition_bibliography(
    papers: list[dict[str, Any]],
    bib_checks: list[dict[str, Any]] | None,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]], list[dict[str, Any]], dict[str, int]]:
    """Partition papers into accepted, candidate, and excluded with complete accounting."""
    accepted: list[dict[str, Any]] = []
    candidates: list[dict[str, Any]] = []
    excluded: list[dict[str, Any]] = []

    for idx, paper in enumerate(papers):
        chk = bib_checks[idx] if (bib_checks is not None and idx < len(bib_checks)) else None
        outcome = (
            str(chk.get("identity_verification_outcome") or chk.get("openalex_match") or BIB_STATUS_NOT_VERIFIED)
            if chk is not None
            else BIB_STATUS_NOT_VERIFIED
        )
        entry = {"paper": paper, "bib_check": chk, "outcome": outcome}
        if outcome == BIB_STATUS_VERIFIED:
            accepted.append(entry)
        elif chk is not None and "duplicate_record" in (chk.get("discrepancy_evidence") or []):
            excluded.append(entry)
        else:
            candidates.append(entry)

    accounting = {
        "total": len(papers),
        "accepted": len(accepted),
        "candidates": len(candidates),
        "excluded": len(excluded),
    }
    return accepted, candidates, excluded, accounting


def render_bibtex_entries(
    entries: list[dict[str, Any]],
    is_candidate: bool,
    used_keys: set[str],
) -> str:
    """Render accepted or candidate BibTeX entries without emitting mismatched DOIs."""
    rendered: list[str] = []
    for item in entries:
        p = item["paper"]
        chk = item.get("bib_check")
        outcome = item.get("outcome", BIB_STATUS_NOT_VERIFIED)
        authors_raw = p.get("Authors", "UNKNOWN")
        bib_authors = format_bibtex_authors(authors_raw)
        first_author = bib_authors.split(" and ")[0].split()[-1] if bib_authors != "UNKNOWN" else "unknown"
        rec_stub = {"_first_author": first_author, "Year": p.get("Year", "UNKNOWN"), "Title": p.get("Title", "paper")}
        key = make_bib_key(rec_stub, used_keys)

        venue = str(p.get("Publication venue") or "UNKNOWN")
        venue_line = f"  journal = {{{venue}}},\n" if venue != "UNKNOWN" else "  note = {Publication venue UNKNOWN},\n"

        if not is_candidate:
            corrected = (chk or {}).get("corrected_identity") or {}
            url = str(corrected.get("url") or p.get("DOI or source URL") or "UNKNOWN")
            doi_norm = normalize_identifier(corrected.get("doi") or url)
            doi_line = f"  doi = {{{doi_norm['normalized'].replace('https://doi.org/', '')}}},\n" if doi_norm["kind"] == "doi" else ""
            rendered.append(
                f"@article{{{key},\n"
                f"  title = {{{p.get('Title', 'UNKNOWN')}}},\n"
                f"  author = {{{bib_authors}}},\n"
                f"  year = {{{p.get('Year', 'UNKNOWN')}}},\n"
                f"{venue_line}{doi_line}"
                f"  url = {{{url}}}\n"
                f"}}"
            )
        else:
            reported_id = str(p.get("DOI or source URL") or "UNKNOWN")
            discrepancy = ", ".join((chk or {}).get("discrepancy_evidence") or [outcome])
            safe_url_line = ""
            rep_norm = normalize_identifier(reported_id)
            if rep_norm["kind"] in ("url", "arxiv") and outcome != BIB_STATUS_MISMATCH:
                safe_url_line = f"  url = {{{rep_norm['normalized']}}},\n"
            header = f"% Status: CANDIDATE (not accepted) | outcome={outcome} | reported_id={reported_id} | discrepancy={discrepancy}\n"
            rendered.append(
                f"{header}@misc{{{key},\n"
                f"  title = {{{p.get('Title', 'UNKNOWN')}}},\n"
                f"  author = {{{bib_authors}}},\n"
                f"  year = {{{p.get('Year', 'UNKNOWN')}}},\n"
                f"{safe_url_line}"
                f"  note = {{Candidate record (not accepted): outcome={outcome}; reported_id={reported_id}; discrepancy={discrepancy}}}\n"
                f"}}"
            )
    return "\n\n".join(rendered) + ("\n" if rendered else "")


def make_usage_record(
    *,
    cost_usd: float | str = "UNKNOWN",
    premium_requests: int | str = "UNKNOWN",
    request_count: int = 1,
    failed_request_count: int = 0,
    interrupted_request_count: int = 0,
    input_tokens: int | str = "UNKNOWN",
    output_tokens: int | str = "UNKNOWN",
    provider_specific_usage: dict[str, Any] | None = None,
    wall_time_seconds: float = 0.0,
    measurement_availability: str = "UNKNOWN",
    provenance: str = "UNKNOWN",
) -> dict[str, Any]:
    """Create a telemetry record keeping monetary cost and provider units strictly separate."""
    if isinstance(input_tokens, int) and isinstance(output_tokens, int):
        total_tokens: int | str = input_tokens + output_tokens
    else:
        total_tokens = "UNKNOWN"
    return {
        "cost_usd": cost_usd,
        "premium_requests": premium_requests,
        "request_count": request_count,
        "failed_request_count": failed_request_count,
        "interrupted_request_count": interrupted_request_count,
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "total_tokens": total_tokens,
        "provider_specific_usage": dict(provider_specific_usage or {}),
        "wall_time_seconds": round(float(wall_time_seconds), 3),
        "measurement_availability": measurement_availability,
        "provenance": provenance,
    }


def aggregate_usage_records(records: list[dict[str, Any]], wall_time_seconds: float) -> dict[str, Any]:
    """Aggregate usage records without conflating units or turning UNKNOWN into zero."""
    def agg_int(key: str) -> tuple[int | str, int | None]:
        vals = [r.get(key) for r in records]
        known = [v for v in vals if type(v) is int]
        if vals and len(known) == len(vals):
            return sum(known), None
        if known:
            return "UNKNOWN", sum(known)
        return "UNKNOWN", None

    cost_vals = [r.get("cost_usd") for r in records]
    known_costs = [float(v) for v in cost_vals if isinstance(v, (int, float)) and not isinstance(v, bool)]
    if cost_vals and len(known_costs) == len(cost_vals):
        total_cost: float | str = round(sum(known_costs), 6)
        partial_cost: float | None = None
    elif known_costs:
        total_cost = "UNKNOWN"
        partial_cost = round(sum(known_costs), 6)
    else:
        total_cost = "UNKNOWN"
        partial_cost = None

    in_tok, in_partial = agg_int("input_tokens")
    out_tok, out_partial = agg_int("output_tokens")
    prem_req, prem_partial = agg_int("premium_requests")
    tot_tok = (in_tok + out_tok) if (type(in_tok) is int and type(out_tok) is int) else "UNKNOWN"

    prov_specific: list[dict[str, Any]] = [r.get("provider_specific_usage") or {} for r in records if r.get("provider_specific_usage")]
    avail_vals = [str(r.get("measurement_availability") or "UNKNOWN") for r in records]
    if any(x is not None for x in (partial_cost, in_partial, out_partial, prem_partial)) or (
        len(set(avail_vals)) > 1 and "UNKNOWN" in avail_vals
    ):
        avail_label = "partial"
    else:
        avail_set = sorted(set(avail_vals))
        avail_label = ",".join(avail_set) if avail_set else "UNKNOWN"

    prov_set = sorted({str(r.get("provenance") or "UNKNOWN") for r in records})

    out: dict[str, Any] = {
        "cost_usd": total_cost,
        "premium_requests": prem_req,
        "request_count": sum(int(r.get("request_count") or 0) for r in records),
        "failed_request_count": sum(int(r.get("failed_request_count") or 0) for r in records),
        "interrupted_request_count": sum(int(r.get("interrupted_request_count") or 0) for r in records),
        "input_tokens": in_tok,
        "output_tokens": out_tok,
        "total_tokens": tot_tok,
        "provider_specific_usage": prov_specific,
        "wall_time_seconds": round(float(wall_time_seconds), 3),
        "measurement_availability": avail_label,
        "provenance": ",".join(prov_set) if prov_set else "UNKNOWN",
    }
    if partial_cost is not None:
        out["cost_usd_partial"] = partial_cost
    if prem_partial is not None:
        out["premium_requests_partial"] = prem_partial
    if in_partial is not None:
        out["input_tokens_partial"] = in_partial
    if out_partial is not None:
        out["output_tokens_partial"] = out_partial
    return out
