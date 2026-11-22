"""AI-native SOTA discovery workflow with sequential Researcher, Analyst, and optional Reviewer roles."""

from __future__ import annotations

import argparse
import csv
import json
import os
import pathlib
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
    if text.startswith("```"):
        lines = text.splitlines()
        if lines and lines[0].startswith("```"):
            lines = lines[1:]
        if lines and lines[-1].strip() == "```":
            lines = lines[:-1]
        text = "\n".join(lines).strip()
    try:
        parsed = json.loads(text)
    except json.JSONDecodeError as exc:
        raise ProviderBlockedError(f"{provider} returned invalid JSON: {exc}") from exc
    if not isinstance(parsed, dict):
        raise ProviderBlockedError(f"{provider} returned non-object JSON payload.")
    return parsed


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


def call_copilot_cli(prompt: str, allow_paid_api: bool = False, timeout: float = 90.0) -> tuple[str, dict[str, Any]]:
    if not allow_paid_api:
        raise ProviderBlockedError(
            "Copilot CLI inference requires explicit --allow-paid-api authorization."
        )
    exe = shutil.which("copilot")
    if not exe:
        raise ProviderBlockedError("copilot CLI is not installed on PATH.")
    try:
        proc = subprocess.run(
            [exe, "-p", prompt, "-s"],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
        )
    except subprocess.TimeoutExpired as exc:
        raise ProviderBlockedError(f"copilot CLI timed out after {timeout}s.") from exc
    if proc.returncode != 0:
        raise ProviderBlockedError(f"copilot CLI failed (exit {proc.returncode}): {proc.stderr.strip()}")
    return proc.stdout, {"input_tokens": "UNKNOWN", "output_tokens": "UNKNOWN", "cost_usd": "UNKNOWN"}


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
        return parse_provider_json(raw, provider), usage
    if provider == "claude-cli":
        raw, usage = call_claude_cli(prompt, allow_paid_api=allow_paid_api)
        return parse_provider_json(raw, provider), usage
    if provider == "copilot-cli":
        raw, usage = call_copilot_cli(prompt, allow_paid_api=allow_paid_api)
        return parse_provider_json(raw, provider), usage
    raise ProviderBlockedError(f"Unsupported provider: {provider}")


def verify_bibliographic_records(papers: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Optional deterministic OpenAlex verification for Reviewer role."""
    checks: list[dict[str, Any]] = []
    for p in papers:
        title = p.get("Title", "")
        url = p.get("DOI or source URL", "")
        try:
            hits = fetch_openalex(title, per_page=3, timeout=10.0)
            matched = any(
                (h.get("doi") or "") == url or title.lower() in (h.get("display_name") or "").lower()
                for h in hits
            )
            checks.append({
                "Title": title,
                "url": url,
                "openalex_match": "VERIFIED" if matched else "NOT_VERIFIED",
            })
        except Exception as exc:
            checks.append({"Title": title, "url": url, "openalex_match": f"UNAVAILABLE ({exc})"})
    return checks


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
        ", ".join(related_topics) if related_topics else "UNKNOWN",
        "",
        "## 2. Chronological Evolution & Comparative Analysis (Analyst)",
        analyst_out.get("comparative_notes") or "No comparable experimental benchmarks verified across papers.",
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
            review_lines.append(f"- {item}")
        review_lines.extend(["", "## Missing Foundational Works"])
        for item in reviewer_out.get("missing_foundational") or ["None flagged."]:
            review_lines.append(f"- {item}")
        review_lines.extend(["", "## Actionable Corrections & Uncertainty Labels"])
        for item in reviewer_out.get("corrections") or ["None."]:
            review_lines.append(f"- {item}")
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

    try:
        researcher_prompt = (
            f"Role: RESEARCHER. Topic: {args.topic}. Objective: {args.objective}. "
            f"Max papers: {args.max_papers}. Return strict JSON with keys 'related_topics' and 'papers' "
            f"(Title, Authors, Year, Publication venue, DOI or source URL, Seminal work candidate, Evidence level)."
        )
        researcher_out, u1 = run_role("researcher", researcher_prompt, args.provider, args.model, args.allow_paid_api, fixture_data)

        analyst_prompt = (
            f"Role: ANALYST. Objective: {args.objective}. Analyze these papers strictly from accessible evidence, "
            f"marking unsupported fields NOT_VERIFIED: {json.dumps(researcher_out)}. Return JSON with 'comparative_notes' and 'papers'."
        )
        analyst_out, u2 = run_role("analyst", analyst_prompt, args.provider, args.model, args.allow_paid_api, fixture_data)

        reviewer_out = None
        u3 = {"input_tokens": 0, "output_tokens": 0, "cost_usd": 0.0}
        bib_checks = None
        if args.with_reviewer:
            reviewer_prompt = (
                f"Role: REVIEWER. Challenge unsupported claims, missing foundational works, and citation risks in: "
                f"{json.dumps(analyst_out)}. Return JSON with 'challenged_claims', 'missing_foundational', 'corrections'."
            )
            reviewer_out, u3 = run_role("reviewer", reviewer_prompt, args.provider, args.model, args.allow_paid_api, fixture_data)
            if args.verify_bib:
                bib_checks = verify_bibliographic_records(analyst_out.get("papers") or [])
    except ProviderBlockedError as exc:
        print(f"PROVIDER_BLOCKED: {exc}", file=sys.stderr)
        return 1

    def sum_field(key: str) -> Any:
        vals = [u.get(key) for u in (u1, u2, u3)]
        if all(isinstance(v, (int, float)) for v in vals):
            return sum(vals)
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
