"""Bounded, coverage-aware iterative scientific discovery engine (Increment B)."""

from __future__ import annotations

import csv
import json
import pathlib
import re
import time
from typing import Any, Callable

from tools.sota_discovery.sota import (
    CONTRIB_CUES,
    LIMIT_CUES,
    RESULT_CUES,
    RetrievalError,
    extract_by_cues,
    fetch_openalex,
    reconstruct_abstract,
    split_sentences,
    tokenize,
)
from tools.sota_integrity import (
    BIB_STATUS_VERIFIED,
    classify_evidence_provenance,
    deduplicate_cue_sentences,
    normalize_identifier,
    partition_bibliography,
    render_bibtex_entries,
    verify_paper_identity,
)

# Screening outcomes
SCREEN_RELEVANT = "RELEVANT"
SCREEN_POTENTIALLY_RELEVANT = "POTENTIALLY_RELEVANT"
SCREEN_IRRELEVANT = "IRRELEVANT"
SCREEN_INSUFFICIENT_EVIDENCE = "INSUFFICIENT_EVIDENCE"

# Termination reasons
TERM_SATURATION = "SATURATION"
TERM_STAGNATION = "STAGNATION"
TERM_BUDGET_EXHAUSTED = "BUDGET_EXHAUSTED"
TERM_MAX_ROUNDS_REACHED = "MAX_ROUNDS_REACHED"
TERM_NO_VALID_QUERIES = "NO_VALID_QUERIES"
TERM_PROVIDER_FAILURE = "PROVIDER_FAILURE"
TERM_INSUFFICIENT_EVIDENCE = "INSUFFICIENT_EVIDENCE"

# Review-first states
RF_APPLICABLE = "REVIEW_FIRST_APPLICABLE"
RF_ATTEMPTED = "REVIEW_FIRST_ATTEMPTED"
RF_UNAVAILABLE = "REVIEW_FIRST_UNAVAILABLE"
RF_NOT_APPLICABLE = "REVIEW_FIRST_NOT_APPLICABLE"


def build_research_plan(
    *,
    topic: str,
    objective: str,
    seeds: list[str] | None = None,
    inclusion_criteria: list[str] | None = None,
    exclusion_criteria: list[str] | None = None,
    year_min: int | None = None,
    year_max: int | None = None,
    concept_groups: dict[str, list[str]] | None = None,
    max_papers: int = 10,
    max_rounds: int = 3,
    max_requests: int = 12,
    max_candidates: int = 100,
    citation_depth: int = 1,
    max_citation_nodes: int = 20,
    prefer_review_first: bool = True,
) -> dict[str, Any]:
    """Build an inspectable, deterministic research plan distinguishing user inputs from generated suggestions."""
    clean_topic = str(topic or "").strip()
    clean_obj = str(objective or "").strip()
    if not clean_topic:
        raise ValueError("Research topic must be non-empty.")
    if not clean_obj:
        raise ValueError("Research objective must be non-empty.")
    if max_papers < 1 or max_rounds < 1 or max_requests < 1:
        raise ValueError("Discovery budgets (max_papers, max_rounds, max_requests) must be >= 1.")

    user_seeds = [str(s).strip() for s in (seeds or []) if str(s).strip()]
    inc = [str(c).strip() for c in (inclusion_criteria or []) if str(c).strip()]
    exc = [str(c).strip() for c in (exclusion_criteria or []) if str(c).strip()]

    dimensions: dict[str, dict[str, Any]] = {}
    if concept_groups:
        for dim, terms in concept_groups.items():
            clean_terms = [str(t).strip() for t in terms if str(t).strip()]
            if clean_terms:
                dimensions[str(dim).strip()] = {"terms": clean_terms, "origin": "user_provided"}
    else:
        topic_terms = sorted(tokenize(clean_topic))
        if topic_terms:
            dimensions["core_topic"] = {"terms": topic_terms, "origin": "generated_from_topic"}
        for idx, s in enumerate(user_seeds):
            s_terms = sorted(tokenize(s)) or [s.lower()]
            dimensions[f"seed_{idx+1}_{re.sub(r'[^a-z0-9]+', '_', s.lower()).strip('_')}"] = {
                "terms": s_terms,
                "origin": "user_provided_seed",
            }
        obj_terms = sorted(tokenize(clean_obj) - set(topic_terms))
        if obj_terms:
            dimensions["objective_focus"] = {"terms": obj_terms[:6], "origin": "generated_from_objective"}

    return {
        "topic": clean_topic,
        "objective": clean_obj,
        "user_seeds": user_seeds,
        "inclusion_criteria": inc,
        "exclusion_criteria": exc,
        "temporal_constraints": {"year_min": year_min, "year_max": year_max},
        "target_dimensions": dimensions,
        "limits": {
            "max_papers": max_papers,
            "max_rounds": max_rounds,
            "max_requests": max_requests,
            "max_candidates": max_candidates,
            "citation_depth": citation_depth,
            "max_citation_nodes": max_citation_nodes,
        },
        "prefer_review_first": prefer_review_first,
    }


def _canonical_key(work: dict[str, Any]) -> str:
    doi_norm = normalize_identifier(work.get("doi") or "")
    if doi_norm["kind"] == "doi":
        return doi_norm["normalized"]
    oa_id = str(work.get("id") or "").strip().lower()
    if oa_id:
        return oa_id
    title = re.sub(r"[^a-z0-9]+", " ", str(work.get("display_name") or work.get("title") or "").lower()).strip()
    year = str(work.get("publication_year") or "nd")
    return f"{title}:{year}" if title else ""


def _work_to_record(w: dict[str, Any], plan: dict[str, Any], source_path: str) -> dict[str, Any]:
    title = str(w.get("display_name") or w.get("title") or "UNKNOWN").strip()
    year = w.get("publication_year")
    abstract = w.get("abstract") or reconstruct_abstract(w.get("abstract_inverted_index"))
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
    venue = str(source.get("display_name") or "").strip() or "UNKNOWN"
    doi_or_url = str(w.get("doi") or primary_loc.get("landing_page_url") or w.get("id") or "UNKNOWN").strip()
    cited_by = int(w.get("cited_by_count") or 0)
    concepts = [str(c.get("display_name") or "").strip() for c in (w.get("concepts") or []) if c.get("display_name")]

    if abstract:
        sents = split_sentences(abstract)
        contribution = extract_by_cues(sents, CONTRIB_CUES, "NOT_VERIFIED (not explicitly stated in abstract)")
        results = extract_by_cues(sents, RESULT_CUES, "NOT_VERIFIED (no quantitative/empirical result in abstract)")
        limitations = extract_by_cues(sents, LIMIT_CUES, "NOT_VERIFIED (limitations not stated in abstract)")
        contribution, results, limitations = deduplicate_cue_sentences(contribution, results, limitations)
    else:
        contribution = "UNKNOWN (metadata-only; no abstract accessible)"
        results = "UNKNOWN (metadata-only; no abstract accessible)"
        limitations = "UNKNOWN (metadata-only; no abstract accessible)"

    pub_type = str(w.get("type") or "UNKNOWN").strip().lower()
    is_review = bool(w.get("is_review") is True or pub_type == "review")

    return {
        "canonical_id": _canonical_key(w),
        "Title": title,
        "Authors": authors,
        "Year": year if year is not None else "UNKNOWN",
        "Publication venue": venue,
        "DOI or source URL": doi_or_url,
        "Relevance rationale": "",
        "Seminal work candidate": (
            f"YES (candidate: year={year}, cited_by={cited_by}; not proof of quality)"
            if ((isinstance(year, int) and year <= 2017 and cited_by >= 50) or cited_by >= 200)
            else "NO"
        ),
        "Research contribution": contribution,
        "Reported results": results,
        "Limitations": limitations,
        "Evidence level": evidence_level,
        "_abstract": abstract,
        "_concepts": concepts,
        "_cited_by": cited_by,
        "_pub_type": pub_type,
        "_pub_type_provenance": "provider_metadata" if w.get("type") else "UNKNOWN",
        "_is_review": is_review,
        "_raw_work": w,
        "_source_path": source_path,
        "_first_author": (authors_list[0].split()[-1] if authors_list else "unknown"),
    }


def screen_candidate(rec: dict[str, Any], plan: dict[str, Any]) -> dict[str, Any]:
    """Deterministic relevance screening separating identity from relevance and missing evidence."""
    title = rec["Title"]
    abstract = rec["_abstract"]
    concepts = rec["_concepts"]
    year = rec["Year"]
    text_blob = f"{title} {abstract} {' '.join(concepts)}".lower()

    # 1. Explicit exclusion criteria / temporal constraints
    t_cfg = plan["temporal_constraints"]
    if isinstance(year, int):
        if t_cfg["year_min"] is not None and year < t_cfg["year_min"]:
            return {"outcome": SCREEN_IRRELEVANT, "reason": f"outside_year_min:{year}<{t_cfg['year_min']}", "matched_dimensions": []}
        if t_cfg["year_max"] is not None and year > t_cfg["year_max"]:
            return {"outcome": SCREEN_IRRELEVANT, "reason": f"outside_year_max:{year}>{t_cfg['year_max']}", "matched_dimensions": []}

    for exc in plan["exclusion_criteria"]:
        if exc.lower() in text_blob:
            return {"outcome": SCREEN_IRRELEVANT, "reason": f"matched_exclusion:{exc}", "matched_dimensions": []}

    # 2. Concept dimension matching
    matched_dims: list[str] = []
    work_tokens = tokenize(text_blob)
    for dim, spec in plan["target_dimensions"].items():
        dim_terms = set(spec["terms"])
        if dim_terms & work_tokens or any(t.lower() in text_blob for t in dim_terms):
            matched_dims.append(dim)

    # 3. Inclusion criteria check
    inc_hits = [c for c in plan["inclusion_criteria"] if c.lower() in text_blob]
    inc_ok = (not plan["inclusion_criteria"]) or bool(inc_hits)

    # 4. Separate insufficient evidence from irrelevance
    if not abstract and not matched_dims:
        return {"outcome": SCREEN_INSUFFICIENT_EVIDENCE, "reason": "no_abstract_and_no_dimension_hit", "matched_dimensions": []}
    if not abstract and matched_dims:
        return {"outcome": SCREEN_POTENTIALLY_RELEVANT, "reason": f"metadata_only_dimension_hit:{','.join(matched_dims)}", "matched_dimensions": matched_dims}
    if matched_dims and inc_ok:
        return {"outcome": SCREEN_RELEVANT, "reason": f"matched_dimensions:{','.join(matched_dims)}", "matched_dimensions": matched_dims}
    if inc_hits and not matched_dims:
        return {"outcome": SCREEN_POTENTIALLY_RELEVANT, "reason": "inclusion_hit_without_dimension", "matched_dimensions": []}
    return {"outcome": SCREEN_IRRELEVANT, "reason": "no_target_dimension_overlap", "matched_dimensions": []}


def assess_accepted_coverage(
    accepted_records: list[dict[str, Any]],
    candidate_records: list[dict[str, Any]],
    plan: dict[str, Any],
) -> dict[str, Any]:
    """Compute coverage strictly over ACCEPTED_CORPUS (verified + relevant), never raw/irrelevant/unresolved."""
    target_dims = list(plan["target_dimensions"].keys())
    per_dim: dict[str, list[str]] = {d: [] for d in target_dims}
    for rec in accepted_records:
        for d in rec.get("_matched_dimensions") or []:
            if d in per_dim:
                per_dim[d].append(rec["canonical_id"])

    cand_per_dim: dict[str, list[str]] = {d: [] for d in target_dims}
    for rec in candidate_records:
        for d in rec.get("_matched_dimensions") or []:
            if d in cand_per_dim:
                cand_per_dim[d].append(rec["canonical_id"])

    covered = [d for d in target_dims if per_dim[d]]
    uncovered = [d for d in target_dims if not per_dim[d]]
    return {
        "target_dimensions": target_dims,
        "covered_dimensions": covered,
        "uncovered_dimensions": uncovered,
        "covered_count": len(covered),
        "target_count": len(target_dims),
        "accepted_count": len(accepted_records),
        "relevant_accepted_per_dimension": per_dim,
        "unresolved_candidate_contributions": cand_per_dim,
        "coverage_satisfied": bool(target_dims and not uncovered and len(accepted_records) >= 1),
    }


def _default_search_fn(query: str, per_page: int) -> list[dict[str, Any]]:
    return fetch_openalex(query, per_page=per_page)


def run_iterative_discovery(
    plan: dict[str, Any],
    *,
    search_fn: Callable[[str, int], list[dict[str, Any]]] | None = None,
    verify_doi_fn: Callable[[str], dict[str, Any] | None] | None = None,
    verify_search_fn: Callable[[str], list[dict[str, Any]]] | None = None,
    citation_fn: Callable[[dict[str, Any]], dict[str, list[dict[str, Any]]]] | None = None,
) -> dict[str, Any]:
    """Execute bounded, coverage-aware iterative discovery with full state accounting."""
    t0 = time.monotonic()
    s_fn = search_fn or _default_search_fn
    limits = plan["limits"]

    buckets: dict[str, dict[str, dict[str, Any]]] = {
        "RAW_CANDIDATES": {},
        "SCREENED_CANDIDATES": {},
        "BIBLIOGRAPHICALLY_VERIFIED": {},
        "EVIDENCE_QUALIFIED": {},
        "ACCEPTED_CORPUS": {},
        "CANDIDATES_FOR_LIMITED_USE": {},
        "EXCLUDED": {},
    }

    seen_queries: set[str] = set()
    executed_queries: list[dict[str, Any]] = []
    repeated_queries_prevented = 0
    search_requests = 0
    raw_observations = 0
    duplicate_observations = 0
    provider_failures: list[str] = []

    visited_citation_nodes: set[str] = set()
    citation_edges: list[dict[str, str]] = []
    citation_cycles_prevented = 0
    citation_capability = "AVAILABLE" if citation_fn is not None else "UNAVAILABLE"

    # Initial query queue
    query_queue: list[dict[str, Any]] = [
        {"query_text": plan["topic"], "origin": "user_topic", "target_dimensions": ["core_topic"]}
    ]
    for s in plan["user_seeds"]:
        query_queue.append({"query_text": s, "origin": "user_seed", "target_dimensions": []})

    # Review-first state
    review_first_status = RF_NOT_APPLICABLE
    if plan["prefer_review_first"]:
        review_first_status = RF_ATTEMPTED
        if search_requests < limits["max_requests"]:
            rq = f"{plan['topic']} review survey"
            seen_queries.add(rq.lower())
            search_requests += 1
            executed_queries.append({"round": 0, "query": rq, "origin": "review_first", "status": "ok"})
            try:
                rev_hits = list(s_fn(rq, min(5, limits["max_papers"])) or [])
                confirmed_reviews = [w for w in rev_hits if w.get("is_review") is True or str(w.get("type") or "").lower() == "review"]
                if confirmed_reviews:
                    review_first_status = RF_APPLICABLE
                    for w in confirmed_reviews:
                        for c in (w.get("concepts") or [])[:3]:
                            cname = str(c.get("display_name") or "").strip()
                            if cname and cname.lower() not in seen_queries:
                                query_queue.append({"query_text": cname, "origin": "review_first_concept", "target_dimensions": []})
                    for w in rev_hits:
                        raw_observations += 1
                        rec = _work_to_record(w, plan, "review_first")
                        if rec["canonical_id"] and rec["canonical_id"] not in buckets["RAW_CANDIDATES"]:
                            buckets["RAW_CANDIDATES"][rec["canonical_id"]] = rec
                        else:
                            duplicate_observations += 1
                else:
                    review_first_status = RF_UNAVAILABLE
            except Exception as exc:
                review_first_status = RF_UNAVAILABLE
                provider_failures.append(f"review_first:{exc}")
                executed_queries[-1]["status"] = f"error:{exc}"

    rounds_trace: list[dict[str, Any]] = []
    screening_rows: list[dict[str, Any]] = []
    seen_bib_identities: set[str] = set()
    termination_reason = TERM_MAX_ROUNDS_REACHED
    consecutive_stagnant_rounds = 0

    for rnd in range(1, limits["max_rounds"] + 1):
        round_queries_run = 0
        new_raw_this_round = 0
        round_errors = 0

        # Dequeue unique queries for this round up to request budget
        while query_queue and search_requests < limits["max_requests"] and len(buckets["RAW_CANDIDATES"]) < limits["max_candidates"]:
            qobj = query_queue.pop(0)
            qtext = str(qobj["query_text"] or "").strip()
            if not qtext:
                continue
            qnorm = qtext.lower()
            if qnorm in seen_queries:
                repeated_queries_prevented += 1
                continue
            seen_queries.add(qnorm)
            search_requests += 1
            round_queries_run += 1
            q_rec = {"round": rnd, "query": qtext, "origin": qobj["origin"], "status": "ok"}
            executed_queries.append(q_rec)
            try:
                hits = list(s_fn(qtext, limits["max_papers"]) or [])
                for w in hits:
                    if len(buckets["RAW_CANDIDATES"]) >= limits["max_candidates"]:
                        break
                    raw_observations += 1
                    rec = _work_to_record(w, plan, f"search:round_{rnd}:{qtext}")
                    cid = rec["canonical_id"]
                    if not cid or cid in buckets["RAW_CANDIDATES"]:
                        duplicate_observations += 1
                        continue
                    buckets["RAW_CANDIDATES"][cid] = rec
                    new_raw_this_round += 1
            except Exception as exc:
                round_errors += 1
                q_rec["status"] = f"error:{exc}"
                provider_failures.append(f"round_{rnd}:{qtext}:{exc}")

        # Bounded citation exploration on newly screened/accepted seeds
        if citation_fn is not None and limits["citation_depth"] >= 1 and len(visited_citation_nodes) < limits["max_citation_nodes"]:
            seeds_to_expand = [
                r for cid, r in list(buckets["RAW_CANDIDATES"].items())
                if cid not in visited_citation_nodes
            ][:3]
            for seed_rec in seeds_to_expand:
                if len(visited_citation_nodes) >= limits["max_citation_nodes"]:
                    break
                scid = seed_rec["canonical_id"]
                if scid in visited_citation_nodes:
                    citation_cycles_prevented += 1
                    continue
                visited_citation_nodes.add(scid)
                try:
                    c_out = citation_fn(seed_rec["_raw_work"]) or {}
                    for direction in ("backward", "forward"):
                        for cw in c_out.get(direction) or []:
                            if len(buckets["RAW_CANDIDATES"]) >= limits["max_candidates"]:
                                break
                            raw_observations += 1
                            crec = _work_to_record(cw, plan, f"citation:{direction}:{scid}")
                            ccid = crec["canonical_id"]
                            citation_edges.append({"seed_id": scid, "target_id": ccid, "direction": direction})
                            if ccid in visited_citation_nodes or ccid == scid:
                                citation_cycles_prevented += 1
                            if not ccid or ccid in buckets["RAW_CANDIDATES"]:
                                duplicate_observations += 1
                                continue
                            buckets["RAW_CANDIDATES"][ccid] = crec
                            new_raw_this_round += 1
                except Exception as exc:
                    provider_failures.append(f"citation:{scid}:{exc}")

        # Screen, verify identity, classify evidence, and route into 7 buckets
        accepted_gain = 0
        for cid, rec in list(buckets["RAW_CANDIDATES"].items()):
            if cid in buckets["ACCEPTED_CORPUS"] or cid in buckets["EXCLUDED"] or cid in buckets["CANDIDATES_FOR_LIMITED_USE"]:
                continue

            scr = screen_candidate(rec, plan)
            rec["_matched_dimensions"] = scr["matched_dimensions"]
            rec["Relevance rationale"] = scr["reason"]
            screening_rows.append({
                "canonical_id": cid,
                "title": rec["Title"],
                "round": rnd,
                "screening_outcome": scr["outcome"],
                "reason": scr["reason"],
                "source_path": rec["_source_path"],
            })

            if scr["outcome"] == SCREEN_IRRELEVANT:
                buckets["EXCLUDED"][cid] = rec
                continue

            buckets["SCREENED_CANDIDATES"][cid] = rec

            # Bibliographic identity verification
            if verify_doi_fn is not None or verify_search_fn is not None:
                bib_chk = verify_paper_identity(
                    rec,
                    fetch_by_doi_fn=verify_doi_fn or (lambda d: None),
                    search_fn=verify_search_fn or (lambda q: []),
                    seen_identities=seen_bib_identities,
                )
            else:
                # OpenAlex search records with valid id/doi + title + year are verified; missing ID/year -> unverifiable
                has_auth_id = bool(rec["_raw_work"].get("doi") or rec["_raw_work"].get("id")) and rec["Year"] != "UNKNOWN"
                bib_chk = {
                    "identity_verification_outcome": BIB_STATUS_VERIFIED if has_auth_id else "UNVERIFIABLE",
                    "discrepancy_evidence": [] if has_auth_id else ["insufficient_metadata"],
                    "corrected_identity": {"url": rec["DOI or source URL"], "doi": rec["_raw_work"].get("doi") or ""} if has_auth_id else None,
                }
            rec["_bib_check"] = bib_chk

            prov = classify_evidence_provenance(
                rec,
                provider="openalex",
                bib_check=bib_chk,
                has_retrieved_abstract=bool(rec["_abstract"]),
            )
            rec["Bibliographic status"] = prov["BIBLIOGRAPHIC_STATUS"]
            rec["Evidence availability"] = prov["EVIDENCE_AVAILABILITY"]
            rec["Claim support status"] = prov["CLAIM_SUPPORT_STATUS"]
            rec["Human review status"] = prov["HUMAN_REVIEW_STATUS"]

            if prov["BIBLIOGRAPHIC_STATUS"] == BIB_STATUS_VERIFIED:
                buckets["BIBLIOGRAPHICALLY_VERIFIED"][cid] = rec
                if scr["outcome"] == SCREEN_RELEVANT and rec["_abstract"]:
                    buckets["EVIDENCE_QUALIFIED"][cid] = rec
                    buckets["ACCEPTED_CORPUS"][cid] = rec
                    accepted_gain += 1
                else:
                    buckets["CANDIDATES_FOR_LIMITED_USE"][cid] = rec
            else:
                buckets["CANDIDATES_FOR_LIMITED_USE"][cid] = rec

        cov = assess_accepted_coverage(
            list(buckets["ACCEPTED_CORPUS"].values()),
            list(buckets["CANDIDATES_FOR_LIMITED_USE"].values()),
            plan,
        )
        cov["round"] = rnd
        cov["accepted_gain"] = accepted_gain
        cov["new_raw_this_round"] = new_raw_this_round
        rounds_trace.append(cov)

        # Termination & ARDE-ALG-01 refinement check
        if round_queries_run == 0 and not query_queue and new_raw_this_round == 0:
            if provider_failures and len(buckets["RAW_CANDIDATES"]) == 0:
                termination_reason = TERM_PROVIDER_FAILURE
            else:
                termination_reason = TERM_NO_VALID_QUERIES if rnd == 1 else TERM_STAGNATION
            break

        if round_errors > 0 and len(buckets["RAW_CANDIDATES"]) == 0:
            termination_reason = TERM_PROVIDER_FAILURE
            break

        if cov["coverage_satisfied"]:
            termination_reason = TERM_SATURATION
            break

        # ARDE-ALG-01: positive discovery gain with insufficient accepted coverage triggers bounded refinement
        if (new_raw_this_round > 0 or accepted_gain > 0) and cov["uncovered_dimensions"]:
            for udim in cov["uncovered_dimensions"]:
                terms = plan["target_dimensions"][udim]["terms"]
                ref_q = f"{plan['topic']} {' '.join(terms[:3])}".strip()
                if ref_q.lower() not in seen_queries:
                    query_queue.append({"query_text": ref_q, "origin": f"refinement:{udim}", "target_dimensions": [udim]})

        if accepted_gain == 0 and new_raw_this_round == 0:
            consecutive_stagnant_rounds += 1
        else:
            consecutive_stagnant_rounds = 0

        if search_requests >= limits["max_requests"] or len(buckets["RAW_CANDIDATES"]) >= limits["max_candidates"]:
            termination_reason = TERM_BUDGET_EXHAUSTED
            break

        if consecutive_stagnant_rounds >= 2 and not query_queue:
            termination_reason = TERM_STAGNATION
            break

        if not query_queue:
            if len(buckets["ACCEPTED_CORPUS"]) == 0 and len(buckets["SCREENED_CANDIDATES"]) > 0:
                termination_reason = TERM_INSUFFICIENT_EVIDENCE
            else:
                termination_reason = TERM_STAGNATION
            break
    else:
        if (
            len(buckets["ACCEPTED_CORPUS"]) == 0
            and len(buckets["SCREENED_CANDIDATES"]) > 0
            and all(not r["_abstract"] for r in buckets["SCREENED_CANDIDATES"].values())
        ):
            termination_reason = TERM_INSUFFICIENT_EVIDENCE
        else:
            termination_reason = TERM_MAX_ROUNDS_REACHED

    final_cov = assess_accepted_coverage(
        list(buckets["ACCEPTED_CORPUS"].values()),
        list(buckets["CANDIDATES_FOR_LIMITED_USE"].values()),
        plan,
    )
    accounting = {
        "raw_observations": raw_observations,
        "unique_canonical_identities": len(buckets["RAW_CANDIDATES"]),
        "duplicate_observations": duplicate_observations,
        "accepted": len(buckets["ACCEPTED_CORPUS"]),
        "candidates_for_limited_use": len(buckets["CANDIDATES_FOR_LIMITED_USE"]),
        "excluded": len(buckets["EXCLUDED"]),
        "verified_identities": len(buckets["BIBLIOGRAPHICALLY_VERIFIED"]),
    }
    assert accounting["accepted"] + accounting["candidates_for_limited_use"] + accounting["excluded"] == accounting["unique_canonical_identities"]

    return {
        "research_plan": plan,
        "review_first_status": review_first_status,
        "rounds": rounds_trace,
        "executed_queries": executed_queries,
        "screening_decisions": screening_rows,
        "citation_exploration": {
            "capability": citation_capability,
            "nodes_visited": len(visited_citation_nodes),
            "cycles_prevented": citation_cycles_prevented,
            "edges": citation_edges,
        },
        "coverage_report": final_cov,
        "state_accounting": accounting,
        "buckets": buckets,
        "measurements": {
            "raw_observations": raw_observations,
            "unique_canonical_identities": len(buckets["RAW_CANDIDATES"]),
            "duplicate_observations": duplicate_observations,
            "verified_identities": len(buckets["BIBLIOGRAPHICALLY_VERIFIED"]),
            "relevant_accepted_identities": len(buckets["ACCEPTED_CORPUS"]),
            "unresolved_candidates": len(buckets["CANDIDATES_FOR_LIMITED_USE"]),
            "explicit_exclusions": len(buckets["EXCLUDED"]),
            "covered_dimensions": final_cov["covered_count"],
            "target_dimensions": final_cov["target_count"],
            "search_requests": search_requests,
            "unique_queries": len(executed_queries),
            "repeated_queries_prevented": repeated_queries_prevented,
            "citation_nodes_visited": len(visited_citation_nodes),
            "citation_cycles_prevented": citation_cycles_prevented,
            "termination_reason": termination_reason,
            "wall_time_seconds": round(time.monotonic() - t0, 4),
            "provider_failures": provider_failures,
        },
        "termination_reason": termination_reason,
    }


def write_discovery_artifacts(out_dir: pathlib.Path, result: dict[str, Any]) -> list[pathlib.Path]:
    """Write machine-readable discovery artifacts alongside standard SOTA outputs."""
    out_dir.mkdir(parents=True, exist_ok=True)
    plan_p = out_dir / "research_plan.json"
    trace_p = out_dir / "discovery_trace.json"
    screen_p = out_dir / "screening_decisions.csv"
    cov_p = out_dir / "coverage_report.json"

    plan_p.write_text(json.dumps(result["research_plan"], indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    trace_p.write_text(json.dumps({
        "review_first_status": result["review_first_status"],
        "termination_reason": result["termination_reason"],
        "state_accounting": result["state_accounting"],
        "measurements": result["measurements"],
        "executed_queries": result["executed_queries"],
        "rounds": result["rounds"],
        "citation_exploration": result["citation_exploration"],
    }, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    cov_p.write_text(json.dumps(result["coverage_report"], indent=2, ensure_ascii=False) + "\n", encoding="utf-8")

    with screen_p.open("w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=["canonical_id", "title", "round", "screening_outcome", "reason", "source_path"])
        writer.writeheader()
        for row in result["screening_decisions"]:
            writer.writerow(row)

    # Write accepted & candidate bibliography using shared sota_integrity renderer
    used_keys: set[str] = set()
    accepted_items = [
        {"paper": r, "bib_check": r.get("_bib_check"), "outcome": r.get("Bibliographic status", BIB_STATUS_VERIFIED)}
        for r in result["buckets"]["ACCEPTED_CORPUS"].values()
    ]
    candidate_items = [
        {"paper": r, "bib_check": r.get("_bib_check"), "outcome": r.get("Bibliographic status", "UNVERIFIABLE")}
        for r in result["buckets"]["CANDIDATES_FOR_LIMITED_USE"].values()
    ]
    bib_p = out_dir / "references.bib"
    cand_bib_p = out_dir / "references_candidates.bib"
    bib_p.write_text(render_bibtex_entries(accepted_items, is_candidate=False, used_keys=used_keys), encoding="utf-8")
    cand_bib_p.write_text(render_bibtex_entries(candidate_items, is_candidate=True, used_keys=used_keys), encoding="utf-8")

    return [plan_p, trace_p, screen_p, cov_p, bib_p, cand_bib_p]
