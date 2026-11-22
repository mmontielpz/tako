"""Adaptive Research Setup Wizard, access profiles, and non-secret user configuration (M024D)."""

from __future__ import annotations

import json
import os
import pathlib
import re
import sys
from typing import Any

from tools.sota_discovery.providers import PROVIDER_REGISTRY, list_provider_capabilities

CONFIG_SCHEMA_VERSION = "tako_user_config_v1"
DEFAULT_SAFE_MAX_HTTP_REQUESTS = 24

ACCESS_PROFILES: dict[str, dict[str, Any]] = {
    "free": {
        "profile": "free",
        "label": "Free & Open Access (Recommended)",
        "description": "Zero-cost, keyless open scientific sources (OpenAlex, arXiv, Crossref).",
        "default_sources": ["openalex", "arxiv", "crossref"],
        "allow_paid_operations": False,
        "offline_only": False,
        "default_max_http_requests": DEFAULT_SAFE_MAX_HTTP_REQUESTS,
        "recommended_assistant": "agy",
        "native_ai_provider": "none",
    },
    "hybrid": {
        "profile": "hybrid",
        "label": "Hybrid (Open Sources + AI CLI Assistant)",
        "description": "Free open sources plus optional Semantic Scholar and external/CLI AI assistant workflow.",
        "default_sources": ["openalex", "arxiv", "crossref", "semantic-scholar"],
        "allow_paid_operations": False,
        "offline_only": False,
        "default_max_http_requests": DEFAULT_SAFE_MAX_HTTP_REQUESTS,
        "recommended_assistant": "copilot",
        "native_ai_provider": "copilot-cli",
    },
    "institutional": {
        "profile": "institutional",
        "label": "Institutional Access (Checklist + Open Sources)",
        "description": "Prepares institutional license checklist (Scopus/IEEE/WoS remain disabled until adapters ship) alongside open sources.",
        "default_sources": ["openalex", "arxiv", "crossref"],
        "allow_paid_operations": False,
        "offline_only": False,
        "default_max_http_requests": DEFAULT_SAFE_MAX_HTTP_REQUESTS,
        "recommended_assistant": "agy",
        "native_ai_provider": "none",
    },
    "paid": {
        "profile": "paid",
        "label": "Budget-Controlled Paid (Explicit Per-Run Authorization)",
        "description": "Records preference for paid AI synthesis providers; never authorizes paid calls without explicit --allow-paid-api.",
        "default_sources": ["openalex", "arxiv", "crossref", "semantic-scholar"],
        "allow_paid_operations": False,
        "offline_only": False,
        "default_max_http_requests": DEFAULT_SAFE_MAX_HTTP_REQUESTS,
        "recommended_assistant": "copilot",
        "native_ai_provider": "openai-responses",
    },
    "offline": {
        "profile": "offline",
        "label": "Offline Only (Fixture & Local Evidence)",
        "description": "Blocks all live network discovery and paid API calls; local fixtures and evidence ingestion only.",
        "default_sources": ["fixture"],
        "allow_paid_operations": False,
        "offline_only": True,
        "default_max_http_requests": 0,
        "recommended_assistant": "agy",
        "native_ai_provider": "fixture",
    },
}

ASSISTANT_OPTIONS: dict[str, dict[str, str]] = {
    "agy": {
        "id": "agy",
        "mode": "cli_first_and_external_operator",
        "guidance": "Antigravity (AGY) operates TAKO via local CLI commands or `tako sota-ai --provider agy-cli --allow-cli-session`.",
    },
    "codex": {
        "id": "codex",
        "mode": "cli_first_subscription",
        "guidance": "OpenAI Codex CLI can be invoked via `tako sota-ai --provider codex-cli --allow-cli-session` using an authenticated `codex` session.",
    },
    "claude": {
        "id": "claude",
        "mode": "cli_first_subscription",
        "guidance": "Claude Code CLI can be invoked via `tako sota-ai --provider claude-cli --allow-cli-session` using an authenticated `claude` session.",
    },
    "copilot": {
        "id": "copilot",
        "mode": "cli_first_subscription",
        "guidance": "GitHub Copilot CLI can operate TAKO or be invoked via `tako sota-ai --provider copilot-cli --allow-cli-session` (run `copilot login` first).",
    },
    "other": {
        "id": "other",
        "mode": "external_cli_operator",
        "guidance": "Any local CLI-capable assistant can drive TAKO via `--help` and machine-readable JSON/CSV/BibTeX outputs.",
    },
    "none": {
        "id": "none",
        "mode": "direct_cli_only",
        "guidance": "Direct researcher CLI usage without an external AI assistant.",
    },
}

_SECRET_KEY_PATTERN = re.compile(r"(api_?key|secret|token|password|credential)", re.IGNORECASE)
_SECRET_VALUE_PATTERN = re.compile(r"(sk-[A-Za-z0-9]{10,}|ghp_[A-Za-z0-9]{10,}|gho_[A-Za-z0-9]{10,}|Bearer\s+\S+)")


class SetupConfigError(ValueError):
    """Raised when user configuration is invalid, unsafe, or conflicts with an existing file."""


def get_user_config_path(*, platform_name: str | None = None, env: dict[str, str] | None = None) -> pathlib.Path:
    """Return the platform-appropriate non-secret user configuration path."""
    environ = env if env is not None else os.environ
    override = environ.get("TAKO_CONFIG_PATH", "").strip()
    if override:
        return pathlib.Path(override)

    is_windows = (platform_name or os.name) in ("nt", "Windows", "windows")
    if is_windows:
        appdata = environ.get("APPDATA", "").strip()
        if appdata:
            return pathlib.Path(appdata) / "tako" / "config.json"
        return pathlib.Path.home() / "AppData" / "Roaming" / "tako" / "config.json"

    xdg = environ.get("XDG_CONFIG_HOME", "").strip()
    base = pathlib.Path(xdg) if xdg else (pathlib.Path.home() / ".config")
    return base / "tako" / "config.json"


def _scan_for_secrets(obj: Any, prefix: str = "") -> None:
    if isinstance(obj, dict):
        for k, v in obj.items():
            full_k = f"{prefix}.{k}" if prefix else str(k)
            if _SECRET_KEY_PATTERN.search(str(k)):
                raise SetupConfigError(f"Refusing to store secret-like field '{full_k}' in user config; use environment variables instead.")
            _scan_for_secrets(v, full_k)
    elif isinstance(obj, list):
        for idx, item in enumerate(obj):
            _scan_for_secrets(item, f"{prefix}[{idx}]")
    elif isinstance(obj, str) and _SECRET_VALUE_PATTERN.search(obj):
        raise SetupConfigError(f"Refusing to store credential-like value in user config field '{prefix}'.")


def validate_user_config(cfg: dict[str, Any]) -> dict[str, Any]:
    """Validate non-secret user configuration and enforce implemented-only source selection."""
    if not isinstance(cfg, dict):
        raise SetupConfigError("User configuration must be a JSON object.")
    _scan_for_secrets(cfg)

    profile = str(cfg.get("profile") or "").strip().lower()
    if profile not in ACCESS_PROFILES:
        raise SetupConfigError(f"Unsupported profile {profile!r}. Choose from: {', '.join(ACCESS_PROFILES)}.")

    sources = cfg.get("sources")
    if not isinstance(sources, list) or not sources:
        raise SetupConfigError("Configuration 'sources' must be a non-empty list of implemented provider IDs.")
    clean_sources: list[str] = []
    for raw_s in sources:
        sid = str(raw_s).strip().lower()
        if sid not in PROVIDER_REGISTRY:
            raise SetupConfigError(f"Unknown provider {sid!r} in configuration.")
        spec = PROVIDER_REGISTRY[sid]
        if spec["implementation_status"] != "IMPLEMENTED":
            raise SetupConfigError(
                f"Provider {sid!r} is documented-only ({spec['official_docs_url']}) and cannot be enabled for execution."
            )
        if sid == "google-scholar-assisted":
            raise SetupConfigError("google-scholar-assisted is manual-assisted only and cannot be enabled as an automated discovery source.")
        if sid not in clean_sources:
            clean_sources.append(sid)

    offline_only = bool(cfg.get("offline_only", profile == "offline"))
    if profile == "offline":
        offline_only = True
    if offline_only and any(s != "fixture" for s in clean_sources):
        raise SetupConfigError("Offline profile/mode allows only ['fixture'] in 'sources'.")

    assistant = str(cfg.get("assistant") or ACCESS_PROFILES[profile]["recommended_assistant"]).strip().lower()
    if assistant not in ASSISTANT_OPTIONS:
        raise SetupConfigError(f"Unsupported assistant {assistant!r}. Choose from: {', '.join(ASSISTANT_OPTIONS)}.")

    max_http = cfg.get("default_max_http_requests", ACCESS_PROFILES[profile]["default_max_http_requests"])
    if not isinstance(max_http, int) or isinstance(max_http, bool) or max_http < 0:
        raise SetupConfigError("'default_max_http_requests' must be an integer >= 0.")

    # Paid authorization is NEVER persisted as auto-approved in config.
    return {
        "schema_version": CONFIG_SCHEMA_VERSION,
        "profile": profile,
        "sources": clean_sources,
        "assistant": assistant,
        "default_max_http_requests": 0 if offline_only else max_http,
        "offline_only": offline_only,
        "allow_paid_operations": False,
        "notes": {
            "auth_policy": "Secrets are never stored here; manage API keys via environment variables.",
            "paid_policy": "Paid provider calls always require explicit --allow-paid-api per run.",
        },
    }


def load_user_config(path: pathlib.Path | None = None) -> dict[str, Any] | None:
    cfg_path = path or get_user_config_path()
    if not cfg_path.is_file():
        return None
    try:
        raw = json.loads(cfg_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise SetupConfigError(f"Failed to read user config {cfg_path}: {exc}") from exc
    return validate_user_config(raw)


def save_user_config(cfg: dict[str, Any], path: pathlib.Path | None = None, *, force: bool = False) -> pathlib.Path:
    validated = validate_user_config(cfg)
    cfg_path = path or get_user_config_path()
    if cfg_path.exists() and not force:
        raise FileExistsError(f"Configuration already exists at {cfg_path}. Re-run with --force to overwrite.")
    cfg_path.parent.mkdir(parents=True, exist_ok=True)
    cfg_path.write_text(json.dumps(validated, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    return cfg_path


def resolve_effective_discovery_settings(
    *,
    cli_sources: str | None = None,
    cli_max_http: int | None = None,
    is_fixture: bool = False,
    config_path: pathlib.Path | None = None,
) -> dict[str, Any]:
    """Resolve effective settings with strict precedence: CLI > user config > built-in defaults."""
    cfg = load_user_config(config_path)
    offline_only = bool(cfg.get("offline_only")) if cfg else False

    if cli_sources is not None:
        sources = [s.strip().lower() for s in cli_sources.split(",") if s.strip()]
        sources_origin = "cli"
    elif cfg and cfg.get("sources"):
        sources = list(cfg["sources"])
        sources_origin = "user_config"
    else:
        sources = ["openalex"]
        sources_origin = "default"

    if cli_max_http is not None:
        max_http = cli_max_http
        max_http_origin = "cli"
    elif cfg and not is_fixture:
        max_http = int(cfg.get("default_max_http_requests", DEFAULT_SAFE_MAX_HTTP_REQUESTS))
        max_http_origin = "user_config"
    else:
        max_http = DEFAULT_SAFE_MAX_HTTP_REQUESTS if not is_fixture else None
        max_http_origin = "default"

    return {
        "profile": cfg["profile"] if cfg else "default",
        "offline_only": offline_only,
        "sources": sources or ["openalex"],
        "sources_origin": sources_origin,
        "max_http_requests": max_http,
        "max_http_origin": max_http_origin,
        "assistant": cfg["assistant"] if cfg else "none",
    }


def format_provider_checklist(profile: str | None = None) -> list[dict[str, Any]]:
    """Build structured provider checklist distinguishing implementation, auth, license, and qualification."""
    rows = list_provider_capabilities(profile=profile)
    checklist: list[dict[str, Any]] = []
    for r in rows:
        impl = r["implementation_status"] == "IMPLEMENTED"
        auth_req = bool(r["auth_required"])
        lic_req = bool(r["license_required"])
        modes = r["access_modes"]
        manual_only = "MANUAL_ASSISTED" in modes and impl
        available = impl and not manual_only and (not auth_req or r["configured"])
        checklist.append({
            "provider_id": r["provider_id"],
            "display_name": r["display_name"],
            "implemented": impl,
            "documented_only": not impl,
            "available": available,
            "configured": bool(r["configured"]),
            "auth_required": auth_req,
            "auth_status": "UNVERIFIED (env presence only)" if auth_req else "NOT_REQUIRED",
            "license_required": lic_req,
            "entitlement_status": "UNVERIFIED" if lic_req else "NOT_REQUIRED",
            "qualification_status": r["qualification_status"],
            "manual_assisted": manual_only,
            "unavailable_reason": (
                ""
                if available
                else ("manual_assisted_only" if manual_only else ("documented_only_not_implemented" if not impl else "missing_env_credentials"))
            ),
            "env_vars": r["auth_env_vars"],
            "access_modes": modes,
        })
    return checklist


def cmd_setup(argv: list[str] | None = None) -> int:
    """CLI handler for `tako setup` (interactive or non-interactive profile configuration)."""
    import argparse

    parser = argparse.ArgumentParser(prog="tako setup", description="Configure TAKO research access profile and non-secret preferences")
    parser.add_argument("--profile", choices=list(ACCESS_PROFILES.keys()), default=None, help="Research access profile (recommended: free)")
    parser.add_argument("--sources", default=None, help="Comma-separated implemented discovery sources")
    parser.add_argument("--assistant", choices=list(ASSISTANT_OPTIONS.keys()), default=None, help="Preferred AI CLI assistant (agy, codex, claude, copilot, other, none)")
    parser.add_argument("--max-http-requests", type=int, default=None, help="Default observable HTTP budget per live discovery run")
    parser.add_argument("--non-interactive", action="store_true", help="Apply options/defaults without interactive prompts")
    parser.add_argument("--show", action="store_true", help="Show active configuration and precedence without modifying files")
    parser.add_argument("--force", action="store_true", help="Overwrite existing configuration file")
    parser.add_argument("--json", action="store_true", help="Emit machine-readable JSON output")
    args = parser.parse_args(argv)

    cfg_path = get_user_config_path()
    if args.show:
        existing = load_user_config(cfg_path)
        effective = resolve_effective_discovery_settings(config_path=cfg_path)
        payload = {"config_path": str(cfg_path), "configured": existing is not None, "config": existing, "effective_defaults": effective}
        if args.json:
            print(json.dumps(payload, indent=2, ensure_ascii=False))
        else:
            print(f"TAKO Setup Configuration ({cfg_path})")
            if existing is None:
                print("Status: NOT_CONFIGURED (using built-in defaults)")
            else:
                print(f"Profile:   {existing['profile']}")
                print(f"Sources:   {', '.join(existing['sources'])}")
                print(f"Assistant: {existing['assistant']} ({ASSISTANT_OPTIONS[existing['assistant']]['mode']})")
                print(f"Offline:   {existing['offline_only']}")
                print(f"Max HTTP:  {existing['default_max_http_requests']}")
        return 0

    if args.non_interactive:
        profile = args.profile or "free"
        prof_spec = ACCESS_PROFILES[profile]
        sources = (
            [s.strip().lower() for s in args.sources.split(",") if s.strip()]
            if args.sources is not None
            else list(prof_spec["default_sources"])
        )
        assistant = args.assistant or prof_spec["recommended_assistant"]
        max_http = args.max_http_requests if args.max_http_requests is not None else prof_spec["default_max_http_requests"]
    else:
        print("TAKO Research Setup")
        prof_map = {"1": "free", "2": "hybrid", "3": "institutional", "4": "paid", "5": "offline"}
        if args.profile is not None:
            profile = args.profile
            print(f"Profile: {profile} ({ACCESS_PROFILES[profile]['label']})")
        else:
            print("Profiles: 1=free (Recommended), 2=hybrid, 3=institutional, 4=paid, 5=offline")
            raw_p = (input("Select profile [1/free]: ").strip().lower() or "free")
            profile = prof_map.get(raw_p, raw_p)
        if profile not in ACCESS_PROFILES:
            print(f"SETUP_ERROR: Unsupported profile {profile!r}", file=sys.stderr)
            return 2
        prof_spec = ACCESS_PROFILES[profile]
        default_src_str = ",".join(prof_spec["default_sources"])
        if args.sources is not None:
            sources = [s.strip().lower() for s in args.sources.split(",") if s.strip()]
        elif profile == "offline":
            sources = ["fixture"]
        else:
            raw_s = input(f"Scientific sources [{default_src_str}] (Enter for default): ").strip() or default_src_str
            sources = [s.strip().lower() for s in raw_s.split(",") if s.strip()]
        assistant_default = args.assistant or prof_spec["recommended_assistant"]
        if args.assistant is not None:
            assistant = args.assistant
        else:
            from tools.ai_cli_providers import detect_ai_cli_providers

            detected_keys = [p["assistant_key"] for p in detect_ai_cli_providers() if p["executable_detected"] and p["adapter_implemented"]]
            det_hint = f"detected: {','.join(detected_keys)}" if detected_keys else "none detected"
            assist_map = {"1": "agy", "2": "codex", "3": "claude", "4": "copilot", "5": "other", "6": "none"}
            raw_a = (
                input(f"Preferred AI CLI (1=agy, 2=codex, 3=claude, 4=copilot, 5=other, 6=none; {det_hint}) [{assistant_default}]: ")
                .strip()
                .lower()
                or assistant_default
            )
            assistant = assist_map.get(raw_a, raw_a)
        max_http = args.max_http_requests if args.max_http_requests is not None else prof_spec["default_max_http_requests"]
        if cfg_path.exists() and not args.force:
            ans = input(f"Config already exists at {cfg_path}. Overwrite? [y/N]: ").strip().lower()
            if ans not in ("y", "yes"):
                print("SETUP_CANCELLED: Existing configuration preserved.")
                return 0
            args.force = True

    cfg = {
        "profile": profile,
        "sources": sources,
        "assistant": assistant,
        "default_max_http_requests": max_http,
        "offline_only": profile == "offline",
    }
    try:
        saved_path = save_user_config(cfg, cfg_path, force=args.force)
    except (SetupConfigError, FileExistsError) as exc:
        print(f"SETUP_ERROR: {exc}", file=sys.stderr)
        return 2

    saved_cfg = load_user_config(saved_path)
    out_obj = {
        "status": "SETUP_COMPLETE",
        "config_path": str(saved_path),
        "config": saved_cfg,
        "assistant_guidance": ASSISTANT_OPTIONS[assistant]["guidance"],
    }
    if args.json:
        print(json.dumps(out_obj, indent=2, ensure_ascii=False))
    else:
        print(f"SETUP_COMPLETE path={saved_path} profile={profile} sources={','.join(saved_cfg['sources'])} assistant={assistant}")
        print(f"Assistant Guidance: {ASSISTANT_OPTIONS[assistant]['guidance']}")
    return 0


def cmd_providers(argv: list[str] | None = None) -> int:
    """CLI handler for `tako providers list`."""
    import argparse

    parser = argparse.ArgumentParser(prog="tako providers", description="Inspect scientific discovery provider capabilities")
    sub = parser.add_subparsers(dest="subcommand", required=True)
    p_list = sub.add_parser("list", help="List provider capabilities, auth requirements, and qualification status")
    p_list.add_argument("--profile", choices=list(ACCESS_PROFILES.keys()), default=None, help="Filter by access profile")
    p_list.add_argument("--json", action="store_true", help="Emit machine-readable JSON checklist")
    args = parser.parse_args(argv)

    if args.subcommand == "list":
        rows = format_provider_checklist(profile=args.profile)
        if args.json:
            print(json.dumps({"providers": rows}, indent=2, ensure_ascii=False))
            return 0
        print("TAKO Scientific Provider Checklist (no secrets shown):")
        for r in rows:
            state = "IMPLEMENTED" if r["implemented"] else "DOCUMENTED_ONLY"
            avail = "AVAILABLE" if r["available"] else f"UNAVAILABLE({r['unavailable_reason']})"
            env_note = ",".join(r["env_vars"]) if r["env_vars"] else "none"
            print(
                f"  - {r['provider_id']}: {state} | {avail} | QUAL={r['qualification_status']} | "
                f"AUTH_REQ={r['auth_required']} | LIC_REQ={r['license_required']} | ENV={env_note}"
            )
        return 0
    return 2

