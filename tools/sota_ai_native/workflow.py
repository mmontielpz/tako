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
import threading
import sys
import time
import urllib.error
import urllib.request
from typing import Any

from tools.ai_cli_providers import (
    authorize_provider_execution,
    build_cli_invocation,
    sanitized_cli_env,
)
from tools.sota_discovery.sota import HttpBudgetExhaustedError, HttpRequestTracker, fetch_openalex
from tools.sota_integrity import (
    aggregate_usage_records,
    classify_evidence_provenance,
    deduplicate_cue_sentences,
    deduplicate_text_items,
    make_usage_record,
    partition_bibliography,
    render_bibtex_entries,
    verify_paper_identity,
)

MATRIX_FIELDS = [
    "Title", "Authors", "Year", "Publication venue", "DOI or source URL",
    "Relevance rationale", "Seminal work candidate", "Research contribution",
    "Reported results", "Limitations", "Evidence level",
    "Bibliographic status", "Evidence availability", "Claim support status", "Human review status",
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
    return "\n".join(text_chunks), make_usage_record(
        cost_usd="UNKNOWN",
        premium_requests="UNKNOWN",
        request_count=1,
        input_tokens=usage.get("input_tokens", "UNKNOWN"),
        output_tokens=usage.get("output_tokens", "UNKNOWN"),
        measurement_availability="provider_reported" if usage else "UNKNOWN",
        provenance="openai-responses",
    )


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
    if provider == "copilot-cli":
        # Copilot CLI recall outputs lack retrieved abstract/full-text provenance in TAKO.
        if low in ("", "model-recall", "model_recall"):
            return "MODEL_RECALL"
        return f"MODEL_RECALL (clamped_from={label!r})"
    if low in VALID_EVIDENCE_LEVELS:
        if low == "not_verified":
            return "NOT_VERIFIED"
        if low == "unknown":
            return "UNKNOWN"
        if low in ("model-recall", "model_recall"):
            return "MODEL_RECALL"
        if low in ("abstract-based", "full-text") and provider != "fixture":
            # Without verified retrieved text correspondence, do not promote recall to abstract/full-text.
            return f"MODEL_RECALL (unbacked_claim={label!r})"
        return low
    return f"UNKNOWN (unsupported_label={label!r})" if label else "UNKNOWN"


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
    input_tokens: int | str = "UNKNOWN"
    output_tokens: int | str = "UNKNOWN"
    premium_requests: int | str = "UNKNOWN"
    cost_usd: float | str = "UNKNOWN"
    provider_specific: dict[str, Any] = {}
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
            input_tokens = u.get("input_tokens", u.get("inputTokens", input_tokens))
            output_tokens = u.get("output_tokens", u.get("outputTokens", output_tokens))
            if "premiumRequests" in u:
                premium_requests = u["premiumRequests"]
        if "total_cost_usd" in obj and isinstance(obj["total_cost_usd"], (int, float)):
            cost_usd = obj["total_cost_usd"]
        if isinstance(data, dict) and "ai_credits" in data:
            provider_specific["ai_credits"] = data["ai_credits"]
        if isinstance(data, dict) and "totalPremiumRequests" in data and premium_requests == "UNKNOWN":
            premium_requests = data["totalPremiumRequests"]

    usage = make_usage_record(
        cost_usd=cost_usd,
        premium_requests=premium_requests,
        request_count=1,
        input_tokens=input_tokens,
        output_tokens=output_tokens,
        provider_specific_usage=provider_specific,
        measurement_availability="provider_reported" if (premium_requests != "UNKNOWN" or input_tokens != "UNKNOWN") else "UNKNOWN",
        provenance="copilot-cli-jsonl",
    )

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


MAX_PROVIDER_OUTPUT_BYTES = 2 * 1024 * 1024


def _sanitized_provider_env(allow_key_env: str | None = None) -> dict[str, str]:
    """Build a minimal environment for CLI subprocesses without leaking unrelated credentials."""
    keep = ("PATH", "HOME", "USER", "LOGNAME", "LANG", "LC_ALL", "TERM", "TMPDIR", "SYSTEMROOT", "COMSPEC")
    env = {k: os.environ[k] for k in keep if k in os.environ}
    if allow_key_env and allow_key_env in os.environ:
        env[allow_key_env] = os.environ[allow_key_env]
    return env


def _run_bounded_subprocess(
    cmd: list[str],
    *,
    env: dict[str, str],
    timeout: float,
    max_bytes: int = MAX_PROVIDER_OUTPUT_BYTES,
    provider_label: str = "provider CLI",
) -> tuple[int, str, str]:
    """Run subprocess with streaming byte-cap enforcement across stdout and stderr."""
    # Preserve compatibility with unit tests that mock subprocess.run directly
    if getattr(subprocess.run, "__module__", "subprocess") != "subprocess":
        try:
            proc = subprocess.run(
                cmd,
                stdin=subprocess.DEVNULL,
                env=env,
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=timeout,
            )
        except subprocess.TimeoutExpired as exc:
            raise ProviderBlockedError(f"{provider_label} timed out after {timeout}s.") from exc
        if len((proc.stdout or "").encode("utf-8", errors="replace")) + len((proc.stderr or "").encode("utf-8", errors="replace")) > max_bytes:
            raise ProviderBlockedError(f"{provider_label} output exceeded MAX_PROVIDER_OUTPUT_BYTES ({max_bytes} bytes).")
        return proc.returncode, proc.stdout or "", proc.stderr or ""

    proc = subprocess.Popen(
        cmd,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
    )
    out_chunks: list[bytes] = []
    err_chunks: list[bytes] = []
    total_bytes = 0
    exceeded = threading.Event()
    lock = threading.Lock()

    def _drain(stream: Any, target: list[bytes]) -> None:
        nonlocal total_bytes
        try:
            while True:
                chunk = stream.read(65536)
                if not chunk:
                    break
                with lock:
                    total_bytes += len(chunk)
                    if total_bytes > max_bytes:
                        exceeded.set()
                        try:
                            proc.kill()
                        except OSError:
                            pass
                        break
                    target.append(chunk)
        finally:
            try:
                stream.close()
            except OSError:
                pass

    t_out = threading.Thread(target=_drain, args=(proc.stdout, out_chunks), daemon=True)
    t_err = threading.Thread(target=_drain, args=(proc.stderr, err_chunks), daemon=True)
    t_out.start()
    t_err.start()
    try:
        rc = proc.wait(timeout=timeout)
    except subprocess.TimeoutExpired as exc:
        try:
            proc.kill()
        except OSError:
            pass
        proc.wait()
        t_out.join(timeout=1.0)
        t_err.join(timeout=1.0)
        raise ProviderBlockedError(f"{provider_label} timed out after {timeout}s.") from exc
    t_out.join(timeout=2.0)
    t_err.join(timeout=2.0)
    if exceeded.is_set():
        raise ProviderBlockedError(f"{provider_label} output exceeded MAX_PROVIDER_OUTPUT_BYTES ({max_bytes} bytes) during streaming.")
    stdout = b"".join(out_chunks).decode("utf-8", errors="replace")
    stderr = b"".join(err_chunks).decode("utf-8", errors="replace")
    return rc, stdout, stderr


def _run_subscription_cli(
    provider: str,
    exe_name: str,
    prompt: str,
    *,
    allow_cli_session: bool = False,
    allow_paid_api: bool = False,
    timeout: float = 90.0,
    strip_var: str | None = None,
) -> tuple[str, dict[str, Any]]:
    ok, reason = authorize_provider_execution(
        provider, allow_cli_session=allow_cli_session, allow_paid_api=allow_paid_api
    )
    if not ok:
        raise ProviderBlockedError(reason)
    exe = shutil.which(exe_name)
    if not exe:
        raise ProviderBlockedError(f"{exe_name} CLI is not installed on PATH.")
    cmd = build_cli_invocation(provider, exe, prompt)
    rc, stdout, stderr = _run_bounded_subprocess(
        cmd,
        env=sanitized_cli_env(strip_var),
        timeout=timeout,
        provider_label=f"{exe_name} CLI",
    )
    if rc != 0:
        err_preview = stderr.strip()[:200] or stdout.strip()[:200]
        raise ProviderBlockedError(f"{exe_name} CLI failed (exit {rc}): {err_preview}")
    if provider == "copilot-cli":
        if not stdout.strip():
            raise ProviderBlockedError(f"copilot CLI returned empty stdout (stderr={stderr.strip()[:200]!r}).")
        return extract_copilot_jsonl_response(stdout)
    try:
        data = json.loads(stdout)
        text = data.get("result") or stdout
        u_raw = data.get("usage") or {}
        cost = data.get("total_cost_usd", "UNKNOWN")
        usage = make_usage_record(
            cost_usd=cost if isinstance(cost, (int, float)) else "UNKNOWN",
            premium_requests="UNKNOWN",
            request_count=1,
            input_tokens=u_raw.get("input_tokens", "UNKNOWN"),
            output_tokens=u_raw.get("output_tokens", "UNKNOWN"),
            provider_specific_usage={"http_attempts": "NOT_OBSERVABLE"},
            measurement_availability="provider_reported" if (u_raw or isinstance(cost, (int, float))) else "UNKNOWN",
            provenance=f"{provider}-json",
        )
    except json.JSONDecodeError:
        text = stdout
        usage = make_usage_record(request_count=1, provider_specific_usage={"http_attempts": "NOT_OBSERVABLE"}, provenance=f"{provider}-raw")
    return text, usage


def call_agy_cli(prompt: str, allow_cli_session: bool = False, allow_paid_api: bool = False, timeout: float = 90.0) -> tuple[str, dict[str, Any]]:
    return _run_subscription_cli("agy-cli", "agy", prompt, allow_cli_session=allow_cli_session, allow_paid_api=allow_paid_api, timeout=timeout)


def call_codex_cli(prompt: str, allow_cli_session: bool = False, allow_paid_api: bool = False, timeout: float = 90.0) -> tuple[str, dict[str, Any]]:
    return _run_subscription_cli("codex-cli", "codex", prompt, allow_cli_session=allow_cli_session, allow_paid_api=allow_paid_api, timeout=timeout, strip_var="OPENAI_API_KEY")


def call_claude_cli(prompt: str, allow_paid_api: bool = False, timeout: float = 90.0, allow_cli_session: bool = False) -> tuple[str, dict[str, Any]]:
    return _run_subscription_cli("claude-cli", "claude", prompt, allow_cli_session=allow_cli_session, allow_paid_api=allow_paid_api, timeout=timeout, strip_var="ANTHROPIC_API_KEY")


def call_copilot_cli(prompt: str, allow_paid_api: bool = False, timeout: float = 120.0, allow_cli_session: bool = False) -> tuple[str, dict[str, Any]]:
    return _run_subscription_cli("copilot-cli", "copilot", prompt, allow_cli_session=allow_cli_session, allow_paid_api=allow_paid_api, timeout=timeout, strip_var="GH_TOKEN")


def run_role(
    role_name: str,
    prompt: str,
    provider: str,
    model: str,
    allow_paid_api: bool,
    fixture_data: dict[str, Any] | None,
    allow_cli_session: bool = False,
) -> tuple[dict[str, Any], dict[str, Any]]:
    if provider == "fixture":
        assert fixture_data is not None
        role_payload = fixture_data.get(role_name.lower()) or {}
        return role_payload, make_usage_record(
            cost_usd=0.0,
            premium_requests=0,
            request_count=0,
            input_tokens=0,
            output_tokens=0,
            measurement_availability="fixture_zero",
            provenance="offline_fixture",
        )
    if provider == "openai-responses":
        raw, usage = call_openai_responses_web_search(prompt, model=model, allow_paid_api=allow_paid_api)
        return validate_role_payload(role_name, parse_provider_json(raw, provider), provider), usage
    if provider == "agy-cli":
        raw, usage = call_agy_cli(prompt, allow_cli_session=allow_cli_session, allow_paid_api=allow_paid_api)
        return validate_role_payload(role_name, parse_provider_json(raw, provider), provider), usage
    if provider == "codex-cli":
        raw, usage = call_codex_cli(prompt, allow_cli_session=allow_cli_session, allow_paid_api=allow_paid_api)
        return validate_role_payload(role_name, parse_provider_json(raw, provider), provider), usage
    if provider == "claude-cli":
        raw, usage = call_claude_cli(prompt, allow_paid_api=allow_paid_api, allow_cli_session=allow_cli_session)
        return validate_role_payload(role_name, parse_provider_json(raw, provider), provider), usage
    if provider == "copilot-cli":
        raw, usage = call_copilot_cli(prompt, allow_paid_api=allow_paid_api, allow_cli_session=allow_cli_session)
        return validate_role_payload(role_name, parse_provider_json(raw, provider), provider), usage
    raise ProviderBlockedError(f"Unsupported provider: {provider}")


def _fetch_openalex_by_doi(
    normalized_doi: str,
    *,
    tracker: HttpRequestTracker | None = None,
) -> dict[str, Any] | None:
    doi_url = "https://api.openalex.org/works/" + normalized_doi.strip()
    if tracker is not None:
        try:
            payload = tracker.get_json(doi_url, timeout=10.0, category="bib_verification")
            return payload if isinstance(payload, dict) else None
        except HttpBudgetExhaustedError:
            raise
        except Exception:
            return None
    req = urllib.request.Request(doi_url, headers={"User-Agent": "TAKO-SOTA-Mini-MVP/0.1"})
    try:
        with urllib.request.urlopen(req, timeout=10.0) as resp:
            if resp.status == 200:
                return json.loads(resp.read().decode("utf-8"))
    except Exception:
        return None
    return None


def verify_bibliographic_records(
    papers: list[dict[str, Any]],
    *,
    tracker: HttpRequestTracker | None = None,
) -> list[dict[str, Any]]:
    """Optional deterministic OpenAlex verification preserving original returned metadata."""
    seen_identities: set[str] = set()
    checks: list[dict[str, Any]] = []
    budget_exhausted = False
    for p in papers:
        if budget_exhausted or (tracker is not None and tracker.budget_exhausted):
            checks.append({
                "Title": str(p.get("Title") or "").strip(),
                "url": str(p.get("DOI or source URL") or "").strip(),
                "openalex_match": "UNVERIFIABLE",
                "identity_verification_outcome": "UNVERIFIABLE",
                "discrepancy_evidence": ["http_budget_exhausted"],
                "corrected_identity": None,
            })
            continue
        try:
            checks.append(
                verify_paper_identity(
                    p,
                    fetch_by_doi_fn=lambda d: _fetch_openalex_by_doi(d, tracker=tracker),
                    search_fn=lambda q: fetch_openalex(q, per_page=3, timeout=10.0, tracker=tracker, category="bib_verification"),
                    seen_identities=seen_identities,
                )
            )
        except HttpBudgetExhaustedError:
            budget_exhausted = True
            checks.append({
                "Title": str(p.get("Title") or "").strip(),
                "url": str(p.get("DOI or source URL") or "").strip(),
                "openalex_match": "UNVERIFIABLE",
                "identity_verification_outcome": "UNVERIFIABLE",
                "discrepancy_evidence": ["http_budget_exhausted"],
                "corrected_identity": None,
            })
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
    raw_papers = list(analyst_out.get("papers") or researcher_out.get("papers") or [])
    related_topics = deduplicate_text_items(list(researcher_out.get("related_topics") or []))

    papers: list[dict[str, Any]] = []
    for idx, p in enumerate(raw_papers):
        item = dict(p)
        contrib, res, lim = deduplicate_cue_sentences(
            str(item.get("Research contribution") or "NOT_VERIFIED"),
            str(item.get("Reported results") or "NOT_VERIFIED"),
            str(item.get("Limitations") or "NOT_VERIFIED"),
        )
        item["Research contribution"] = contrib
        item["Reported results"] = res
        item["Limitations"] = lim
        chk = bib_checks[idx] if (bib_checks is not None and idx < len(bib_checks)) else None
        prov = classify_evidence_provenance(
            item,
            provider=provider,
            bib_check=chk,
            has_retrieved_abstract=False,
            has_full_text=False,
        )
        item["Evidence level"] = prov["EVIDENCE_AVAILABILITY"]
        item["Bibliographic status"] = prov["BIBLIOGRAPHIC_STATUS"]
        item["Evidence availability"] = prov["EVIDENCE_AVAILABILITY"]
        item["Claim support status"] = prov["CLAIM_SUPPORT_STATUS"]
        item["Human review status"] = prov["HUMAN_REVIEW_STATUS"]
        papers.append(item)

    matrix_path = out_dir / "literature_matrix.csv"
    with matrix_path.open("w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=MATRIX_FIELDS)
        writer.writeheader()
        for p in papers:
            row = {k: p.get(k, "NOT_VERIFIED") or "NOT_VERIFIED" for k in MATRIX_FIELDS}
            writer.writerow(row)

    bib_path = out_dir / "references.bib"
    candidates_bib_path = out_dir / "references_candidates.bib"
    used_keys: set[str] = set()
    accepted, candidates, excluded, accounting = partition_bibliography(papers, bib_checks)
    bib_path.write_text(render_bibtex_entries(accepted, is_candidate=False, used_keys=used_keys), encoding="utf-8")
    candidates_bib_path.write_text(render_bibtex_entries(candidates, is_candidate=True, used_keys=used_keys), encoding="utf-8")

    chronological = sorted(papers, key=lambda r: (int(r["Year"]) if isinstance(r.get("Year"), int) else 9999, str(r.get("Title", ""))))
    source_mode = "FIXTURE MODE (NOT LIVE AI-NATIVE RESEARCH)" if is_fixture else f"Live Provider-Native ({provider})"
    raw_notes = analyst_out.get("comparative_notes")
    if isinstance(raw_notes, list):
        dedup_notes = deduplicate_text_items([str(item).strip() for item in raw_notes if str(item).strip()])
        comp_notes = "\n".join(f"- {item}" for item in dedup_notes) or "No comparable experimental benchmarks verified across papers."
    else:
        comp_notes = str(raw_notes or "").strip() or "No comparable experimental benchmarks verified across papers."

    report_path = out_dir / "sota_report.md"
    lines = [
        f"# AI-Native SOTA Discovery Report: {topic}",
        "",
        f"- **Source Mode:** {source_mode}",
        f"- **Research Objective:** {objective}",
        f"- **Papers Analyzed:** {len(papers)} (bibliography: accepted={accounting['accepted']}, candidates={accounting['candidates']}, excluded={accounting['excluded']})",
        f"- **Elapsed Seconds:** {elapsed_seconds:.2f}",
        f"- **Token Usage:** input={usage_totals.get('input_tokens', 'UNKNOWN')}, output={usage_totals.get('output_tokens', 'UNKNOWN')}, total={usage_totals.get('total_tokens', 'UNKNOWN')}, requests={usage_totals.get('request_count', 'UNKNOWN')}, premium_requests={usage_totals.get('premium_requests', 'UNKNOWN')}, cost_usd={usage_totals.get('cost_usd', 'UNKNOWN')} (availability={usage_totals.get('measurement_availability', 'UNKNOWN')})",
        "",
    ]
    if accounting["accepted"] == 0:
        lines.extend([
            "> **Empty Accepted Bibliography Notice:** `references.bib` is EMPTY (0 bibliographically verified entries). Unverified or mismatched records are routed to `references_candidates.bib`. Process exit `0` indicates pipeline completion only, not scientific quality `PASS`.",
            "",
        ])
    lines.extend([
        "## 1. Related Research Topics (Researcher)",
        ", ".join(str(t) for t in related_topics) if related_topics else "UNKNOWN",
        "",
        "## 2. Chronological Evolution & Comparative Analysis (Analyst)",
        comp_notes,
        "",
    ])
    for p in chronological:
        lines.extend([
            f"### {p.get('Year', 'UNKNOWN')} — {p.get('Title', 'UNKNOWN')}",
            f"- **Authors:** {p.get('Authors', 'UNKNOWN')} | **Venue:** {p.get('Publication venue', 'UNKNOWN')}",
            f"- **Status:** Bibliographic=`{p.get('Bibliographic status', 'NOT_CHECKED')}` | Evidence=`{p.get('Evidence availability', 'UNKNOWN')}` | Claim support=`{p.get('Claim support status', 'NOT_CHECKED')}` | Human review=`{p.get('Human review status', 'PENDING')}`",
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
        for item in deduplicate_text_items(reviewer_out.get("challenged_claims") or ["None flagged."]):
            review_lines.append(f"- {_stringify_item(item)}")
        review_lines.extend(["", "## Missing Foundational Works"])
        for item in deduplicate_text_items(reviewer_out.get("missing_foundational") or ["None flagged."]):
            review_lines.append(f"- {_stringify_item(item)}")
        review_lines.extend(["", "## Actionable Corrections & Uncertainty Labels"])
        for item in deduplicate_text_items(reviewer_out.get("corrections") or ["None."]):
            review_lines.append(f"- {_stringify_item(item)}")
        if bib_checks is not None:
            review_lines.extend(["", "## Optional Deterministic Bibliographic Verification (OpenAlex)"])
            for chk in bib_checks:
                review_lines.append(f"- **{chk['Title']}** ({chk['url']}): `{chk['openalex_match']}`")
    review_path.write_text("\n".join(review_lines) + "\n", encoding="utf-8")
    return [report_path, matrix_path, bib_path, candidates_bib_path, review_path]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="tako sota-ai",
        description="TAKO Research Mate / AI-Native SOTA Workflow (Researcher -> Analyst -> optional Reviewer)",
        epilog=(
            "Examples:\n"
            "  tako sota-ai --topic 'Edge AI' --objective 'Survey latency' --provider fixture\n"
            "  tako sota-ai --topic 'Edge AI' --objective 'Survey latency' --provider claude-cli --allow-cli-session\n"
            "  tako sota-ai --topic 'Edge AI' --objective 'Survey latency' --provider openai-responses --allow-paid-api"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--topic", default=None)
    parser.add_argument("--objective", default=None)
    parser.add_argument("--workspace", type=pathlib.Path, default=None, help="Optional TAKO workspace directory (tako_workspace.json)")
    parser.add_argument("--max-papers", type=int, default=10)
    parser.add_argument(
        "--provider",
        choices=["agy-cli", "codex-cli", "claude-cli", "copilot-cli", "openai-responses", "fixture"],
        default="fixture",
        help="AI provider (default: fixture; use --allow-cli-session for local AI CLIs or --allow-paid-api for direct API)",
    )
    parser.add_argument("--model", default="gpt-4.1-mini")
    parser.add_argument("--with-reviewer", action="store_true", help="Run optional Reviewer role")
    parser.add_argument("--verify-bib", action="store_true", help="Optionally verify citations via OpenAlex")
    parser.add_argument(
        "--max-http-requests",
        type=int,
        default=None,
        help="Maximum observable HTTP attempts for --verify-bib (OpenAlex); does not bound black-box provider CLI network activity",
    )
    parser.add_argument(
        "--allow-cli-session",
        action="store_true",
        help="Authorize local authenticated AI CLI execution (may consume subscription quotas, credits, or premium requests)",
    )
    parser.add_argument("--allow-paid-api", action="store_true", help="Explicitly authorize direct metered API calls (e.g. openai-responses)")
    parser.add_argument("--fixture", type=pathlib.Path, default=None, help="Offline JSON fixture for --provider fixture (defaults to bundled example_fixture.json)")
    parser.add_argument("--out-dir", default=None)
    parser.add_argument("--force", action="store_true", help="Allow overwriting non-empty output directory")
    parser.add_argument("--json", action="store_true", help="Emit machine-readable JSON output only")
    parser.add_argument("--events-file", type=pathlib.Path, default=None, help="Optional JSONL path for workflow events")
    args = parser.parse_args(argv)

    from tools.research_workspace import ensure_safe_out_dir, load_workspace, register_workflow_stage
    from tools.workflow_events import WorkflowEventEmitter, make_run_id, sanitize_error

    ws_manifest = None
    if args.workspace is not None:
        try:
            ws_manifest = load_workspace(args.workspace)
        except Exception as exc:
            print(f"WORKSPACE_ERROR: {exc}", file=sys.stderr)
            return 2
        if args.topic is None:
            args.topic = ws_manifest["topic"]
        if args.objective is None:
            args.objective = ws_manifest["objective"]

    if not args.topic or not args.objective:
        print("ERROR: --topic and --objective are required (or pass --workspace with tako_workspace.json).", file=sys.stderr)
        return 2

    if args.out_dir is None:
        args.out_dir = str((args.workspace / "sota_ai_native_output") if args.workspace is not None else pathlib.Path("sota_ai_native_output"))

    if args.max_http_requests is not None and args.max_http_requests < 1:
        print("ERROR: --max-http-requests must be >= 1", file=sys.stderr)
        return 2

    from tools.researcher_setup import SetupConfigError, resolve_effective_discovery_settings

    try:
        effective_cfg = resolve_effective_discovery_settings(is_fixture=(args.provider == "fixture"))
    except SetupConfigError as exc:
        print(f"CONFIG_ERROR: {exc}", file=sys.stderr)
        return 2
    if effective_cfg["offline_only"] and args.provider != "fixture":
        print(
            "OFFLINE_MODE_BLOCKED: Active user profile 'offline' prohibits live provider network calls; use --provider fixture or reconfigure via 'tako setup'.",
            file=sys.stderr,
        )
        return 2

    t0 = time.monotonic()
    fixture_data = None
    if args.provider == "fixture":
        fixture_path = args.fixture or pathlib.Path(__file__).with_name("example_fixture.json")
        if not fixture_path.is_file():
            print("ERROR: --provider fixture requires an existing --fixture JSON file", file=sys.stderr)
            return 2
        fixture_data = json.loads(fixture_path.read_text(encoding="utf-8"))

    out_dir = pathlib.Path(args.out_dir)
    if args.workspace is not None or args.force:
        try:
            ensure_safe_out_dir(out_dir, force=args.force)
        except Exception as exc:
            print(f"OUTPUT_ERROR: {exc}", file=sys.stderr)
            return 2

    run_id = make_run_id("sota-ai")
    events_path = args.events_file
    if events_path is None and args.workspace is not None:
        events_path = args.workspace / "runs" / f"{run_id}_events.jsonl"
    emitter = WorkflowEventEmitter(
        workflow="sota-ai",
        workspace_id=(ws_manifest["research_id"] if ws_manifest else "standalone"),
        run_id=run_id,
        events_file=events_path,
        json_mode=args.json,
    )
    emitter.emit("workflow_started", stage="discovery", status="RUNNING", message=f"Topic: {args.topic} | Provider: {args.provider}")
    if args.workspace is not None:
        register_workflow_stage(args.workspace, "discovery", status="RUNNING", run_id=run_id, out_dir=out_dir)

    try:
        emitter.emit("stage_started", stage="researcher", status="RUNNING", message="Running Researcher stage")
        ts_r0 = time.monotonic()
        researcher_prompt = (
            f"Role: RESEARCHER. Topic: {args.topic}. Objective: {args.objective}. "
            f"Max papers: {args.max_papers}. Return ONLY a single valid JSON object (no markdown, no prose) with keys "
            f"'related_topics' (list of strings) and 'papers' (list of objects with keys: "
            f"'Title', 'Authors' (semicolon-separated string), 'Year', 'Publication venue', 'DOI or source URL', "
            f"'Relevance rationale', 'Seminal work candidate', 'Evidence level' [one of: metadata-only, abstract-based, model-recall, NOT_VERIFIED])."
        )
        researcher_out, u1 = run_role("researcher", researcher_prompt, args.provider, args.model, args.allow_paid_api, fixture_data, allow_cli_session=args.allow_cli_session)
        _save_stage_json(out_dir, "stage_01_researcher.json", {"elapsed_seconds": round(time.monotonic() - ts_r0, 3), "usage": u1, "payload": researcher_out})

        bib_tracker = HttpRequestTracker(max_http_requests=args.max_http_requests) if args.verify_bib else None
        bib_checks = verify_bibliographic_records(researcher_out.get("papers") or [], tracker=bib_tracker) if args.verify_bib else None
        if bib_checks is not None:
            _save_stage_json(out_dir, "stage_02_bib_verification.json", bib_checks)
            if bib_tracker is not None:
                _save_stage_json(out_dir, "stage_02_bib_verification_telemetry.json", bib_tracker.summary())

        ts_a0 = time.monotonic()
        analyst_prompt = (
            f"Role: ANALYST. Objective: {args.objective}. Analyze these papers strictly from accessible evidence, "
            f"marking unsupported fields NOT_VERIFIED and preserving original returned fields. "
            f"Return ONLY a single valid JSON object (no markdown, no prose) with keys 'comparative_notes' and 'papers' "
            f"(including 'Title', 'Authors', 'Year', 'Publication venue', 'DOI or source URL', 'Relevance rationale', "
            f"'Seminal work candidate', 'Research contribution', 'Reported results', 'Limitations', 'Evidence level'). "
            f"Papers: {json.dumps(researcher_out)}. Bibliographic verification: {json.dumps(bib_checks)}."
        )
        analyst_out, u2 = run_role("analyst", analyst_prompt, args.provider, args.model, args.allow_paid_api, fixture_data, allow_cli_session=args.allow_cli_session)
        _save_stage_json(out_dir, "stage_03_analyst.json", {"elapsed_seconds": round(time.monotonic() - ts_a0, 3), "usage": u2, "payload": analyst_out})

        reviewer_out = None
        u3 = None
        if args.with_reviewer:
            ts_v0 = time.monotonic()
            reviewer_prompt = (
                f"Role: REVIEWER. Challenge unsupported claims, missing foundational works, and citation mismatches in: "
                f"{json.dumps(analyst_out)} (bib_checks={json.dumps(bib_checks)}). "
                f"Return ONLY a single valid JSON object (no markdown, no prose) with keys 'challenged_claims', 'missing_foundational', 'corrections' (all lists of strings)."
            )
            reviewer_out, u3 = run_role("reviewer", reviewer_prompt, args.provider, args.model, args.allow_paid_api, fixture_data, allow_cli_session=args.allow_cli_session)
            _save_stage_json(out_dir, "stage_04_reviewer.json", {"elapsed_seconds": round(time.monotonic() - ts_v0, 3), "usage": u3, "payload": reviewer_out})
    except ProviderBlockedError as exc:
        err_s = sanitize_error(exc)
        emitter.emit("stage_failed", stage="discovery", status="FAILED", error=err_s, message=err_s)
        if args.workspace is not None:
            register_workflow_stage(args.workspace, "discovery", status="FAILED", run_id=run_id, out_dir=out_dir, error=err_s)
        print(f"PROVIDER_BLOCKED: {exc}", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        emitter.emit("workflow_interrupted", stage="discovery", status="INTERRUPTED", message="Interrupted by user")
        if args.workspace is not None:
            register_workflow_stage(args.workspace, "discovery", status="INTERRUPTED", run_id=run_id, out_dir=out_dir, error="Interrupted by user")
        print("AI_NATIVE_INTERRUPTED: Run interrupted by user.", file=sys.stderr)
        return 130

    elapsed = time.monotonic() - t0
    usage_records = [u1, u2] + ([u3] if u3 is not None else [])
    usage_totals = aggregate_usage_records(usage_records, wall_time_seconds=elapsed)
    outputs = write_ai_native_outputs(
        out_dir=out_dir,
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
    out_strs = [str(p) for p in outputs]
    emitter.emit("workflow_completed", stage="discovery", status="COMPLETED", artifacts=out_strs, message=f"Completed: provider={args.provider}, out_dir={out_dir}")
    if args.workspace is not None:
        register_workflow_stage(args.workspace, "discovery", status="COMPLETED", run_id=run_id, out_dir=out_dir, outputs=outputs, summary={"provider": args.provider, "reviewer": args.with_reviewer})
    if args.json:
        print(json.dumps({"status": "AI_NATIVE_COMPLETE", "run_id": run_id, "provider": args.provider, "reviewer": args.with_reviewer, "elapsed_seconds": round(elapsed, 3), "outputs": out_strs}, indent=2, ensure_ascii=False))
        return 0
    print(f"AI_NATIVE_COMPLETE provider={args.provider} reviewer={args.with_reviewer} outputs={out_strs}")
    return 0
