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
        "GH_OR_GITHUB_TOKEN": bool(
            os.environ.get("GH_TOKEN", "").strip() or os.environ.get("GITHUB_TOKEN", "").strip()
        ),
    }

    providers = [
        {
            "name": "openalex (tako sota)",
            "installed": "YES",
            "configured": "YES (no key required)",
            "available": "YES",
            "native_search": "YES (OpenAlex Works API)",
            "live_tested": "PASS (Linux)",
        },
        {
            "name": "copilot-cli (tako sota-ai)",
            "installed": _bool_label(bool(exes["copilot"])),
            "configured": "SESSION_DEPENDENT (run `copilot login`)",
            "available": _bool_label(bool(exes["copilot"])),
            "native_search": "NOT_VERIFIED (no documented general web search; pair with --verify-bib)",
            "live_tested": "NOT_TESTED",
        },
        {
            "name": "claude-cli (tako sota-ai)",
            "installed": _bool_label(bool(exes["claude"])),
            "configured": "SESSION_DEPENDENT (authenticated `claude` session)",
            "available": _bool_label(bool(exes["claude"])),
            "native_search": "BUILT_IN_CLI_WEBSEARCH (requires authenticated session)",
            "live_tested": "NOT_TESTED",
        },
        {
            "name": "openai-responses (tako sota-ai)",
            "installed": "YES (stdlib HTTP)",
            "configured": _bool_label(creds["OPENAI_API_KEY"]),
            "available": _bool_label(creds["OPENAI_API_KEY"]),
            "native_search": "YES (web_search_preview)",
            "live_tested": "NOT_TESTED",
        },
        {
            "name": "fixture (tako sota-ai)",
            "installed": "YES",
            "configured": "YES",
            "available": "YES (offline test mode only)",
            "native_search": "NO (offline fixture)",
            "live_tested": "PASS (offline unit tests)",
        },
        {
            "name": "codex-cli",
            "installed": _bool_label(bool(exes["codex"])),
            "configured": "NOT_IMPLEMENTED",
            "available": "NO (not integrated in tako sota-ai)",
            "native_search": "NOT_IMPLEMENTED",
            "live_tested": "NOT_TESTED",
        },
        {
            "name": "gemini-cli",
            "installed": _bool_label(bool(exes["gemini"])),
            "configured": "NOT_IMPLEMENTED",
            "available": "NO (not integrated in tako sota-ai)",
            "native_search": "NOT_IMPLEMENTED",
            "live_tested": "NOT_TESTED",
        },
    ]

    missing: list[str] = []
    if not creds["OPENAI_API_KEY"]:
        missing.append("OPENAI_API_KEY unset (required only for --provider openai-responses)")
    if not exes["copilot"]:
        missing.append("copilot executable not on PATH (required only for --provider copilot-cli)")
    if not exes["claude"]:
        missing.append("claude executable not on PATH (required only for --provider claude-cli)")

    return {
        "python": py_ver,
        "os": f"{os_name} {os_rel} ({arch})",
        "executables": {k: bool(v) for k, v in exes.items()},
        "credentials_present": creds,
        "providers": providers,
        "missing_requirements": missing,
    }


def run_doctor() -> int:
    report = build_doctor_report()
    print(f"TAKO Doctor")
    print(f"Python: {report['python']}")
    print(f"OS:     {report['os']}")
    print("Executables (on PATH): " + ", ".join(f"{k}={_bool_label(v)}" for k, v in report["executables"].items()))
    print("Credentials (boolean only): " + ", ".join(f"{k}={_bool_label(v)}" for k, v in report["credentials_present"].items()))
    print("\nProviders:")
    for p in report["providers"]:
        print(
            f"  - {p['name']}: INSTALLED={p['installed']} | CONFIGURED={p['configured']} | "
            f"AVAILABLE={p['available']} | SEARCH={p['native_search']} | LIVE_TESTED={p['live_tested']}"
        )
    if report["missing_requirements"]:
        print("\nMissing / Optional Requirements:")
        for item in report["missing_requirements"]:
            print(f"  - {item}")
    return 0
