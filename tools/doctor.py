"""Read-only environment and provider readiness check for TAKO CLI."""

from __future__ import annotations

import os
import platform
import shutil


def _bool_label(flag: bool) -> str:
    return "YES" if flag else "NO"


def build_doctor_report() -> dict[str, object]:
    py_ver = platform.python_version()
    os_name = platform.system()
    os_rel = platform.release()
    arch = platform.machine()

    exes = {
        "claude": shutil.which("claude"),
        "copilot": shutil.which("copilot"),
        "codex": shutil.which("codex"),
        "gemini": shutil.which("gemini"),
    }
    creds = {
        "OPENAI_API_KEY": bool(os.environ.get("OPENAI_API_KEY", "").strip()),
        "ANTHROPIC_API_KEY": bool(os.environ.get("ANTHROPIC_API_KEY", "").strip()),
        "SEMANTIC_SCHOLAR_API_KEY": bool(os.environ.get("SEMANTIC_SCHOLAR_API_KEY", "").strip()),
        "GH_OR_GITHUB_TOKEN": bool(
            os.environ.get("GH_TOKEN", "").strip() or os.environ.get("GITHUB_TOKEN", "").strip()
        ),
    }

    providers = [
        {
            "name": "openalex (tako sota)",
            "installed": "YES",
            "configured": "YES (no key required)",
            "authenticated": "NOT_REQUIRED",
            "available": "YES",
            "native_search": "YES (OpenAlex Works API)",
            "live_tested": "PREVIOUSLY_QUALIFIED (Linux)",
        },
        {
            "name": "arxiv (tako sota --sources arxiv)",
            "installed": "YES",
            "configured": "YES (no key required)",
            "authenticated": "NOT_REQUIRED",
            "available": "YES",
            "native_search": "YES (arXiv Atom API; 1 req/3s pacing)",
            "live_tested": "FIXTURE_QUALIFIED",
        },
        {
            "name": "crossref (tako sota --sources crossref)",
            "installed": "YES",
            "configured": "YES (polite pool via User-Agent/mailto)",
            "authenticated": "NOT_REQUIRED",
            "available": "YES",
            "native_search": "YES (Crossref REST API)",
            "live_tested": "FIXTURE_QUALIFIED",
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
        {
            "name": "copilot-cli (tako sota-ai)",
            "installed": _bool_label(bool(exes["copilot"])),
            "configured": "SESSION_DEPENDENT (run `copilot login`)",
            "authenticated": "UNVERIFIED (never inferred from executable)",
            "available": _bool_label(bool(exes["copilot"])),
            "native_search": "NOT_VERIFIED (no documented general web search; pair with --verify-bib)",
            "live_tested": "CURRENTLY_UNVERIFIED",
        },
        {
            "name": "claude-cli (tako sota-ai)",
            "installed": _bool_label(bool(exes["claude"])),
            "configured": "SESSION_DEPENDENT (authenticated `claude` session)",
            "authenticated": "UNVERIFIED (never inferred from executable)",
            "available": _bool_label(bool(exes["claude"])),
            "native_search": "BUILT_IN_CLI_WEBSEARCH (requires authenticated session)",
            "live_tested": "CURRENTLY_UNVERIFIED",
        },
        {
            "name": "openai-responses (tako sota-ai)",
            "installed": "YES (stdlib HTTP)",
            "configured": _bool_label(creds["OPENAI_API_KEY"]),
            "authenticated": "KEY_PRESENT_UNVERIFIED" if creds["OPENAI_API_KEY"] else "NO",
            "available": _bool_label(creds["OPENAI_API_KEY"]),
            "native_search": "YES (web_search_preview)",
            "live_tested": "CURRENTLY_UNVERIFIED",
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
        {
            "name": "codex-cli",
            "installed": _bool_label(bool(exes["codex"])),
            "configured": "NOT_IMPLEMENTED",
            "authenticated": "UNSUPPORTED",
            "available": "NO (not integrated in tako sota-ai)",
            "native_search": "NOT_IMPLEMENTED",
            "live_tested": "UNSUPPORTED",
        },
        {
            "name": "gemini-cli",
            "installed": _bool_label(bool(exes["gemini"])),
            "configured": "NOT_IMPLEMENTED",
            "authenticated": "UNSUPPORTED",
            "available": "NO (not integrated in tako sota-ai)",
            "native_search": "NOT_IMPLEMENTED",
            "live_tested": "UNSUPPORTED",
        },
    ]

    local_capabilities = {
        "evidence_txt_md_json_bib_ris_jats": "SUPPORTED (local offline ingestion)",
        "evidence_pdf": "UNSUPPORTED (zero-dependency mode; supply .txt/.md/.xml or JSON)",
        "semantic_extraction": "UNAVAILABLE (normalizes supplied fields only)",
        "windows_native": "NOT_TESTED (WSL/Linux tested only; native Windows in M024)",
    }

    missing: list[str] = []
    if not creds["OPENAI_API_KEY"]:
        missing.append("OPENAI_API_KEY unset (required only for --provider openai-responses)")
    if not exes["copilot"]:
        missing.append("copilot executable not on PATH (required only for --provider copilot-cli)")
    if not exes["claude"]:
        missing.append("claude executable not on PATH (required only for --provider claude-cli)")

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
        "setup_profile": effective_cfg["profile"],
        "effective_sources": effective_cfg["sources"],
        "offline_only": effective_cfg["offline_only"],
        "assistant": effective_cfg["assistant"],
        "executables": {k: bool(v) for k, v in exes.items()},
        "credentials_present": creds,
        "providers": providers,
        "local_capabilities": local_capabilities,
        "missing_requirements": missing,
    }


def run_doctor() -> int:
    report = build_doctor_report()
    print(f"TAKO Doctor")
    print(f"Python: {report['python']}")
    print(f"OS:     {report['os']}")
    print(f"Setup Profile: {report['setup_profile']} (config={report['config_path']}, sources={','.join(report['effective_sources'])}, assistant={report['assistant']}, offline_only={report['offline_only']})")
    print("Executables (on PATH): " + ", ".join(f"{k}={_bool_label(v)}" for k, v in report["executables"].items()))
    print("Credentials (boolean only): " + ", ".join(f"{k}={_bool_label(v)}" for k, v in report["credentials_present"].items()))
    print("\nProviders:")
    for p in report["providers"]:
        print(
            f"  - {p['name']}: INSTALLED={p['installed']} | CONFIGURED={p['configured']} | "
            f"AUTHENTICATED={p['authenticated']} | AVAILABLE={p['available']} | SEARCH={p['native_search']} | LIVE_TESTED={p['live_tested']}"
        )
    print("\nLocal Evidence & Platform Capabilities:")
    for k, v in report["local_capabilities"].items():
        print(f"  - {k}: {v}")
    if report["missing_requirements"]:
        print("\nMissing / Optional Requirements:")
        for item in report["missing_requirements"]:
            print(f"  - {item}")
    return 0
