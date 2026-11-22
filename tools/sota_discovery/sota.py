"""Minimal State-of-the-Art (SOTA) literature discovery using OpenAlex (stdlib only)."""

from __future__ import annotations

import argparse
import csv
import json
import pathlib
import re
import sys
import urllib.error
import urllib.parse
import urllib.request
from typing import Any

OPENALEX_WORKS_URL = "https://api.openalex.org/works"
STOPWORDS = {
    "the", "and", "for", "with", "that", "this", "from", "into", "using", "based",
    "works", "work", "research", "identify", "foundational", "important", "methods",
    "methodological", "developments", "contributions", "limitations", "are", "was",
}
CONTRIB_CUES = ("propose", "introduce", "present", "develop", "formulate", "design", "review", "survey")
RESULT_CUES = ("achiev", "demonstrat", "show", "outperform", "accuracy", "error", "nrmse", "memory", "capacity", "benchmark", "%")
LIMIT_CUES = ("limit", "challeng", "bottleneck", "however", "although", "noise", "decoher", "scalab", "constraint", "open problem", "drawback")


class RetrievalError(RuntimeError):
    """Raised when live scholarly retrieval fails."""


def reconstruct_abstract(inverted_index: dict[str, list[int]] | None) -> str:
    """Reconstruct plain-text abstract from OpenAlex abstract_inverted_index."""
    if not inverted_index:
        return ""
    positions: list[tuple[int, str]] = []
    for word, idxs in inverted_index.items():
        for idx in idxs:
            positions.append((idx, word))
    if not positions:
        return ""
    positions.sort(key=lambda item: item[0])
    return " ".join(word for _, word in positions)


def tokenize(text: str) -> set[str]:
    return {t for t in re.findall(r"[a-z0-9]{3,}", text.lower()) if t not in STOPWORDS}


def split_sentences(text: str) -> list[str]:
    return [s.strip() for s in re.split(r"(?<=[.!?])\s+", text.strip()) if s.strip()]


def extract_by_cues(sentences: list[str], cues: tuple[str, ...], fallback: str) -> str:
    matched = [s for s in sentences if any(c in s.lower() for c in cues)]
    if not matched:
        return fallback
    return " ".join(matched[:2])


def fetch_openalex(query: str, per_page: int = 10, timeout: float = 15.0) -> list[dict[str, Any]]:
    params = urllib.parse.urlencode({"search": query, "per-page": str(per_page)})
    url = f"{OPENALEX_WORKS_URL}?{params}"
    req = urllib.request.Request(
        url,
        headers={"User-Agent": "TAKO-SOTA-Mini-MVP/0.1 (mailto:research-local@example.org)"},
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            if resp.status != 200:
                raise RetrievalError(f"OpenAlex HTTP {resp.status} for query {query!r}")
            payload = json.loads(resp.read().decode("utf-8"))
    except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as exc:
        raise RetrievalError(f"Live OpenAlex retrieval failed for {query!r}: {exc}") from exc
    return list(payload.get("results") or [])


def discover_works(
    topic: str,
    objective: str,
    seeds: list[str],
    max_papers: int = 10,
    fixture_path: pathlib.Path | None = None,
) -> tuple[list[dict[str, Any]], list[str], bool]:
    """Retrieve, deduplicate, and score candidate works. Returns (papers, related_topics, is_fixture)."""
    raw_works: list[dict[str, Any]] = []
    is_fixture = fixture_path is not None

    if fixture_path is not None:
        data = json.loads(fixture_path.read_text(encoding="utf-8"))
        raw_works = list(data.get("results", data) if isinstance(data, dict) else data)
    else:
        queries = [topic] + [s for s in seeds if s.lower() != topic.lower()]
        per_query = max(5, min(15, max_papers))
        for q in queries:
            raw_works.extend(fetch_openalex(q, per_page=per_query))

    dedup: dict[str, dict[str, Any]] = {}
    for w in raw_works:
        key = (w.get("doi") or w.get("id") or w.get("display_name") or w.get("title") or "").strip().lower()
        if key and key not in dedup:
            dedup[key] = w

    target_tokens = tokenize(f"{topic} {objective} {' '.join(seeds)}")
    topic_tokens = tokenize(topic)
    records: list[dict[str, Any]] = []
    related_counter: dict[str, int] = {}

    for w in dedup.values():
        title = (w.get("display_name") or w.get("title") or "UNKNOWN").strip()
        year = w.get("publication_year")
        abstract = reconstruct_abstract(w.get("abstract_inverted_index"))
        evidence_level = "abstract-based" if abstract else "metadata-only"

        authors_list = [
            (a.get("author") or {}).get("display_name", "").strip()
            for a in (w.get("authorships") or [])
        ]
        authors_list = [a for a in authors_list if a]
        authors = "; ".join(authors_list[:6]) + (" et al." if len(authors_list) > 6 else "")
        if not authors:
            authors = "UNKNOWN"

        primary_loc = w.get("primary_location") or {}
        source = primary_loc.get("source") or {}
        venue = (source.get("display_name") or "").strip() or "UNKNOWN"

        doi_or_url = (w.get("doi") or primary_loc.get("landing_page_url") or w.get("id") or "UNKNOWN").strip()
        cited_by = int(w.get("cited_by_count") or 0)

        concepts = [c.get("display_name", "").strip() for c in (w.get("concepts") or []) if c.get("display_name")]
        for c in concepts[:5]:
            related_counter[c] = related_counter.get(c, 0) + 1

        text_blob = f"{title} {abstract} {' '.join(concepts)}"
        work_tokens = tokenize(text_blob)
        overlap = sorted(target_tokens & work_tokens)
        topic_overlap = sorted(topic_tokens & work_tokens)
        seed_hits = [s for s in seeds if s.lower() in text_blob.lower()]
        score = len(overlap) + 2 * len(topic_overlap) + 2 * len(seed_hits)

        is_seminal_candidate = bool(
            (isinstance(year, int) and year <= 2017 and cited_by >= 50)
            or (cited_by >= 200 and len(overlap) >= 2)
        )
        seminal_label = (
            f"YES (candidate: year={year}, cited_by={cited_by}; not proof of quality)"
            if is_seminal_candidate
            else "NO"
        )

        if abstract:
            from tools.sota_integrity import deduplicate_cue_sentences

            sents = split_sentences(abstract)
            contribution = extract_by_cues(sents, CONTRIB_CUES, "NOT_VERIFIED (not explicitly stated in abstract)")
            results = extract_by_cues(sents, RESULT_CUES, "NOT_VERIFIED (no quantitative/empirical result in abstract)")
            limitations = extract_by_cues(sents, LIMIT_CUES, "NOT_VERIFIED (limitations not stated in abstract)")
            contribution, results, limitations = deduplicate_cue_sentences(contribution, results, limitations)
        else:
            contribution = "UNKNOWN (metadata-only; no abstract accessible)"
            results = "UNKNOWN (metadata-only; no abstract accessible)"
            limitations = "UNKNOWN (metadata-only; no abstract accessible)"

        rationale_parts = []
        if seed_hits:
            rationale_parts.append(f"seed match: {', '.join(seed_hits[:3])}")
        if overlap:
            rationale_parts.append(f"objective/topic terms ({len(overlap)}): {', '.join(overlap[:5])}")
        rationale = "; ".join(rationale_parts) if rationale_parts else "Retrieved via scholarly search; weak lexical overlap"

        from tools.sota_integrity import (
            BIB_STATUS_UNVERIFIABLE,
            BIB_STATUS_VERIFIED,
            classify_evidence_provenance,
        )

        bib_ok = bool(title and title != "UNKNOWN" and year is not None and (w.get("doi") or w.get("id")))
        bib_stub = {"identity_verification_outcome": BIB_STATUS_VERIFIED if bib_ok else BIB_STATUS_UNVERIFIABLE}
        prov = classify_evidence_provenance(
            {"Evidence level": evidence_level},
            provider="openalex",
            bib_check=bib_stub,
            has_retrieved_abstract=bool(abstract),
        )

        records.append({
            "Title": title,
            "Authors": authors,
            "Year": year if year is not None else "UNKNOWN",
            "Publication venue": venue,
            "DOI or source URL": doi_or_url,
            "Relevance rationale": rationale,
            "Seminal work candidate": seminal_label,
            "Research contribution": contribution,
            "Reported results": results,
            "Limitations": limitations,
            "Evidence level": evidence_level,
            "Bibliographic status": prov["BIBLIOGRAPHIC_STATUS"],
            "Evidence availability": prov["EVIDENCE_AVAILABILITY"],
            "Claim support status": prov["CLAIM_SUPPORT_STATUS"],
            "Human review status": prov["HUMAN_REVIEW_STATUS"],
            "_score": score,
            "_cited_by": cited_by,
            "_first_author": (authors_list[0].split()[-1] if authors_list else "unknown"),
        })

    records.sort(key=lambda r: (r["_score"], 1 if r["Evidence level"] == "abstract-based" else 0, r["_cited_by"]), reverse=True)
    selected = records[:max_papers]
    related_topics = [k for k, _ in sorted(related_counter.items(), key=lambda kv: kv[1], reverse=True)[:12]]
    return selected, related_topics, is_fixture


def make_bib_key(rec: dict[str, Any], used: set[str]) -> str:
    author = re.sub(r"[^a-z0-9]", "", str(rec["_first_author"]).lower()) or "author"
    year = str(rec["Year"]) if rec["Year"] != "UNKNOWN" else "nd"
    word = re.sub(r"[^a-z0-9]", "", (rec["Title"].split()[0] if rec["Title"].split() else "paper").lower())
    base = f"{author}{year}{word}"
    key = base
    idx = 2
    while key in used:
        key = f"{base}_{idx}"
        idx += 1
    used.add(key)
    return key


def write_outputs(
    out_dir: pathlib.Path,
    topic: str,
    objective: str,
    seeds: list[str],
    records: list[dict[str, Any]],
    related_topics: list[str],
    is_fixture: bool,
) -> list[pathlib.Path]:
    from tools.sota_integrity import BIB_STATUS_VERIFIED, render_bibtex_entries

    out_dir.mkdir(parents=True, exist_ok=True)
    matrix_path = out_dir / "literature_matrix.csv"
    bib_path = out_dir / "references.bib"
    candidates_bib_path = out_dir / "references_candidates.bib"
    report_path = out_dir / "sota_report.md"

    fields = [
        "Title", "Authors", "Year", "Publication venue", "DOI or source URL",
        "Relevance rationale", "Seminal work candidate", "Research contribution",
        "Reported results", "Limitations", "Evidence level",
        "Bibliographic status", "Evidence availability", "Claim support status", "Human review status",
    ]
    with matrix_path.open("w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=fields)
        writer.writeheader()
        for r in records:
            writer.writerow({k: r[k] for k in fields})

    used_keys: set[str] = set()
    accepted_items = [
        {"paper": r, "bib_check": None, "outcome": r.get("Bibliographic status", BIB_STATUS_VERIFIED)}
        for r in records
        if r.get("Bibliographic status") == BIB_STATUS_VERIFIED
    ]
    candidate_items = [
        {"paper": r, "bib_check": None, "outcome": r.get("Bibliographic status", "UNVERIFIABLE")}
        for r in records
        if r.get("Bibliographic status") != BIB_STATUS_VERIFIED
    ]
    bib_path.write_text(render_bibtex_entries(accepted_items, is_candidate=False, used_keys=used_keys), encoding="utf-8")
    candidates_bib_path.write_text(render_bibtex_entries(candidate_items, is_candidate=True, used_keys=used_keys), encoding="utf-8")

    chronological = sorted(records, key=lambda r: (int(r["Year"]) if isinstance(r["Year"], int) else 9999, r["Title"]))
    seminal = [r for r in records if str(r["Seminal work candidate"]).startswith("YES")]
    source_mode = "FIXTURE MODE (NOT LIVE SCIENTIFIC DISCOVERY)" if is_fixture else "Live OpenAlex API (metadata + reconstructed abstracts)"

    lines = [
        f"# State-of-the-Art (SOTA) Discovery Report: {topic}",
        "",
        f"- **Source Mode:** {source_mode}",
        f"- **Research Objective:** {objective}",
        f"- **Discovery Seeds:** {', '.join(seeds) if seeds else 'None'}",
        f"- **Papers Selected:** {len(records)} (bibliography: accepted={len(accepted_items)}, candidates={len(candidate_items)}, excluded=0)",
        "- **Evidence Disclaimer:** Extractions are strictly `abstract-based` or `metadata-only` (no full-text PDF parsing). Missing fields are marked `UNKNOWN` or `NOT_VERIFIED`. Citation counts and venues are not treated as proof of scientific superiority.",
        "",
    ]
    if len(accepted_items) == 0:
        lines.extend([
            "> **Empty Accepted Bibliography Notice:** `references.bib` is EMPTY (0 bibliographically verified entries). Unverified records are routed to `references_candidates.bib`. Process exit `0` indicates pipeline completion only, not scientific quality `PASS`.",
            "",
        ])
    lines.extend([
        "## 1. Related Research Topics Discovered",
        ", ".join(related_topics) if related_topics else "UNKNOWN",
        "",
        "## 2. Candidate Seminal & Foundational Works",
    ])
    if seminal:
        for r in seminal:
            lines.append(f"- **{r['Year']} — {r['Title']}** ({r['Authors']}) — {r['DOI or source URL']} (`{r['Evidence level']}`)")
    else:
        lines.append("- None flagged by heuristic; inspect `literature_matrix.csv` manually.")

    lines.extend(["", "## 3. Chronological Evolution"])
    for r in chronological:
        lines.extend([
            f"### {r['Year']} — {r['Title']}",
            f"- **Authors:** {r['Authors']} | **Venue:** {r['Publication venue']} | **Status:** Bibliographic=`{r.get('Bibliographic status', 'NOT_CHECKED')}` | Evidence=`{r.get('Evidence availability', r['Evidence level'])}` | Claim support=`{r.get('Claim support status', 'NOT_CHECKED')}`",
            f"- **Source:** {r['DOI or source URL']}",
            f"- **Contribution:** {r['Research contribution']}",
            f"- **Reported results:** {r['Reported results']}",
            f"- **Limitations:** {r['Limitations']}",
            "",
        ])

    lines.extend([
        "## 4. Known Limitations of This SOTA Run",
        "- Uses OpenAlex metadata and reconstructed abstracts only (`full-text` not parsed).",
        "- Sentence-level cue extraction can miss nuances; verify key claims against primary papers.",
        "- Seminal-work flags are heuristic candidates only, not peer-reviewed endorsements.",
    ])
    report_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return [report_path, matrix_path, bib_path, candidates_bib_path]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="TAKO SOTA Discovery Mini MVP (OpenAlex stdlib CLI)")
    parser.add_argument("--topic", required=True, help="Research topic")
    parser.add_argument("--objective", required=True, help="Research objective")
    parser.add_argument("--max-papers", type=int, default=10, help="Maximum number of papers (default: 10)")
    parser.add_argument("--seed", action="append", default=[], help="Related topic seed (repeatable)")
    parser.add_argument("--out-dir", default="sota_output", help="Output directory")
    parser.add_argument("--fixture", type=pathlib.Path, default=None, help="Optional offline JSON fixture for testing")
    parser.add_argument("--iterative", action="store_true", help="Run bounded coverage-aware iterative discovery (Increment B)")
    parser.add_argument("--max-rounds", type=int, default=3, help="Maximum discovery rounds (default: 3)")
    parser.add_argument("--max-requests", type=int, default=12, help="Maximum search requests (default: 12)")
    parser.add_argument("--citation-depth", type=int, default=1, help="Citation exploration depth (default: 1)")
    parser.add_argument("--no-review-first", action="store_true", help="Disable review-first discovery seed step")
    args = parser.parse_args(argv)

    if args.max_papers < 1 or args.max_rounds < 1 or args.max_requests < 1:
        print("ERROR: --max-papers, --max-rounds, and --max-requests must be >= 1", file=sys.stderr)
        return 2

    if args.iterative:
        from tools.sota_discovery.engine import build_research_plan, run_iterative_discovery, write_discovery_artifacts

        try:
            plan = build_research_plan(
                topic=args.topic,
                objective=args.objective,
                seeds=args.seed,
                max_papers=args.max_papers,
                max_rounds=args.max_rounds,
                max_requests=args.max_requests,
                citation_depth=args.citation_depth,
                prefer_review_first=not args.no_review_first,
            )
        except ValueError as exc:
            print(f"ERROR: {exc}", file=sys.stderr)
            return 2

        search_fn = None
        if args.fixture is not None:
            fx_data = json.loads(args.fixture.read_text(encoding="utf-8"))
            fx_list = list(fx_data.get("results", fx_data) if isinstance(fx_data, dict) else fx_data)
            search_fn = lambda q, n: fx_list[:n]

        res = run_iterative_discovery(plan, search_fn=search_fn)
        out_dir = pathlib.Path(args.out_dir)
        disc_outs = write_discovery_artifacts(out_dir, res)
        accepted_recs = list(res["buckets"]["ACCEPTED_CORPUS"].values())
        sota_outs = write_outputs(
            out_dir=out_dir,
            topic=args.topic,
            objective=args.objective,
            seeds=args.seed,
            records=accepted_recs,
            related_topics=[],
            is_fixture=(args.fixture is not None),
        )
        # Re-write discovery bib files so write_outputs does not overwrite segregated accepted/candidate split
        disc_outs = write_discovery_artifacts(out_dir, res)
        cov = res["coverage_report"]
        if not cov["coverage_satisfied"]:
            rep = out_dir / "sota_report.md"
            rep.write_text(
                rep.read_text(encoding="utf-8")
                + f"\n> **Coverage Notice:** Accepted coverage is INSUFFICIENT ({cov['covered_count']}/{cov['target_count']} dimensions covered; termination={res['termination_reason']}).\n",
                encoding="utf-8",
            )
        all_outs = sorted({str(p) for p in (sota_outs + disc_outs)})
        print(f"SOTA_ITERATIVE_COMPLETE termination={res['termination_reason']} accepted={len(accepted_recs)} outputs={all_outs}")
        return 0

    try:
        records, related, is_fixture = discover_works(
            topic=args.topic,
            objective=args.objective,
            seeds=args.seed,
            max_papers=args.max_papers,
            fixture_path=args.fixture,
        )
    except RetrievalError as exc:
        print(f"RETRIEVAL_BLOCKED: {exc}", file=sys.stderr)
        return 1

    outputs = write_outputs(
        out_dir=pathlib.Path(args.out_dir),
        topic=args.topic,
        objective=args.objective,
        seeds=args.seed,
        records=records,
        related_topics=related,
        is_fixture=is_fixture,
    )
    print(f"SOTA_COMPLETE papers={len(records)} fixture={is_fixture} outputs={[str(p) for p in outputs]}")
    return 0
