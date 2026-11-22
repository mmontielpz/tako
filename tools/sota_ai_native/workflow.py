"""AI-native SOTA discovery workflow with sequential Researcher, Analyst, and optional Reviewer roles."""

from __future__ import annotations

import argparse
import csv
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.request
from typing import Any

from tools.sota_discovery.sota import fetch_openalex, make_bib_key

MATRIX_FIELDS = [
    "Title", "Authors", "Year", "Publication venue", "DOI or source URL",
    "Relevance rationale", "Seminal work candidate", "Research contribution",
    "Reported results", "Limitations", "Evidence level",
]


class ProviderBlockedError(RuntimeError):
    """Raised when provider-native research cannot run (missing creds or unauthorized paid API)."""


def call_openai_responses_web_search(
    prompt: str,
    model: str = "gpt-4.1-mini",
    allow_paid_api: bool = False,
    timeout: float = 45.0,
) -> tuple[str, dict[str, Any]]:
    if not allow_paid_api:
        raise ProviderBlockedError(
            "Paid OpenAI API execution requires explicit --allow-paid-api authorization."
        )
    api_key = os.environ.get("OPENAI_API_KEY", "").strip()
    if not api_key:
        raise ProviderBlockedError("OPENAI_API_KEY is not set in environment.")

    body = json.dumps({
        "model": model,
        "input": prompt,
        "tools": [{"type": "web_search_preview"}],
        "store": False,
    }).encode("utf-8")
    req = urllib.request.Request(
        "https://api.openai.com/v1/responses",
        data=body,
        headers={
            "Authorization": f"Bearer {api_key}",
            "Content-Type": "application/json",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            payload = json.loads(resp.read().decode("utf-8"))
    except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as exc:
        raise ProviderBlockedError(f"OpenAI Responses API request failed: {exc}") from exc

    text_chunks: list[str] = []
    for item in payload.get("output") or []:
        for content in item.get("content") or []:
            if content.get("type") == "output_text" and content.get("text"):
                text_chunks.append(content["text"])
    usage = payload.get("usage") or {}
    return "\n".join(text_chunks), {
        "input_tokens": usage.get("input_tokens", "UNKNOWN"),
        "output_tokens": usage.get("output_tokens", "UNKNOWN"),
        "cost_usd": "UNKNOWN",
    }


def parse_provider_json(raw: str, provider: str) -> dict[str, Any]:
    text = raw.strip()
    if not text:
        raise ProviderBlockedError(f"{provider} returned an empty response (0 bytes after stripping).")

    candidates: list[str] = [text]
    for fence_match in re.finditer(r"```(?:json)?\s*\n(.*?)```", text, flags=re.DOTALL | re.IGNORECASE):
        candidates.append(fence_match.group(1).strip())

    decoder = json.JSONDecoder()
    for cand in candidates:
        try:
            parsed = json.loads(cand)
            if isinstance(parsed, dict):
                return parsed
        except json.JSONDecodeError:
            pass
        found: list[dict[str, Any]] = []
        idx = cand.find("{")
        while idx != -1:
            try:
                parsed, end_idx = decoder.raw_decode(cand, idx)
                if isinstance(parsed, dict):
                    found.append(parsed)
                    idx = cand.find("{", end_idx)
                    continue
            except json.JSONDecodeError:
                pass
            idx = cand.find("{", idx + 1)
        if len(found) > 1:
            raise ProviderBlockedError(f"{provider} returned multiple competing JSON objects ({len(found)}).")
        if len(found) == 1:
            return found[0]

    preview = text[:160].replace("\n", " ")
    raise ProviderBlockedError(f"{provider} returned invalid JSON (no JSON object found; preview={preview!r}).")


VALID_EVIDENCE_LEVELS = {
    "metadata-only",
    "abstract-based",
    "full-text",
    "model-recall",
    "model_recall",
    "not_verified",
    "unknown",
}


def normalize_evidence_level(raw_level: Any, provider: str) -> str:
    label = str(raw_level or "").strip()
    low = label.lower()
    if low in VALID_EVIDENCE_LEVELS:
        if low == "not_verified":
            return "NOT_VERIFIED"
        if low == "unknown":
            return "UNKNOWN"
        if low in ("model-recall", "model_recall"):
            return "MODEL_RECALL"
        return low
    if provider == "copilot-cli":
        return f"MODEL_RECALL (unsupported_label={label!r})" if label else "MODEL_RECALL"
    return f"NOT_VERIFIED (unsupported_label={label!r})" if label else "NOT_VERIFIED"


def normalize_authors(raw_authors: Any) -> str:
    if isinstance(raw_authors, list):
        cleaned = [str(a).strip() for a in raw_authors if str(a).strip()]
        return "; ".join(cleaned) if cleaned else "UNKNOWN"
    text = str(raw_authors or "").strip()
    return text if text else "UNKNOWN"


def normalize_paper_record(paper: dict[str, Any], provider: str) -> dict[str, Any]:
    out = dict(paper)
    out["Authors"] = normalize_authors(paper.get("Authors"))
    out["Evidence level"] = normalize_evidence_level(paper.get("Evidence level"), provider)
    year = paper.get("Year")
    if isinstance(year, str) and year.strip().isdigit():
        out["Year"] = int(year.strip())
    return out


def validate_role_payload(role_name: str, payload: dict[str, Any], provider: str) -> dict[str, Any]:
    role = role_name.lower()
    if role in ("researcher", "analyst"):
        papers = payload.get("papers")
        if not isinstance(papers, list) or not all(isinstance(p, dict) and isinstance(p.get("Title"), str) and p["Title"].strip() for p in papers):
            raise ProviderBlockedError(f"{provider} {role} payload failed schema validation: 'papers' must be a list of paper dicts with 'Title'.")
        if role == "researcher" and "related_topics" in payload and not isinstance(payload["related_topics"], list):
            raise ProviderBlockedError(f"{provider} researcher payload failed schema validation: 'related_topics' must be a list.")
        payload = dict(payload)
        payload["papers"] = [normalize_paper_record(p, provider) for p in papers]
    elif role == "reviewer":
        keys = ("challenged_claims", "missing_foundational", "corrections")
        if not any(isinstance(payload.get(k), list) for k in keys):
            raise ProviderBlockedError(f"{provider} reviewer payload failed schema validation: missing review list fields.")
    return payload


def extract_copilot_jsonl_response(stdout: str) -> tuple[str, dict[str, Any]]:
    """Extract assistant text and usage metadata from Copilot CLI --output-format json (JSONL)."""
    usage: dict[str, Any] = {"input_tokens": "UNKNOWN", "output_tokens": "UNKNOWN", "cost_usd": "UNKNOWN"}
    messages: list[str] = []
    final_answers: list[str] = []
    event_types: list[str] = []
    parsed_any_jsonl = False

    for line in stdout.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            obj = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(obj, dict):
            continue
        parsed_any_jsonl = True
        ev_type = str(obj.get("type") or "")
        if ev_type:
            event_types.append(ev_type)
        data = obj.get("data") if isinstance(obj.get("data"), dict) else obj

        if ev_type in ("assistant.message", "message") or (
            isinstance(data, dict) and isinstance(data.get("content"), str) and ev_type not in ("user.message", "tool.execution_start", "tool.execution_complete")
        ):
            content = data.get("content")
            if isinstance(content, str) and content.strip():
                messages.append(content)
                if isinstance(data, dict) and data.get("phase") == "final_answer":
                    final_answers.append(content)
        elif isinstance(obj.get("result"), str) and obj["result"].strip():
            messages.append(obj["result"])

        if isinstance(obj.get("usage"), dict):
            u = obj["usage"]
            usage["input_tokens"] = u.get("input_tokens", u.get("inputTokens", usage["input_tokens"]))
            usage["output_tokens"] = u.get("output_tokens", u.get("outputTokens", usage["output_tokens"]))
            if "premiumRequests" in u and usage["cost_usd"] == "UNKNOWN":
                usage["cost_usd"] = f"premium_requests:{u['premiumRequests']}"
        if "total_cost_usd" in obj:
            usage["cost_usd"] = obj["total_cost_usd"]
        elif isinstance(data, dict) and "ai_credits" in data:
            usage["cost_usd"] = f"credits:{data['ai_credits']}"
        elif isinstance(data, dict) and "totalPremiumRequests" in data and usage["cost_usd"] == "UNKNOWN":
            usage["cost_usd"] = f"premium_requests:{data['totalPremiumRequests']}"

    candidates = final_answers if final_answers else messages
    if candidates:
        for msg in reversed(candidates):
            try:
                parse_provider_json(msg, "copilot-cli")
                return msg, usage
            except ProviderBlockedError:
                continue
        return candidates[-1], usage
    if parsed_any_jsonl:
        raise ProviderBlockedError(
            f"copilot-cli JSONL stream contained no assistant message (events={event_types[:10]})."
        )
    return stdout, usage


def call_claude_cli(prompt: str, allow_paid_api: bool = False, timeout: float = 90.0) -> tuple[str, dict[str, Any]]:
    if not allow_paid_api:
        raise ProviderBlockedError(
            "Claude CLI inference requires explicit --allow-paid-api authorization."
        )
    exe = shutil.which("claude")
    if not exe:
        raise ProviderBlockedError("claude CLI is not installed on PATH.")
    try:
        proc = subprocess.run(
            [exe, "-p", prompt, "--output-format", "json"],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
        )
    except subprocess.TimeoutExpired as exc:
        raise ProviderBlockedError(f"claude CLI timed out after {timeout}s.") from exc
    if proc.returncode != 0:
        raise ProviderBlockedError(f"claude CLI failed (exit {proc.returncode}): {proc.stderr.strip()}")
    try:
        data = json.loads(proc.stdout)
        text = data.get("result") or proc.stdout
        usage = {
            "input_tokens": (data.get("usage") or {}).get("input_tokens", "UNKNOWN"),
            "output_tokens": (data.get("usage") or {}).get("output_tokens", "UNKNOWN"),
            "cost_usd": data.get("total_cost_usd", "UNKNOWN"),
        }
    except json.JSONDecodeError:
        text = proc.stdout
        usage = {"input_tokens": "UNKNOWN", "output_tokens": "UNKNOWN", "cost_usd": "UNKNOWN"}
    return text, usage


def call_copilot_cli(prompt: str, allow_paid_api: bool = False, timeout: float = 120.0) -> tuple[str, dict[str, Any]]:
    if not allow_paid_api:
        raise ProviderBlockedError(
            "Copilot CLI inference requires explicit --allow-paid-api authorization."
        )
    exe = shutil.which("copilot")
    if not exe:
        raise ProviderBlockedError("copilot CLI is not installed on PATH.")
    try:
        proc = subprocess.run(
            [
                exe,
                "-p", prompt,
                "--output-format", "json",
                "--stream", "off",
                "--no-ask-user",
                "--no-custom-instructions",
                "--disable-builtin-mcps",
            ],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
        )
    except subprocess.TimeoutExpired as exc:
        raise ProviderBlockedError(f"copilot CLI timed out after {timeout}s.") from exc
    if proc.returncode != 0:
        err_preview = proc.stderr.strip()[:200] or proc.stdout.strip()[:200]
        raise ProviderBlockedError(f"copilot CLI failed (exit {proc.returncode}): {err_preview}")
    if not proc.stdout.strip():
        err_preview = proc.stderr.strip()[:200]
        raise ProviderBlockedError(
            f"copilot CLI returned empty stdout (stderr={err_preview!r})."
        )
    return extract_copilot_jsonl_response(proc.stdout)


def run_role(
    role_name: str,
    prompt: str,
    provider: str,
    model: str,
    allow_paid_api: bool,
    fixture_data: dict[str, Any] | None,
) -> tuple[dict[str, Any], dict[str, Any]]:
    if provider == "fixture":
        assert fixture_data is not None
        role_payload = fixture_data.get(role_name.lower()) or {}
        return role_payload, {"input_tokens": 0, "output_tokens": 0, "cost_usd": 0.0}
    if provider == "openai-responses":
        raw, usage = call_openai_responses_web_search(prompt, model=model, allow_paid_api=allow_paid_api)
        return validate_role_payload(role_name, parse_provider_json(raw, provider), provider), usage
    if provider == "claude-cli":
        raw, usage = call_claude_cli(prompt, allow_paid_api=allow_paid_api)
        return validate_role_payload(role_name, parse_provider_json(raw, provider), provider), usage
    if provider == "copilot-cli":
        raw, usage = call_copilot_cli(prompt, allow_paid_api=allow_paid_api)
        return validate_role_payload(role_name, parse_provider_json(raw, provider), provider), usage
    raise ProviderBlockedError(f"Unsupported provider: {provider}")


def verify_bibliographic_records(papers: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Optional deterministic OpenAlex verification preserving original returned metadata."""
    checks: list[dict[str, Any]] = []
    for p in papers:
        title = str(p.get("Title") or "")
        year = p.get("Year")
        url = str(p.get("DOI or source URL") or "")
        try:
            best = None
            if "doi.org/" in url.lower():
                doi_url = "https://api.openalex.org/works/" + url.strip()
                req = urllib.request.Request(doi_url, headers={"User-Agent": "TAKO-SOTA-Mini-MVP/0.1"})
                try:
                    with urllib.request.urlopen(req, timeout=10.0) as resp:
                        if resp.status == 200:
                            best = json.loads(resp.read().decode("utf-8"))
                except Exception:
                    best = None
            if best is None:
                hits = fetch_openalex(title, per_page=3, timeout=10.0)
                for h in hits:
                    htitle = (h.get("display_name") or "").lower()
                    if title.lower()[:25] in htitle or htitle[:25] in title.lower():
                        best = h
                        break
                if best is None and hits:
                    best = hits[0]

            if best:
                oa_title = (best.get("display_name") or "").strip()
                oa_year = best.get("publication_year")
                oa_doi = (best.get("doi") or "").strip()
                title_match = title.lower()[:25] in oa_title.lower() or oa_title.lower()[:25] in title.lower()
                year_match = str(year) == str(oa_year)
                doi_match = (not url or url in ("UNKNOWN", "NOT_VERIFIED")) or (url.lower().rstrip("/") == oa_doi.lower().rstrip("/"))
                status = "VERIFIED" if (title_match and year_match and doi_match) else "MISMATCH"
                checks.append({
                    "Title": title,
                    "returned_year": year,
                    "url": url,
                    "oa_title": oa_title,
                    "oa_year": oa_year,
                    "oa_doi": oa_doi,
                    "openalex_match": status,
                })
            else:
                checks.append({"Title": title, "returned_year": year, "url": url, "openalex_match": "UNVERIFIABLE"})
        except Exception as exc:
            checks.append({"Title": title, "returned_year": year, "url": url, "openalex_match": f"UNAVAILABLE ({exc})"})
    return checks


def _stringify_item(item: Any) -> str:
    if isinstance(item, str):
        return item.strip()
    if isinstance(item, dict):
        return json.dumps(item, ensure_ascii=False)
    if isinstance(item, list):
        return "; ".join(_stringify_item(x) for x in item if _stringify_item(x))
    return str(item).strip()


def _save_stage_json(out_dir: pathlib.Path, filename: str, data: Any) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / filename).write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def write_ai_native_outputs(
    out_dir: pathlib.Path,
    topic: str,
    objective: str,
    provider: str,
    researcher_out: dict[str, Any],
    analyst_out: dict[str, Any],
    reviewer_out: dict[str, Any] | None,
    bib_checks: list[dict[str, Any]] | None,
    usage_totals: dict[str, Any],
    elapsed_seconds: float,
) -> list[pathlib.Path]:
    out_dir.mkdir(parents=True, exist_ok=True)
    is_fixture = provider == "fixture"
    papers = list(analyst_out.get("papers") or researcher_out.get("papers") or [])
    related_topics = list(researcher_out.get("related_topics") or [])

    matrix_path = out_dir / "literature_matrix.csv"
    with matrix_path.open("w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=MATRIX_FIELDS)
        writer.writeheader()
        for p in papers:
            row = {k: p.get(k, "NOT_VERIFIED") or "NOT_VERIFIED" for k in MATRIX_FIELDS}
            writer.writerow(row)

    bib_path = out_dir / "references.bib"
    used_keys: set[str] = set()
    bib_entries: list[str] = []
    for p in papers:
        authors = str(p.get("Authors") or "UNKNOWN")
        first_author = authors.split(";")[0].split()[-1] if authors != "UNKNOWN" else "unknown"
        rec_stub = {"_first_author": first_author, "Year": p.get("Year", "UNKNOWN"), "Title": p.get("Title", "paper")}
        key = make_bib_key(rec_stub, used_keys)
        url = str(p.get("DOI or source URL") or "UNKNOWN")
        doi_line = f"  doi = {{{url.replace('https://doi.org/', '')}}},\n" if "doi.org/" in url else ""
        venue = str(p.get("Publication venue") or "UNKNOWN")
        venue_line = f"  journal = {{{venue}}},\n" if venue != "UNKNOWN" else "  note = {Publication venue UNKNOWN},\n"
        bib_entries.append(
            f"@article{{{key},\n"
            f"  title = {{{p.get('Title', 'UNKNOWN')}}},\n"
            f"  author = {{{authors}}},\n"
            f"  year = {{{p.get('Year', 'UNKNOWN')}}},\n"
            f"{venue_line}{doi_line}"
            f"  url = {{{url}}}\n"
            f"}}"
        )
    bib_path.write_text("\n\n".join(bib_entries) + ("\n" if bib_entries else ""), encoding="utf-8")

    chronological = sorted(papers, key=lambda r: (int(r["Year"]) if isinstance(r.get("Year"), int) else 9999, str(r.get("Title", ""))))
    source_mode = "FIXTURE MODE (NOT LIVE AI-NATIVE RESEARCH)" if is_fixture else f"Live Provider-Native ({provider})"
    raw_notes = analyst_out.get("comparative_notes")
    if isinstance(raw_notes, list):
        comp_notes = "\n".join(f"- {item}" for item in raw_notes if str(item).strip()) or "No comparable experimental benchmarks verified across papers."
    else:
        comp_notes = str(raw_notes or "").strip() or "No comparable experimental benchmarks verified across papers."

    report_path = out_dir / "sota_report.md"
    lines = [
        f"# AI-Native SOTA Discovery Report: {topic}",
        "",
        f"- **Source Mode:** {source_mode}",
        f"- **Research Objective:** {objective}",
        f"- **Papers Analyzed:** {len(papers)}",
        f"- **Elapsed Seconds:** {elapsed_seconds:.2f}",
        f"- **Token Usage:** input={usage_totals['input_tokens']}, output={usage_totals['output_tokens']}, cost_usd={usage_totals['cost_usd']}",
        "",
        "## 1. Related Research Topics (Researcher)",
        ", ".join(str(t) for t in related_topics) if related_topics else "UNKNOWN",
        "",
        "## 2. Chronological Evolution & Comparative Analysis (Analyst)",
        comp_notes,
        "",
    ]
    for p in chronological:
        lines.extend([
            f"### {p.get('Year', 'UNKNOWN')} — {p.get('Title', 'UNKNOWN')}",
            f"- **Authors:** {p.get('Authors', 'UNKNOWN')} | **Venue:** {p.get('Publication venue', 'UNKNOWN')} | **Evidence:** `{p.get('Evidence level', 'NOT_VERIFIED')}`",
            f"- **Source:** {p.get('DOI or source URL', 'UNKNOWN')}",
            f"- **Contribution:** {p.get('Research contribution', 'NOT_VERIFIED')}",
            f"- **Reported results:** {p.get('Reported results', 'NOT_VERIFIED')}",
            f"- **Limitations:** {p.get('Limitations', 'NOT_VERIFIED')}",
            "",
        ])
    report_path.write_text("\n".join(lines) + "\n", encoding="utf-8")

    review_path = out_dir / "review_report.md"
    if reviewer_out is None:
        review_lines = [
            f"# SOTA Review Report: {topic}",
            "",
            "- **Status:** `SKIPPED` (optional `--with-reviewer` not enabled; cost-control mode).",
        ]
    else:
        review_lines = [
            f"# SOTA Review Report: {topic}",
            "",
            f"- **Source Mode:** {source_mode}",
            "",
            "## Challenged / Unsupported Claims",
        ]
        for item in reviewer_out.get("challenged_claims") or ["None flagged."]:
            review_lines.append(f"- {_stringify_item(item)}")
        review_lines.extend(["", "## Missing Foundational Works"])
        for item in reviewer_out.get("missing_foundational") or ["None flagged."]:
            review_lines.append(f"- {_stringify_item(item)}")
        review_lines.extend(["", "## Actionable Corrections & Uncertainty Labels"])
        for item in reviewer_out.get("corrections") or ["None."]:
            review_lines.append(f"- {_stringify_item(item)}")
        if bib_checks is not None:
            review_lines.extend(["", "## Optional Deterministic Bibliographic Verification (OpenAlex)"])
            for chk in bib_checks:
                review_lines.append(f"- **{chk['Title']}** ({chk['url']}): `{chk['openalex_match']}`")
    review_path.write_text("\n".join(review_lines) + "\n", encoding="utf-8")
    return [report_path, matrix_path, bib_path, review_path]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="TAKO AI-Native SOTA Discovery Workflow")
    parser.add_argument("--topic", required=True)
    parser.add_argument("--objective", required=True)
    parser.add_argument("--max-papers", type=int, default=10)
    parser.add_argument("--provider", choices=["openai-responses", "claude-cli", "copilot-cli", "fixture"], default="openai-responses")
    parser.add_argument("--model", default="gpt-4.1-mini")
    parser.add_argument("--with-reviewer", action="store_true", help="Run optional Reviewer role")
    parser.add_argument("--verify-bib", action="store_true", help="Optionally verify citations via OpenAlex")
    parser.add_argument("--allow-paid-api", action="store_true", help="Explicitly authorize paid provider API/CLI calls")
    parser.add_argument("--fixture", type=pathlib.Path, default=None, help="Offline JSON fixture for --provider fixture (defaults to bundled example_fixture.json)")
    parser.add_argument("--out-dir", default="sota_ai_native_output")
    args = parser.parse_args(argv)

    t0 = time.monotonic()
    fixture_data = None
    if args.provider == "fixture":
        fixture_path = args.fixture or pathlib.Path(__file__).with_name("example_fixture.json")
        if not fixture_path.is_file():
            print("ERROR: --provider fixture requires an existing --fixture JSON file", file=sys.stderr)
            return 2
        fixture_data = json.loads(fixture_path.read_text(encoding="utf-8"))

    out_dir = pathlib.Path(args.out_dir)
    try:
        ts_r0 = time.monotonic()
        researcher_prompt = (
            f"Role: RESEARCHER. Topic: {args.topic}. Objective: {args.objective}. "
            f"Max papers: {args.max_papers}. Return ONLY a single valid JSON object (no markdown, no prose) with keys "
            f"'related_topics' (list of strings) and 'papers' (list of objects with keys: "
            f"'Title', 'Authors' (semicolon-separated string), 'Year', 'Publication venue', 'DOI or source URL', "
            f"'Relevance rationale', 'Seminal work candidate', 'Evidence level' [one of: metadata-only, abstract-based, model-recall, NOT_VERIFIED])."
        )
        researcher_out, u1 = run_role("researcher", researcher_prompt, args.provider, args.model, args.allow_paid_api, fixture_data)
        _save_stage_json(out_dir, "stage_01_researcher.json", {"elapsed_seconds": round(time.monotonic() - ts_r0, 3), "usage": u1, "payload": researcher_out})

        bib_checks = verify_bibliographic_records(researcher_out.get("papers") or []) if args.verify_bib else None
        if bib_checks is not None:
            _save_stage_json(out_dir, "stage_02_bib_verification.json", bib_checks)

        ts_a0 = time.monotonic()
        analyst_prompt = (
            f"Role: ANALYST. Objective: {args.objective}. Analyze these papers strictly from accessible evidence, "
            f"marking unsupported fields NOT_VERIFIED and preserving original returned fields. "
            f"Return ONLY a single valid JSON object (no markdown, no prose) with keys 'comparative_notes' and 'papers' "
            f"(including 'Title', 'Authors', 'Year', 'Publication venue', 'DOI or source URL', 'Relevance rationale', "
            f"'Seminal work candidate', 'Research contribution', 'Reported results', 'Limitations', 'Evidence level'). "
            f"Papers: {json.dumps(researcher_out)}. Bibliographic verification: {json.dumps(bib_checks)}."
        )
        analyst_out, u2 = run_role("analyst", analyst_prompt, args.provider, args.model, args.allow_paid_api, fixture_data)
        _save_stage_json(out_dir, "stage_03_analyst.json", {"elapsed_seconds": round(time.monotonic() - ts_a0, 3), "usage": u2, "payload": analyst_out})

        reviewer_out = None
        u3 = {"input_tokens": 0, "output_tokens": 0, "cost_usd": 0.0}
        if args.with_reviewer:
            ts_v0 = time.monotonic()
            reviewer_prompt = (
                f"Role: REVIEWER. Challenge unsupported claims, missing foundational works, and citation mismatches in: "
                f"{json.dumps(analyst_out)} (bib_checks={json.dumps(bib_checks)}). "
                f"Return ONLY a single valid JSON object (no markdown, no prose) with keys 'challenged_claims', 'missing_foundational', 'corrections' (all lists of strings)."
            )
            reviewer_out, u3 = run_role("reviewer", reviewer_prompt, args.provider, args.model, args.allow_paid_api, fixture_data)
            _save_stage_json(out_dir, "stage_04_reviewer.json", {"elapsed_seconds": round(time.monotonic() - ts_v0, 3), "usage": u3, "payload": reviewer_out})
    except ProviderBlockedError as exc:
        print(f"PROVIDER_BLOCKED: {exc}", file=sys.stderr)
        return 1

    def sum_field(key: str) -> Any:
        vals = [u.get(key) for u in (u1, u2, u3)]
        if all(isinstance(v, (int, float)) for v in vals):
            return sum(vals)
        if key == "cost_usd":
            prem = []
            for v in vals:
                if isinstance(v, str) and v.startswith("premium_requests:"):
                    try:
                        prem.append(int(v.split(":", 1)[1]))
                    except ValueError:
                        pass
                elif v == 0 or v == 0.0:
                    prem.append(0)
            if prem and len(prem) == len(vals):
                return f"premium_requests:{sum(prem)}"
        return "UNKNOWN"

    usage_totals = {
        "input_tokens": sum_field("input_tokens"),
        "output_tokens": sum_field("output_tokens"),
        "cost_usd": sum_field("cost_usd"),
    }
    elapsed = time.monotonic() - t0
    outputs = write_ai_native_outputs(
        out_dir=pathlib.Path(args.out_dir),
        topic=args.topic,
        objective=args.objective,
        provider=args.provider,
        researcher_out=researcher_out,
        analyst_out=analyst_out,
        reviewer_out=reviewer_out,
        bib_checks=bib_checks,
        usage_totals=usage_totals,
        elapsed_seconds=elapsed,
    )
    print(f"AI_NATIVE_COMPLETE provider={args.provider} reviewer={args.with_reviewer} outputs={[str(p) for p in outputs]}")
    return 0
