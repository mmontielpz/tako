"""Portable research workspace, local evidence ingestion, export, and recovery for TAKO CLI."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
import shutil
import sys
import xml.etree.ElementTree as ET
from datetime import datetime, timezone
from typing import Any

from tools.sota_evidence import (
    load_bundle_from_workflow_dir,
    run_evidence_and_comparative_pipeline,
    write_evidence_outputs,
)
from tools.sota_integrity import BIB_STATUS_VERIFIED, EVIDENCE_ABSTRACT_BASED, EVIDENCE_METADATA_ONLY

WORKSPACE_SCHEMA = "tako-workspace/v1"
WORKSPACE_FILENAME = "tako_workspace.json"
TAKO_VERSION = "0.1.0a2"

SUPPORTED_LOCAL_FORMATS = {".txt", ".md", ".json", ".bib", ".ris", ".xml", ".jats"}


def _utc_now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def _sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _sha256_file(path: pathlib.Path) -> str:
    return _sha256_bytes(path.read_bytes())


def _safe_path(path: pathlib.Path, *, base_dir: pathlib.Path | None = None, must_exist: bool = True) -> pathlib.Path:
    """Reject symlinks (including parent symlinks), traversal escaping base_dir, and missing files."""
    if path.is_symlink():
        raise ValueError(f"Unsafe symlink path rejected: {path}")
    # Check parent directories for symlinks (stop at base_dir if provided, or check non-root parents)
    cur = path.parent
    base_res = base_dir.resolve() if base_dir is not None else None
    while cur and cur != cur.parent:
        if cur.is_symlink():
            if base_res is None or cur.resolve() == base_res or base_res in cur.resolve().parents:
                raise ValueError(f"Unsafe parent-directory symlink rejected: {cur}")
        if base_res is not None and cur.resolve() == base_res:
            break
        cur = cur.parent
    resolved = path.resolve()
    for part in path.parts:
        if part == ".." and base_res is not None:
            if resolved != base_res and base_res not in resolved.parents:
                raise ValueError(f"Path traversal outside workspace rejected: {path}")
    if base_res is not None:
        if resolved != base_res and base_res not in resolved.parents:
            raise ValueError(f"Path outside workspace boundary rejected: {path}")
    if must_exist and not resolved.exists():
        raise FileNotFoundError(f"Path not found: {path}")
    return resolved


def _to_ws_rel(path: pathlib.Path, ws_dir: pathlib.Path) -> str:
    """Store workspace-relative path when inside ws_dir for directory portability."""
    p_res = path.resolve()
    ws_res = ws_dir.resolve()
    try:
        return p_res.relative_to(ws_res).as_posix()
    except ValueError:
        return str(path)


def _from_ws_rel(val: str, ws_dir: pathlib.Path) -> pathlib.Path:
    p = pathlib.Path(val)
    return p if p.is_absolute() else (ws_dir / p)


def _ensure_safe_out_dir(out_dir: pathlib.Path, *, force: bool = False, resume: bool = False) -> pathlib.Path:
    if out_dir.is_symlink():
        raise ValueError(f"Unsafe symlink output directory rejected: {out_dir}")
    if out_dir.exists():
        if not out_dir.is_dir():
            raise ValueError(f"Output path exists and is not a directory: {out_dir}")
        if any(out_dir.iterdir()) and not (force or resume):
            raise FileExistsError(
                f"Output directory '{out_dir}' is non-empty. Pass --force to overwrite or --resume to continue."
            )
    out_dir.mkdir(parents=True, exist_ok=True)
    return out_dir


def _make_research_id(topic: str, objective: str) -> str:
    slug = re.sub(r"[^a-z0-9]+", "-", topic.lower()).strip("-")[:24] or "research"
    digest = hashlib.sha256(f"{topic}\n{objective}".encode("utf-8")).hexdigest()[:8]
    return f"{slug}-{digest}"


def init_workspace(
    workspace_dir: pathlib.Path,
    *,
    topic: str,
    objective: str,
    scope: str = "",
    force: bool = False,
) -> pathlib.Path:
    if not topic.strip() or not objective.strip():
        raise ValueError("Both --topic and --objective must be non-empty.")
    if workspace_dir.is_symlink():
        raise ValueError(f"Unsafe symlink workspace path rejected: {workspace_dir}")
    manifest_p = workspace_dir / WORKSPACE_FILENAME
    if manifest_p.exists() and not force:
        raise FileExistsError(f"Workspace already exists at {manifest_p}. Pass --force to re-initialize.")

    workspace_dir.mkdir(parents=True, exist_ok=True)
    for sub in ("evidence", "runs", "exports"):
        (workspace_dir / sub).mkdir(exist_ok=True)

    manifest = {
        "schema_version": WORKSPACE_SCHEMA,
        "tako_version": TAKO_VERSION,
        "research_id": _make_research_id(topic.strip(), objective.strip()),
        "topic": topic.strip(),
        "objective": objective.strip(),
        "scope": scope.strip() or "Exploratory SOTA and evidence synthesis",
        "created_at": _utc_now(),
        "updated_at": _utc_now(),
        "provider_config": {"credentials_stored": False, "mode": "offline_or_explicit"},
        "evidence_sources": [],
        "stages": {
            "discovery": {"status": "NOT_STARTED", "outputs": [], "input_hashes": {}, "workflow_dir": None},
            "evidence": {"status": "NOT_STARTED", "outputs": [], "input_hashes": {}, "out_dir": None},
            "export": {"status": "NOT_STARTED", "outputs": [], "out_dir": None},
        },
        "quality_status": {
            "verdict": "NOT_EVALUATED",
            "report_qualification": "NOT_EVALUATED",
        },
        "limitations": [
            "PDF full-text parsing is not supported in zero-dependency mode; supply .txt/.md/.xml or JSON.",
            "Semantic extraction is unavailable; normalize_extraction normalizes supplied fields only.",
        ],
    }
    manifest_p.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    return manifest_p


def load_workspace(workspace_dir: pathlib.Path) -> dict[str, Any]:
    manifest_p = workspace_dir / WORKSPACE_FILENAME
    if manifest_p.is_symlink():
        raise ValueError(f"Unsafe symlink manifest rejected: {manifest_p}")
    if not manifest_p.is_file():
        raise FileNotFoundError(f"Workspace manifest not found: {manifest_p}. Run `tako init` first.")
    try:
        data = json.loads(manifest_p.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise ValueError(f"Malformed workspace manifest JSON in {manifest_p}: {exc}") from exc
    if not isinstance(data, dict) or data.get("schema_version") != WORKSPACE_SCHEMA:
        raise ValueError(f"Unsupported or invalid workspace schema in {manifest_p}")
    return data


def save_workspace(workspace_dir: pathlib.Path, manifest: dict[str, Any]) -> pathlib.Path:
    manifest["updated_at"] = _utc_now()
    manifest_p = workspace_dir / WORKSPACE_FILENAME
    manifest_p.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    return manifest_p


ensure_safe_out_dir = _ensure_safe_out_dir


def register_workflow_stage(
    workspace_dir: pathlib.Path,
    stage_name: str,
    *,
    status: str,
    run_id: str,
    out_dir: pathlib.Path | None = None,
    outputs: list[pathlib.Path] | None = None,
    summary: dict[str, Any] | None = None,
    error: str | None = None,
) -> pathlib.Path:
    """Update stage state in tako_workspace.json without breaking v1 compatibility."""
    manifest = load_workspace(workspace_dir)
    stages = manifest.setdefault("stages", {})
    prev = stages.get(stage_name) or {}
    rel_outputs = [_to_ws_rel(p, workspace_dir) for p in (outputs or [])] if outputs is not None else prev.get("outputs", [])
    rel_out_dir = _to_ws_rel(out_dir, workspace_dir) if out_dir is not None else prev.get("out_dir")
    entry: dict[str, Any] = {
        "status": status,
        "run_id": run_id,
        "updated_at": _utc_now(),
        "outputs": rel_outputs,
    }
    if rel_out_dir:
        entry["out_dir"] = rel_out_dir
    if summary:
        entry["summary"] = summary
    if error:
        entry["error"] = error
    stages[stage_name] = entry
    if status == "COMPLETED" and rel_out_dir and stage_name in ("evidence", "discovery"):
        manifest.setdefault("last_run_dir", rel_out_dir)
    return save_workspace(workspace_dir, manifest)


def _parse_bibtex(text: str, source_name: str) -> list[dict[str, Any]]:
    entries: list[dict[str, Any]] = []
    for idx, m in enumerate(re.finditer(r"@(\w+)\s*\{\s*([^,]+)\s*,([\s\S]*?)\n\}", text), start=1):
        body = m.group(3)
        fields: dict[str, str] = {}
        for fm in re.finditer(r"(\w+)\s*=\s*[\{\"]([^\n\}\"]*)[\}\"]", body):
            fields[fm.group(1).strip().lower()] = fm.group(2).strip()
        title = fields.get("title") or f"BibTeX Entry {idx}"
        authors = [a.strip() for a in fields.get("author", "UNKNOWN").split(" and ") if a.strip()]
        doi = fields.get("doi") or ""
        url = f"https://doi.org/{doi}" if doi else (fields.get("url") or source_name)
        abstract = fields.get("abstract") or ""
        entries.append({
            "paper_id": m.group(2).strip() or f"bib_{idx}",
            "Title": title,
            "Authors": authors,
            "Year": fields.get("year") or "UNKNOWN",
            "Venue": fields.get("journal") or fields.get("booktitle") or "UNKNOWN",
            "DOI or source URL": url,
            "Bibliographic status": BIB_STATUS_VERIFIED if (doi and fields.get("year")) else "UNVERIFIABLE",
            "Evidence availability": EVIDENCE_ABSTRACT_BASED if abstract else EVIDENCE_METADATA_ONLY,
            "abstract": abstract,
            "evidence_source": source_name,
            "claims": [],
        })
    return entries


def _parse_ris(text: str, source_name: str) -> list[dict[str, Any]]:
    entries: list[dict[str, Any]] = []
    cur: dict[str, Any] = {"Authors": []}
    idx = 0
    for line in text.splitlines():
        m = re.match(r"^([A-Z0-9]{2})\s{2}-\s*(.*)$", line.rstrip())
        if not m:
            continue
        tag, val = m.group(1), m.group(2).strip()
        if tag == "ER":
            idx += 1
            doi = cur.get("doi", "")
            abstract = cur.get("abstract", "")
            entries.append({
                "paper_id": f"ris_{idx}",
                "Title": cur.get("title") or f"RIS Entry {idx}",
                "Authors": cur.get("Authors") or ["UNKNOWN"],
                "Year": cur.get("year") or "UNKNOWN",
                "Venue": cur.get("venue") or "UNKNOWN",
                "DOI or source URL": f"https://doi.org/{doi}" if doi else source_name,
                "Bibliographic status": BIB_STATUS_VERIFIED if (doi and cur.get("year")) else "UNVERIFIABLE",
                "Evidence availability": EVIDENCE_ABSTRACT_BASED if abstract else EVIDENCE_METADATA_ONLY,
                "abstract": abstract,
                "evidence_source": source_name,
                "claims": [],
            })
            cur = {"Authors": []}
        elif tag in ("TI", "T1"):
            cur["title"] = val
        elif tag in ("AU", "A1"):
            cur["Authors"].append(val)
        elif tag in ("PY", "Y1"):
            cur["year"] = val[:4]
        elif tag == "DO":
            cur["doi"] = val
        elif tag in ("JO", "T2", "JF"):
            cur["venue"] = val
        elif tag == "AB":
            cur["abstract"] = val
    return entries


def _parse_jats_xml(text: str, source_name: str, file_hash: str) -> list[dict[str, Any]]:
    if re.search(r"<!(?:DOCTYPE|ENTITY)", text, re.IGNORECASE):
        raise ValueError(f"Unsafe XML DOCTYPE/ENTITY declaration rejected in {source_name}")
    root = ET.fromstring(text)
    title_el = root.find(".//article-title")
    abs_el = root.find(".//abstract")
    body_el = root.find(".//body")
    title = "".join(title_el.itertext()).strip() if title_el is not None else pathlib.Path(source_name).stem
    abstract = "".join(abs_el.itertext()).strip() if abs_el is not None else ""
    body = "".join(body_el.itertext()).strip() if body_el is not None else ""
    rec: dict[str, Any] = {
        "paper_id": f"jats_{file_hash[:8]}",
        "Title": title,
        "Authors": ["UNKNOWN"],
        "Year": "UNKNOWN",
        "DOI or source URL": source_name,
        "Bibliographic status": "UNVERIFIABLE",
        "full_text": body,
        "abstract": abstract,
        "evidence_source": source_name,
        "retrieval_method": "local_jats_xml",
        "claims": [],
    }
    if not body:
        rec["Evidence availability"] = EVIDENCE_ABSTRACT_BASED if abstract else EVIDENCE_METADATA_ONLY
        rec["extraction_limitations"] = ["JATS XML missing <body> content; full text unavailable."]
    return [rec]


def ingest_local_evidence(
    source_paths: list[pathlib.Path],
    *,
    base_dir: pathlib.Path | None = None,
) -> tuple[list[dict[str, Any]], list[dict[str, str]]]:
    """Ingest local .txt, .md, .json, .bib, .ris, or .xml/.jats files safely."""
    papers: list[dict[str, Any]] = []
    source_records: list[dict[str, str]] = []
    for raw_p in source_paths:
        p = _safe_path(raw_p, base_dir=base_dir, must_exist=True)
        if not p.is_file():
            raise ValueError(f"Source path is not a file: {raw_p}")
        ext = p.suffix.lower()
        if ext == ".pdf":
            raise ValueError(
                f"Unsupported format '.pdf' for '{raw_p}': PDF parsing is unavailable in zero-dependency mode. "
                "Supply plain text (.txt), Markdown (.md), JATS XML (.xml), BibTeX (.bib), RIS (.ris), or JSON (.json)."
            )
        if ext not in SUPPORTED_LOCAL_FORMATS:
            raise ValueError(
                f"Unsupported evidence format '{ext}' for '{raw_p}'. Supported: {', '.join(sorted(SUPPORTED_LOCAL_FORMATS))}"
            )

        raw_bytes = p.read_bytes()
        fhash = _sha256_bytes(raw_bytes)
        raw_text = raw_bytes.decode("utf-8")
        source_records.append({"path": str(raw_p), "sha256": fhash, "format": ext})

        if ext in (".txt", ".md"):
            lines = raw_text.splitlines()
            first_h = next((ln.lstrip("# ").strip() for ln in lines if ln.strip()), p.stem)
            papers.append({
                "paper_id": f"local_{fhash[:8]}",
                "Title": first_h[:120],
                "Authors": ["UNKNOWN"],
                "Year": "UNKNOWN",
                "DOI or source URL": str(raw_p),
                "Bibliographic status": "UNVERIFIABLE",
                "local_text_path": str(p),
                "full_text": raw_text.strip(),
                "evidence_source": str(raw_p),
                "retrieval_method": "local_markdown_file" if ext == ".md" else "local_plain_text_file",
                "claims": [],
            })
        elif ext == ".json":
            try:
                obj = json.loads(raw_text)
            except json.JSONDecodeError as exc:
                raise ValueError(f"Malformed JSON in '{raw_p}': {exc}") from exc
            if isinstance(obj, dict) and isinstance(obj.get("papers"), list):
                for item in obj["papers"]:
                    if isinstance(item, dict):
                        item["_imported_untrusted"] = True
                papers.extend(obj["papers"])
            elif isinstance(obj, dict) and ("Title" in obj or "title" in obj or "paper_id" in obj):
                obj["_imported_untrusted"] = True
                papers.append(obj)
            else:
                raise ValueError(f"Unexpected JSON schema in '{raw_p}': expected object with 'papers' list or paper record.")
        elif ext == ".bib":
            parsed = _parse_bibtex(raw_text, str(raw_p))
            if not parsed:
                raise ValueError(f"No valid BibTeX entries found in '{raw_p}'.")
            papers.extend(parsed)
        elif ext == ".ris":
            parsed = _parse_ris(raw_text, str(raw_p))
            if not parsed:
                raise ValueError(f"No valid RIS entries found in '{raw_p}'.")
            papers.extend(parsed)
        elif ext in (".xml", ".jats"):
            papers.extend(_parse_jats_xml(raw_text, str(raw_p), fhash))

    return papers, source_records


def _export_bibtex_to_ris(bib_path: pathlib.Path, ris_out: pathlib.Path) -> bool:
    if not bib_path.is_file():
        return False
    text = bib_path.read_text(encoding="utf-8")
    parsed = _parse_bibtex(text, str(bib_path))
    lines: list[str] = []
    for p in parsed:
        lines.append("TY  - JOUR")
        lines.append(f"TI  - {p.get('Title', 'UNKNOWN')}")
        for a in p.get("Authors") or []:
            lines.append(f"AU  - {a}")
        if p.get("Year") and p.get("Year") != "UNKNOWN":
            lines.append(f"PY  - {p['Year']}")
        doi_url = str(p.get("DOI or source URL") or "")
        if "doi.org/" in doi_url:
            lines.append(f"DO  - {doi_url.split('doi.org/', 1)[1]}")
        lines.append("ER  - ")
        lines.append("")
    ris_out.write_text("\n".join(lines), encoding="utf-8")
    return True


def cmd_init(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="tako init", description="Initialize a portable TAKO research workspace")
    parser.add_argument("--topic", required=True, help="Research topic")
    parser.add_argument("--objective", required=True, help="Research objective")
    parser.add_argument("--scope", default="", help="Optional research scope")
    parser.add_argument("--workspace", type=pathlib.Path, default=pathlib.Path("."), help="Workspace directory")
    parser.add_argument("--force", action="store_true", help="Overwrite existing workspace manifest")
    args = parser.parse_args(argv)
    try:
        mp = init_workspace(args.workspace, topic=args.topic, objective=args.objective, scope=args.scope, force=args.force)
    except Exception as exc:
        print(f"INIT_ERROR: {exc}", file=sys.stderr)
        return 2
    print(f"WORKSPACE_INITIALIZED path={mp}")
    return 0


def cmd_status(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="tako status", description="Inspect TAKO research workspace state")
    parser.add_argument("--workspace", type=pathlib.Path, default=pathlib.Path("."), help="Workspace directory")
    parser.add_argument("--json", action="store_true", help="Emit machine-readable JSON status")
    args = parser.parse_args(argv)
    try:
        m = load_workspace(args.workspace)
    except Exception as exc:
        print(f"STATUS_ERROR: {exc}", file=sys.stderr)
        return 2

    stages_view: dict[str, dict[str, Any]] = {}
    for name, st in (m.get("stages") or {}).items():
        status_val = str(st.get("status") or "NOT_STARTED")
        outs = list(st.get("outputs") or [])
        missing = [o for o in outs if not _from_ws_rel(o, args.workspace).exists()] if outs else []
        if status_val == "COMPLETED" and outs and missing:
            status_val = "STALE"
        stages_view[name] = {
            **st,
            "effective_status": status_val,
            "missing_outputs": missing,
        }

    if args.json:
        print(json.dumps({**m, "stages_effective": stages_view}, indent=2, ensure_ascii=False))
        return 0

    print(f"Research ID:  {m['research_id']}")
    print(f"Topic:        {m['topic']}")
    print(f"Objective:    {m['objective']}")
    print(f"TAKO Version: {m['tako_version']}")
    print(f"Quality:      {m['quality_status'].get('verdict')} ({m['quality_status'].get('report_qualification')})")
    print("Stages:")
    for name, st in stages_view.items():
        extra = f", run_id={st['run_id']}" if st.get("run_id") else ""
        if st.get("missing_outputs"):
            extra += f", missing={len(st['missing_outputs'])}"
        print(f"  - {name}: {st['effective_status']} (outputs={len(st.get('outputs') or [])}{extra})")
    print(f"Evidence Sources: {len(m.get('evidence_sources') or [])}")
    return 0


def cmd_evidence(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="tako evidence",
        description="Run evidence acquisition, claim verification, comparative SOTA, and Tier 1 gating",
    )
    parser.add_argument("--input", type=pathlib.Path, help="Input JSON bundle")
    parser.add_argument("--from-workflow-dir", type=pathlib.Path, help="Existing sota or sota-ai output directory")
    parser.add_argument("--source", type=pathlib.Path, nargs="+", help="Local evidence files (.txt, .md, .json, .bib, .ris, .xml)")
    parser.add_argument("--workspace", type=pathlib.Path, help="Optional TAKO workspace directory")
    parser.add_argument("--out-dir", type=pathlib.Path, default=None, help="Output directory")
    parser.add_argument("--resume", action="store_true", help="Resume from verified workspace state")
    parser.add_argument("--force", action="store_true", help="Allow overwriting non-empty output directory")
    args = parser.parse_args(argv)

    if not any((args.input, args.from_workflow_dir, args.source, args.workspace)):
        print("ERROR: provide --input, --from-workflow-dir, --source, or --workspace.", file=sys.stderr)
        return 2

    ws_manifest: dict[str, Any] | None = None
    if args.workspace:
        try:
            ws_manifest = load_workspace(args.workspace)
        except Exception as exc:
            print(f"WORKSPACE_ERROR: {exc}", file=sys.stderr)
            return 2
        if args.resume:
            ev_stage = (ws_manifest.get("stages") or {}).get("evidence", {})
            recorded_hashes = ev_stage.get("input_hashes") or {}
            for src_path_str, old_hash in recorded_hashes.items():
                sp = _from_ws_rel(src_path_str, args.workspace)
                if not sp.is_file() or _sha256_file(sp) != old_hash:
                    print(f"RESUME_REFUSED: stale or missing input hash for {src_path_str}", file=sys.stderr)
                    return 2

    out_dir = args.out_dir or (args.workspace / "runs" / "evidence" if args.workspace else pathlib.Path("sota_evidence_output"))
    try:
        _ensure_safe_out_dir(out_dir, force=args.force, resume=args.resume)
    except Exception as exc:
        print(f"OUTPUT_ERROR: {exc}", file=sys.stderr)
        return 2

    bundle: dict[str, Any] = {
        "topic": ws_manifest["topic"] if ws_manifest else "Research Topic",
        "objective": ws_manifest["objective"] if ws_manifest else "Evidence-grounded SOTA analysis",
        "papers": [],
    }
    input_hashes: dict[str, str] = {}

    try:
        if args.input:
            in_p = _safe_path(args.input, must_exist=True)
            try:
                loaded = json.loads(in_p.read_text(encoding="utf-8"))
            except json.JSONDecodeError as exc:
                raise ValueError(f"Malformed JSON in {args.input}: {exc}") from exc
            if not isinstance(loaded, dict) or not isinstance(loaded.get("papers"), list):
                raise ValueError(f"Unexpected bundle schema in {args.input}: 'papers' list required.")
            if ws_manifest is not None:
                in_topic = str(loaded.get("topic") or "").strip()
                in_obj = str(loaded.get("objective") or "").strip()
                if in_topic and in_topic.lower() != ws_manifest["topic"].lower():
                    raise ValueError(f"WORKSPACE_CONFLICT: input topic '{in_topic}' conflicts with workspace topic '{ws_manifest['topic']}'.")
                if in_obj and in_obj.lower() != ws_manifest["objective"].lower():
                    raise ValueError(f"WORKSPACE_CONFLICT: input objective '{in_obj}' conflicts with workspace objective '{ws_manifest['objective']}'.")
            for item in loaded["papers"]:
                if isinstance(item, dict):
                    item["_imported_untrusted"] = True
            bundle = loaded
            h_key = _to_ws_rel(in_p, args.workspace) if args.workspace else str(args.input)
            input_hashes[h_key] = _sha256_file(in_p)

        if args.from_workflow_dir:
            wf_p = _safe_path(args.from_workflow_dir, must_exist=True)
            wf_bundle = load_bundle_from_workflow_dir(wf_p)
            if ws_manifest is not None:
                plan_prov = (wf_bundle.get("discovery_provenance") or {}).get("research_plan") or {}
                wf_topic = str(plan_prov.get("topic") or "").strip()
                wf_obj = str(plan_prov.get("objective") or "").strip()
                if wf_topic and wf_topic.lower() != ws_manifest["topic"].lower():
                    raise ValueError(f"WORKSPACE_CONFLICT: workflow topic '{wf_topic}' conflicts with workspace topic '{ws_manifest['topic']}'.")
                if wf_obj and wf_obj.lower() != ws_manifest["objective"].lower():
                    raise ValueError(f"WORKSPACE_CONFLICT: workflow objective '{wf_obj}' conflicts with workspace objective '{ws_manifest['objective']}'.")
            if not args.input:
                bundle["topic"] = ws_manifest["topic"] if ws_manifest else wf_bundle["topic"]
                bundle["objective"] = ws_manifest["objective"] if ws_manifest else wf_bundle["objective"]
            bundle["discovery_provenance"] = wf_bundle.get("discovery_provenance")
            bundle["papers"].extend(wf_bundle.get("papers") or [])

        if args.source:
            src_papers, src_recs = ingest_local_evidence(args.source)
            bundle["papers"].extend(src_papers)
            for r in src_recs:
                h_key = _to_ws_rel(pathlib.Path(r["path"]), args.workspace) if args.workspace else r["path"]
                input_hashes[h_key] = r["sha256"]
                if ws_manifest is not None:
                    ws_manifest["evidence_sources"].append({"path": h_key, "sha256": r["sha256"], "format": r["format"]})

        res = run_evidence_and_comparative_pipeline(bundle)
        outs = write_evidence_outputs(out_dir, res)
    except Exception as exc:
        if ws_manifest is not None and args.workspace is not None:
            ws_manifest["stages"]["evidence"]["status"] = "FAILED"
            ws_manifest["stages"]["evidence"]["error"] = str(exc)
            save_workspace(args.workspace, ws_manifest)
        print(f"EVIDENCE_ERROR: {exc}", file=sys.stderr)
        return 2

    if ws_manifest is not None and args.workspace is not None:
        ws_manifest["stages"]["evidence"] = {
            "status": "COMPLETED",
            "recovery_mode": "deterministic_stage_restart" if args.resume else "fresh_execution",
            "out_dir": _to_ws_rel(out_dir, args.workspace),
            "outputs": [_to_ws_rel(p, args.workspace) for p in outs],
            "input_hashes": input_hashes,
        }
        ws_manifest["quality_status"] = {
            "verdict": res["quality_verdict"],
            "report_qualification": res["report_qualification"],
        }
        save_workspace(args.workspace, ws_manifest)

    print(f"EVIDENCE_SOTA_COMPLETE qualification={res['report_qualification']} verdict={res['quality_verdict']} outputs={[str(p) for p in outs]}")
    return 0


def cmd_export(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="tako export", description="Export portable scientific artifacts")
    parser.add_argument("--workspace", type=pathlib.Path, help="TAKO workspace directory")
    parser.add_argument("--from-dir", type=pathlib.Path, help="Run output directory to export from")
    parser.add_argument("--out-dir", type=pathlib.Path, required=True, help="Export destination directory")
    parser.add_argument("--format", choices=["all", "markdown", "csv", "json", "bibtex", "ris"], default="all")
    parser.add_argument("--force", action="store_true", help="Allow overwriting non-empty export directory")
    args = parser.parse_args(argv)

    src_dir = args.from_dir
    ws_manifest: dict[str, Any] | None = None
    if args.workspace:
        try:
            ws_manifest = load_workspace(args.workspace)
        except Exception as exc:
            print(f"WORKSPACE_ERROR: {exc}", file=sys.stderr)
            return 2
        if src_dir is None:
            ev_out = (ws_manifest.get("stages") or {}).get("evidence", {}).get("out_dir")
            if ev_out:
                src_dir = _from_ws_rel(ev_out, args.workspace)

    if src_dir is None or not src_dir.is_dir():
        print("EXPORT_ERROR: source run directory not found. Pass --from-dir or run `tako evidence` in workspace.", file=sys.stderr)
        return 2

    try:
        dest = _ensure_safe_out_dir(args.out_dir, force=args.force)
    except Exception as exc:
        print(f"EXPORT_ERROR: {exc}", file=sys.stderr)
        return 2

    ext_map = {
        "markdown": {".md"},
        "csv": {".csv"},
        "json": {".json"},
        "bibtex": {".bib"},
        "ris": set(),
        "all": {".md", ".csv", ".json", ".bib"},
    }
    allowed_exts = ext_map[args.format]
    exported: list[dict[str, Any]] = []
    for item in sorted(src_dir.iterdir()):
        if item.is_file() and not item.is_symlink() and item.suffix.lower() in allowed_exts:
            target = dest / item.name
            shutil.copy2(item, target)
            exported.append({"filename": item.name, "bytes": target.stat().st_size, "sha256": _sha256_file(target)})

    if args.format in ("all", "ris"):
        ris_p = dest / "references.ris"
        if _export_bibtex_to_ris(src_dir / "references.bib", ris_p):
            exported.append({"filename": ris_p.name, "bytes": ris_p.stat().st_size, "sha256": _sha256_file(ris_p)})

    inv_p = dest / "artifact_inventory.json"
    inv_p.write_text(json.dumps({"exported_from": str(src_dir), "format": args.format, "artifacts": exported}, indent=2) + "\n", encoding="utf-8")

    if ws_manifest is not None and args.workspace is not None:
        ws_manifest["stages"]["export"] = {"status": "COMPLETED", "out_dir": _to_ws_rel(dest, args.workspace), "outputs": [a["filename"] for a in exported] + ["artifact_inventory.json"]}
        save_workspace(args.workspace, ws_manifest)

    print(f"EXPORT_COMPLETE count={len(exported)} out_dir={dest}")
    return 0
