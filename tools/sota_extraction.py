"""Structured scientific evidence extraction contract for TAKO CLI and Research Mate (M029C, F-008)."""

from __future__ import annotations

import json
import re
import shutil
import subprocess
from typing import Any

from tools.ai_cli_providers import (
    ProviderCallBudget,
    authorize_provider_execution,
    build_cli_invocation,
    decode_cli_provider_stdout,
    parse_structured_json_response,
    sanitized_cli_env,
)

EXTRACTION_SCHEMA_VERSION = "tako-extraction/v1"

STRUCTURED_FIELDS = (
    "research_problem",
    "research_objective",
    "scientific_contribution",
    "methodology",
    "architecture",
    "datasets",
    "experimental_protocols",
    "metrics",
    "reported_results",
    "limitations",
    "research_gaps",
)

EVIDENCE_TYPES = {
    "FULL_TEXT",
    "ABSTRACT_BASED",
    "METADATA_ONLY",
    "MODEL_RECALL",
    "UNAVAILABLE",
}


def _normalize_evidence_type(raw: str | None) -> str:
    val = str(raw or "").strip().upper()
    if "FULL_TEXT" in val:
        return "FULL_TEXT"
    if "ABSTRACT" in val:
        return "ABSTRACT_BASED"
    if "METADATA" in val:
        return "METADATA_ONLY"
    if "MODEL_RECALL" in val:
        return "MODEL_RECALL"
    return "UNAVAILABLE"


def _first_matching_sentence(text: str, keywords: tuple[str, ...]) -> str:
    for sent in re.split(r"(?<=[.!?])\s+", text.strip()):
        sl = sent.lower()
        if any(k in sl for k in keywords):
            return sent.strip()
    return ""


def extract_structured_deterministic(
    record: dict[str, Any],
    *,
    source_artifact: str = "literature_matrix.csv",
) -> dict[str, Any]:
    """Deterministic-first structured extraction preserving evidence boundaries without inference."""
    pub_id = str(record.get("DOI or source URL") or record.get("paper_id") or record.get("Title") or "UNKNOWN")
    ev_type = _normalize_evidence_type(record.get("Evidence availability") or record.get("Evidence level"))
    abstract = str(record.get("_abstract") or record.get("abstract") or "").strip()
    full_text = str(record.get("full_text") or "").strip()
    corpus_text = full_text or abstract

    if full_text:
        ev_type = "FULL_TEXT"
    elif abstract and ev_type in ("UNAVAILABLE", "METADATA_ONLY"):
        ev_type = "ABSTRACT_BASED"

    fields: dict[str, str] = {}
    missing: list[str] = []

    def _pick(field_name: str, existing_key: str, cues: tuple[str, ...]) -> None:
        val = str(record.get(existing_key) or "").strip()
        if val and val.upper() not in ("UNKNOWN", "NOT_EXTRACTED", "NOT_APPLICABLE"):
            fields[field_name] = val
            return
        if corpus_text:
            hit = _first_matching_sentence(corpus_text, cues)
            if hit:
                fields[field_name] = hit
                return
        fields[field_name] = "NOT_AVAILABLE"
        missing.append(field_name)

    _pick("research_problem", "Research problem", ("problem", "challenge", "struggle", "difficult"))
    _pick("research_objective", "Research objective", ("we propose", "this paper", "aim", "present", "introduce"))
    _pick("scientific_contribution", "Research contribution", ("propose", "introduce", "novel", "design", "achieve"))
    _pick("methodology", "Methodology", ("method", "approach", "pipeline", "training", "loss", "algorithm"))
    _pick("architecture", "Architecture", ("architecture", "network", "backbone", "encoder", "decoder", "module", "block"))
    _pick("datasets", "Datasets", ("cityscapes", "camvid", "ade20k", "coco", "imagenet", "dataset", "benchmark"))
    _pick("experimental_protocols", "Experimental protocols", ("protocol", "resolution", "fp16", "tensorrt", "jetson", "gpu", "batch"))
    _pick("metrics", "Metrics", ("miou", "fps", "ms", "flops", "params", "accuracy", "latency", "throughput"))
    _pick("reported_results", "Reported results", ("achieves", "reaches", "outperforms", "%", "fps", "miou"))
    _pick("limitations", "Limitations", ("limitation", "however", "drawback", "trade-off", "fails", "constrained"))
    _pick("research_gaps", "Research gaps", ("future", "gap", "open", "unexplored", "remains"))

    locator = "full_text" if full_text else ("abstract" if abstract else "NONE")
    if ev_type in ("FULL_TEXT", "ABSTRACT_BASED") and any(v != "NOT_AVAILABLE" for v in fields.values()):
        ver_status = "GROUNDED_FULL_TEXT" if ev_type == "FULL_TEXT" else "ABSTRACT_ONLY_UNVERIFIED_FOR_FULL_CLAIMS"
    else:
        ver_status = "UNVERIFIED"

    claims: list[dict[str, Any]] = []
    for fname in ("scientific_contribution", "reported_results", "limitations"):
        if fields[fname] != "NOT_AVAILABLE":
            claims.append(
                {
                    "claim_id": f"{pub_id}::{fname}",
                    "publication_id": pub_id,
                    "field": fname,
                    "statement": fields[fname],
                    "source_artifact": source_artifact,
                    "evidence_locator": locator,
                    "evidence_type": ev_type,
                    "verification_status": ver_status,
                    "missing_information": list(missing),
                    "human_review_status": "PENDING_HUMAN_REVIEW",
                }
            )

    return {
        "schema_version": EXTRACTION_SCHEMA_VERSION,
        "publication_id": pub_id,
        "evidence_type": ev_type,
        "verification_status": ver_status,
        "fields": fields,
        "missing_information": missing,
        "claims": claims,
        "human_review_status": "PENDING_HUMAN_REVIEW",
    }


def extract_structured_with_cli(
    record: dict[str, Any],
    *,
    provider: str,
    allow_cli_session: bool = False,
    allow_paid_api: bool = False,
    allow_unisolated_cli: bool = False,
    fixture_payload: dict[str, Any] | None = None,
    source_artifact: str = "literature_matrix.csv",
    timeout: float = 60.0,
    budget: ProviderCallBudget | None = None,
) -> dict[str, Any]:
    """Optional CLI-first AI extraction using M029A authorization boundary."""
    base = extract_structured_deterministic(record, source_artifact=source_artifact)
    if provider == "fixture":
        if fixture_payload:
            for k in STRUCTURED_FIELDS:
                if k in fixture_payload and str(fixture_payload[k]).strip():
                    base["fields"][k] = str(fixture_payload[k]).strip()
            base["missing_information"] = [k for k in STRUCTURED_FIELDS if base["fields"][k] == "NOT_AVAILABLE"]
        return base

    ok, reason = authorize_provider_execution(
        provider,
        allow_cli_session=allow_cli_session,
        allow_paid_api=allow_paid_api,
        allow_unisolated_cli=allow_unisolated_cli,
    )
    if not ok:
        raise PermissionError(reason)

    exe_name = provider.replace("-cli", "")
    exe = shutil.which(exe_name)
    if not exe:
        raise FileNotFoundError(f"{exe_name} CLI not found on PATH")

    text_input = str(record.get("full_text") or record.get("_abstract") or record.get("abstract") or "").strip()
    if not text_input:
        # Never fabricate extraction when no text is available
        return base

    prompt = (
        "Extract ONLY facts explicitly stated in the provided text as a single JSON object with keys: "
        + ", ".join(STRUCTURED_FIELDS)
        + ". Use 'NOT_AVAILABLE' for any unstated field. Text:\n"
        + text_input[:3000]
    )
    if budget is not None:
        budget.consume("extraction")

    cmd = build_cli_invocation(provider, exe, prompt)
    try:
        proc = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, env=sanitized_cli_env())
        if proc.returncode != 0:
            raise RuntimeError(f"{provider} failed (exit {proc.returncode}): {proc.stderr.strip()[:200]}")
        assistant_text, _ = decode_cli_provider_stdout(provider, proc.stdout, proc.stderr)
        parsed = parse_structured_json_response(assistant_text, provider)
    except Exception:
        if budget is not None:
            budget.record_failure()
        raise
    if budget is not None:
        budget.record_success()
    if isinstance(parsed, dict):
        for k in STRUCTURED_FIELDS:
            if k in parsed and str(parsed[k]).strip():
                base["fields"][k] = str(parsed[k]).strip()
        base["missing_information"] = [k for k in STRUCTURED_FIELDS if base["fields"][k] == "NOT_AVAILABLE"]
    return base
