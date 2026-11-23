"""Read-only environment and provider readiness check for TAKO CLI."""

from __future__ import annotations

import argparse
import json
import os
import platform
import shutil

from tools.ai_cli_providers import detect_ai_cli_providers


def _bool_label(flag: bool) -> str:
    return "YES" if flag else "NO"


def build_doctor_report() -> dict[str, object]:
    py_ver = platform.python_version()
    os_name = platform.system()
    os_rel = platform.release()
    arch = platform.machine()

    ai_clis = detect_ai_cli_providers()
    exes = {item["assistant_key"]: item["executable_detected"] for item in ai_clis}
    creds = {
        "OPENAI_API_KEY": bool(os.environ.get("OPENAI_API_KEY", "").strip()),
        "ANTHROPIC_API_KEY": bool(os.environ.get("ANTHROPIC_API_KEY", "").strip()),
        "SEMANTIC_SCHOLAR_API_KEY": bool(os.environ.get("SEMANTIC_SCHOLAR_API_KEY", "").strip()),
        "GH_OR_GITHUB_TOKEN": bool(
            os.environ.get("GH_TOKEN", "").strip() or os.environ.get("GITHUB_TOKEN", "").strip()
        ),
    }

    providers: list[dict[str, str]] = [
        {
            "name": "openalex (tako sota)",
            "installed": "YES",
            "configured": "YES (no key required)",
            "authenticated": "NOT_REQUIRED",
            "available": "YES",
            "native_search": "YES (OpenAlex Works API)",
            "live_tested": "QUALIFIED (Linux)",
        },
        {
            "name": "arxiv (tako sota --sources arxiv)",
            "installed": "YES",
            "configured": "YES (no key required)",
            "authenticated": "NOT_REQUIRED",
            "available": "YES",
            "native_search": "YES (arXiv Atom API; 1 req/3s pacing)",
            "live_tested": "QUALIFIED (Linux)",
        },
        {
            "name": "crossref (tako sota --sources crossref)",
            "installed": "YES",
            "configured": "YES (polite pool via User-Agent/mailto)",
            "authenticated": "NOT_REQUIRED",
            "available": "YES",
            "native_search": "YES (Crossref REST API)",
            "live_tested": "QUALIFIED (Linux)",
        },
        {
            "name": "semantic-scholar (tako sota --sources semantic-scholar)",
            "installed": "YES",
            "configured": "YES (optional SEMANTIC_SCHOLAR_API_KEY)",
            "authenticated": "KEY_PRESENT_UNVERIFIED" if creds["SEMANTIC_SCHOLAR_API_KEY"] else "ANONYMOUS_SHARED_POOL",
            "available": "YES",
            "native_search": "YES (Semantic Scholar Graph API)",
            "live_tested": "FIXTURE_QUALIFIED",
        },
        {
            "name": "google-scholar-assisted",
            "installed": "YES",
            "configured": "MANUAL_ASSISTED_ONLY",
            "authenticated": "UNSUPPORTED",
            "available": "MANUAL_ONLY (assisted links + import; no scraping)",
            "native_search": "NO (no official API)",
            "live_tested": "FIXTURE_QUALIFIED",
        },
    ]

    for cli in ai_clis:
        providers.append(
            {
                "name": f"{cli['id']} (tako sota-ai)",
                "installed": _bool_label(cli["executable_detected"]),
                "configured": "CLI_SESSION (--allow-cli-session)" if cli["adapter_implemented"] else "NOT_IMPLEMENTED",
                "authenticated": str(cli["authentication_status"]),
                "available": _bool_label(cli["executable_detected"] and cli["adapter_implemented"]),
                "native_search": "YES (CLI session dependent)" if cli["adapter_implemented"] else "NOT_IMPLEMENTED",
                "live_tested": str(cli["live_inference_status"]),
            }
        )

    providers.extend(
        [
            {
                "name": "openai-responses (tako sota-ai)",
                "installed": "YES (stdlib HTTP)",
                "configured": _bool_label(creds["OPENAI_API_KEY"]),
                "authenticated": "KEY_PRESENT_UNVERIFIED" if creds["OPENAI_API_KEY"] else "NO",
                "available": _bool_label(creds["OPENAI_API_KEY"]),
                "native_search": "YES (web_search_preview; requires --allow-paid-api)",
                "live_tested": "NOT_TESTED",
            },
            {
                "name": "fixture (tako sota / sota-ai)",
                "installed": "YES",
                "configured": "YES",
                "authenticated": "NOT_REQUIRED",
                "available": "YES (offline fixture mode only)",
                "native_search": "NO (offline fixture)",
                "live_tested": "OFFLINE_QUALIFIED (unit tests)",
            },
        ]
    )

    local_capabilities = {
        "evidence_txt_md_json_bib_ris_jats": "SUPPORTED (local offline ingestion)",
        "evidence_pdf": "UNSUPPORTED (zero-dependency mode; supply .txt/.md/.xml or JSON)",
        "semantic_extraction": "UNAVAILABLE (normalizes supplied fields only)",
        "cachyos_native": "QUALIFIED (experimental CLI)",
        "ubuntu_24_04": "CI_TESTED (native desktop NOT_TESTED)",
        "windows_ci": "CI_TESTED (python-cli-windows; new Windows dev frozen)",
    }

    missing: list[str] = []
    if not any(exes.get(k) for k in ("agy", "codex", "claude", "copilot")):
        missing.append("No supported AI CLI found on PATH (agy, codex, claude, copilot)")
    if not creds["OPENAI_API_KEY"]:
        missing.append("OPENAI_API_KEY unset (required only for --provider openai-responses --allow-paid-api)")

    from tools.researcher_setup import SetupConfigError, get_user_config_path, resolve_effective_discovery_settings

    cfg_path = get_user_config_path()
    try:
        effective_cfg = resolve_effective_discovery_settings(config_path=cfg_path)
    except SetupConfigError as exc:
        effective_cfg = {"profile": f"INVALID ({exc})", "sources": [], "offline_only": False, "assistant": "none"}

    return {
        "python": py_ver,
        "os": f"{os_name} {os_rel} ({arch})",
        "config_path": str(cfg_path),
        "config_exists": cfg_path.exists(),
        "setup_profile": effective_cfg["profile"],
        "effective_sources": effective_cfg["sources"],
        "offline_only": effective_cfg["offline_only"],
        "assistant": effective_cfg["assistant"],
        "executables": exes,
        "ai_cli_providers": ai_clis,
        "credentials_present": creds,
        "providers": providers,
        "local_capabilities": local_capabilities,
        "missing_requirements": missing,
    }


def run_doctor(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="tako doctor", description="Inspect environment and provider readiness")
    parser.add_argument("--verbose", action="store_true", help="Show full provider capability table and platform details")
    parser.add_argument("--json", action="store_true", help="Emit machine-readable JSON diagnostics")
    args = parser.parse_args(argv or [])

    report = build_doctor_report()
    if args.json:
        print(json.dumps(report, indent=2, ensure_ascii=False))
        return 0

    from tools.cli import __version__

    if not args.verbose:
        ai_summary = ", ".join(
            f"{c['assistant_key']}={'DETECTED(auth_unverified)' if (c['executable_detected'] and c['adapter_implemented']) else ('UNSUPPORTED' if not c['adapter_implemented'] else 'MISSING')}"
            for c in report["ai_cli_providers"]  # type: ignore[attr-defined]
        )
        cfg_state = "configured" if report["config_exists"] else "default (run `tako setup`)"
        print(f"TAKO Doctor (v{__version__} | {report['os']} | Python {report['python']})")
        print(f"  Config:    {report['setup_profile']} ({cfg_state} | assistant={report['assistant']})")
        print(f"  Discovery: {', '.join(report['effective_sources'])} (openalex, arxiv, crossref, semantic-scholar)")
        print(f"  AI CLIs:   {ai_summary}")
        print(f"  Evidence:  txt/md/bib/ris/jats=OK | pdf=UNSUPPORTED | paid_api_key={_bool_label(report['credentials_present']['OPENAI_API_KEY'])}")  # type: ignore[index]
        if report["missing_requirements"]:
            print(f"  Notes:     {'; '.join(report['missing_requirements'])}")  # type: ignore[arg-type]
        print("Run `tako doctor --verbose` for detailed provider and platform diagnostics.")
        return 0

    print(f"TAKO Doctor (v{__version__})")
    print(f"Python: {report['python']}")
    print(f"OS:     {report['os']}")
    print(f"Setup Profile: {report['setup_profile']} (config={report['config_path']}, sources={','.join(report['effective_sources'])}, assistant={report['assistant']}, offline_only={report['offline_only']})")
    print("Executables (on PATH): " + ", ".join(f"{k}={_bool_label(v)}" for k, v in report["executables"].items()))  # type: ignore[union-attr]
    print("Credentials (boolean only): " + ", ".join(f"{k}={_bool_label(v)}" for k, v in report["credentials_present"].items()))  # type: ignore[union-attr]
    print("\nProviders:")
    for p in report["providers"]:  # type: ignore[attr-defined]
        print(
            f"  - {p['name']}: INSTALLED={p['installed']} | CONFIGURED={p['configured']} | "
            f"AUTHENTICATED={p['authenticated']} | AVAILABLE={p['available']} | SEARCH={p['native_search']} | LIVE_TESTED={p['live_tested']}"
        )
    print("\nLocal Evidence & Platform Capabilities:")
    for k, v in report["local_capabilities"].items():  # type: ignore[union-attr]
        print(f"  - {k}: {v}")
    if report["missing_requirements"]:
        print("\nNotes / Optional Requirements:")
        for item in report["missing_requirements"]:  # type: ignore[attr-defined]
            print(f"  - {item}")
    return 0
