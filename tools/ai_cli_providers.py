"""CLI-first AI provider registry, capability detection, and authorization boundary (M029A).

Designed as a shared layer usable by both the TAKO CLI and future Research Mate
Desktop bridge without duplicating provider or authorization logic.
"""

from __future__ import annotations

from dataclasses import dataclass, field
import json
import os
import re
import shutil
from typing import Any


class ProviderResponseError(RuntimeError):
    """Raised when a provider response cannot be decoded or fails contract checks."""


class BudgetExhaustedError(RuntimeError):
    """Raised when a workflow exceeds its configured provider subprocess budget."""


@dataclass
class ProviderCallBudget:
    """Shared workflow-level budget for provider subprocess invocations."""

    max_calls: int | None = None
    attempted_calls: int = 0
    completed_calls: int = 0
    failed_calls: int = 0
    stages: list[str] = field(default_factory=list)

    def __post_init__(self) -> None:
        if self.max_calls is not None and self.max_calls < 0:
            raise ValueError(f"max_calls must be >= 0, got {self.max_calls}")

    def consume(self, stage: str) -> None:
        """Reserve one provider subprocess invocation before execution."""
        if self.max_calls is not None and self.attempted_calls >= self.max_calls:
            raise BudgetExhaustedError(
                f"Provider invocation budget exhausted ({self.attempted_calls}/{self.max_calls}) before stage '{stage}'."
            )
        self.attempted_calls += 1
        self.stages.append(stage)

    def record_success(self) -> None:
        self.completed_calls += 1

    def record_failure(self) -> None:
        self.failed_calls += 1

    def summary(self) -> dict[str, Any]:
        return {
            "max_provider_calls": self.max_calls,
            "attempted_calls": self.attempted_calls,
            "completed_calls": self.completed_calls,
            "failed_calls": self.failed_calls,
            "stages": list(self.stages),
        }


PROVIDER_EXECUTION_CONTRACTS: dict[str, dict[str, Any]] = {
    "agy-cli": {
        "sandbox_flags": ["--sandbox"],
        "non_interactive_flags": ["-p", "--output-format", "json"],
        "os_confinement": "provider_sandbox_flag",
    },
    "codex-cli": {
        "sandbox_flags": ["--sandbox", "read-only", "--skip-git-repo-check"],
        "non_interactive_flags": ["exec"],
        "os_confinement": "provider_read_only_sandbox",
    },
    "claude-cli": {
        "sandbox_flags": ["--tools", ""],
        "non_interactive_flags": ["-p", "--output-format", "json"],
        "os_confinement": "tools_disabled_prompt_only",
    },
    "copilot-cli": {
        "sandbox_flags": ["--no-ask-user", "--no-custom-instructions", "--disable-builtin-mcps"],
        "non_interactive_flags": ["-p", "--output-format", "json", "--stream", "off"],
        "os_confinement": "mcp_and_prompt_restricted",
    },
}


AI_CLI_SPECS: dict[str, dict[str, Any]] = {
    "agy-cli": {
        "id": "agy-cli",
        "assistant_key": "agy",
        "executable": "agy",
        "auth_category": "subscription_cli",
        "adapter_implemented": True,
        "interactive_supported": True,
        "non_interactive_supported": True,
        "structured_output_supported": True,
        "live_inference_status": "NOT_TESTED",
        "notes": "Antigravity CLI (`agy -p ... --output-format json`). May consume subscription quota/credits.",
    },
    "codex-cli": {
        "id": "codex-cli",
        "assistant_key": "codex",
        "executable": "codex",
        "auth_category": "subscription_cli",
        "adapter_implemented": True,
        "interactive_supported": True,
        "non_interactive_supported": True,
        "structured_output_supported": False,
        "live_inference_status": "NOT_TESTED",
        "notes": "OpenAI Codex CLI (`codex exec ...`). May consume ChatGPT/Codex subscription quota.",
    },
    "claude-cli": {
        "id": "claude-cli",
        "assistant_key": "claude",
        "executable": "claude",
        "auth_category": "subscription_cli",
        "adapter_implemented": True,
        "interactive_supported": True,
        "non_interactive_supported": True,
        "structured_output_supported": True,
        "live_inference_status": "NOT_TESTED",
        "notes": "Claude Code CLI (`claude -p ... --output-format json`). May consume Claude subscription usage/credits.",
    },
    "copilot-cli": {
        "id": "copilot-cli",
        "assistant_key": "copilot",
        "executable": "copilot",
        "auth_category": "subscription_cli",
        "adapter_implemented": True,
        "interactive_supported": True,
        "non_interactive_supported": True,
        "structured_output_supported": True,
        "live_inference_status": "EXCLUDED_PARTIAL_ISOLATION",
        "notes": "GitHub Copilot CLI (`copilot -p ... --output-format json`). Excluded from v0.1.0a3 security-qualified scope (PARTIAL isolation: no OS sandbox).",
    },
    "gemini-cli": {
        "id": "gemini-cli",
        "assistant_key": "gemini",
        "executable": "gemini",
        "auth_category": "unsupported",
        "adapter_implemented": False,
        "interactive_supported": False,
        "non_interactive_supported": False,
        "structured_output_supported": False,
        "live_inference_status": "UNSUPPORTED",
        "notes": "Not integrated in TAKO.",
    },
}


def detect_ai_cli_providers() -> list[dict[str, Any]]:
    """Inspect PATH for supported AI CLI executables without inferring authentication."""
    results: list[dict[str, Any]] = []
    for pid, spec in AI_CLI_SPECS.items():
        exe_path = shutil.which(spec["executable"])
        detected = bool(exe_path)
        auth_status = (
            "UNSUPPORTED"
            if not spec["adapter_implemented"]
            else ("UNVERIFIED_SESSION_DEPENDENT" if detected else "EXECUTABLE_NOT_FOUND")
        )
        results.append(
            {
                "id": pid,
                "assistant_key": spec["assistant_key"],
                "executable": spec["executable"],
                "executable_detected": detected,
                "adapter_implemented": spec["adapter_implemented"],
                "authentication_status": auth_status,
                "interactive_supported": spec["interactive_supported"],
                "non_interactive_supported": spec["non_interactive_supported"],
                "structured_output_supported": spec["structured_output_supported"],
                "live_inference_status": spec["live_inference_status"],
                "auth_category": spec["auth_category"],
                "notes": spec["notes"],
            }
        )
    return results


def authorize_provider_execution(
    provider: str,
    *,
    allow_cli_session: bool = False,
    allow_paid_api: bool = False,
    allow_unisolated_cli: bool = False,
) -> tuple[bool, str]:
    """Enforce strict separation between local CLI sessions, metered APIs, and fixtures."""
    if provider == "fixture":
        return True, "offline_fixture"
    if provider == "openai-responses":
        if not allow_paid_api:
            return (
                False,
                "Direct metered API provider 'openai-responses' requires explicit --allow-paid-api authorization.",
            )
        return True, "metered_api_key"
    if provider in ("agy-cli", "codex-cli", "claude-cli", "copilot-cli"):
        if not (allow_cli_session or allow_paid_api):
            return (
                False,
                f"Local AI CLI '{provider}' requires explicit --allow-cli-session authorization "
                "(local CLI execution may consume subscription quotas, premium requests, or credits).",
            )
        if provider == "copilot-cli" and not allow_unisolated_cli:
            return (
                False,
                "Provider 'copilot-cli' is excluded from the v0.1.0a3 security-qualified scope "
                "(PARTIAL execution isolation: no OS sandbox). Pass --allow-unisolated-cli to override.",
            )
        return True, "subscription_cli"
    return False, f"Unsupported AI provider: {provider}"


def build_cli_invocation(provider: str, exe: str, prompt: str) -> list[str]:
    """Build non-interactive CLI invocation using verified flags for installed CLIs."""
    if provider == "agy-cli":
        return [exe, "-p", prompt, "--output-format", "json", "--sandbox"]
    if provider == "codex-cli":
        return [exe, "exec", "--sandbox", "read-only", "--skip-git-repo-check", prompt]
    if provider == "claude-cli":
        return [exe, "-p", prompt, "--output-format", "json", "--tools", ""]
    if provider == "copilot-cli":
        return [
            exe,
            "-p",
            prompt,
            "--output-format",
            "json",
            "--stream",
            "off",
            "--no-ask-user",
            "--no-custom-instructions",
            "--disable-builtin-mcps",
        ]
    raise ValueError(f"No CLI invocation defined for provider: {provider}")


_SAFE_ENV_ALLOWLIST = (
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "LANG",
    "LC_ALL",
    "TERM",
    "TMPDIR",
    "SYSTEMROOT",
    "COMSPEC",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
)

_BLOCKED_CREDENTIAL_ENV_VARS = (
    "OPENAI_API_KEY",
    "OPENAI_ORG_ID",
    "OPENAI_BASE_URL",
    "AZURE_OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "COPILOT_GITHUB_TOKEN",
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "GOOGLE_CLOUD_API_KEY",
)


def sanitized_cli_env(strip_var: str | None = None, *, allow_key_env: str | None = None) -> dict[str, str]:
    """Return minimal allowlisted environment excluding direct API keys in CLI subscription mode."""
    env = {k: os.environ[k] for k in _SAFE_ENV_ALLOWLIST if k in os.environ}
    for key in _BLOCKED_CREDENTIAL_ENV_VARS:
        if strip_var is None or key == strip_var:
            env.pop(key, None)
    if allow_key_env and allow_key_env in os.environ:
        env[allow_key_env] = os.environ[allow_key_env]
    return env


def parse_structured_json_response(raw: str, provider: str) -> dict[str, Any]:
    """Extract a single unambiguous JSON object from raw provider output."""
    text = raw.strip()
    if not text:
        raise ProviderResponseError(f"{provider} returned an empty response (0 bytes after stripping).")

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
            raise ProviderResponseError(f"{provider} returned multiple competing JSON objects ({len(found)}).")
        if len(found) == 1:
            return found[0]

    preview = text[:160].replace("\n", " ")
    raise ProviderResponseError(f"{provider} returned invalid JSON (no JSON object found; preview={preview!r}).")


def unwrap_provider_envelope(data: dict[str, Any]) -> str | None:
    """Extract assistant payload text from known CLI JSON envelopes (result, response, content, text, output)."""
    for key in ("result", "response", "text", "content", "output"):
        val = data.get(key)
        if isinstance(val, str) and val.strip():
            return val
        if isinstance(val, dict):
            return json.dumps(val, ensure_ascii=False)
    return None


def decode_copilot_jsonl(stdout: str) -> tuple[str, dict[str, Any]]:
    """Decode GitHub Copilot CLI --output-format json (JSONL) into assistant text and usage metadata."""
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
            isinstance(data, dict)
            and isinstance(data.get("content"), str)
            and ev_type not in ("user.message", "tool.execution_start", "tool.execution_complete")
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

    usage_meta = {
        "cost_usd": cost_usd,
        "premium_requests": premium_requests,
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "provider_specific_usage": provider_specific,
        "measurement_availability": "provider_reported" if (premium_requests != "UNKNOWN" or input_tokens != "UNKNOWN") else "UNKNOWN",
        "provenance": "copilot-cli-jsonl",
    }

    candidates = final_answers if final_answers else messages
    if candidates:
        for msg in reversed(candidates):
            try:
                parse_structured_json_response(msg, "copilot-cli")
                return msg, usage_meta
            except ProviderResponseError:
                continue
        return candidates[-1], usage_meta
    if parsed_any_jsonl:
        raise ProviderResponseError(
            f"copilot-cli JSONL stream contained no assistant message (events={event_types[:10]})."
        )
    return stdout, usage_meta


def _parse_codex_stderr_tokens(stderr: str) -> int | str:
    if not stderr:
        return "UNKNOWN"
    m_tok = re.search(r"tokens\s+used\s*[:\n\r]+\s*([0-9][0-9,]*)", stderr, flags=re.IGNORECASE)
    return int(m_tok.group(1).replace(",", "")) if m_tok else "UNKNOWN"


def decode_cli_provider_stdout(provider: str, stdout: str, stderr: str = "") -> tuple[str, dict[str, Any]]:
    """Decode CLI stdout into (assistant_text, raw_usage_meta) for all supported CLI providers."""
    if provider == "copilot-cli":
        if not stdout.strip():
            raise ProviderResponseError(f"copilot CLI returned empty stdout (stderr={stderr.strip()[:200]!r}).")
        return decode_copilot_jsonl(stdout)
    codex_total = _parse_codex_stderr_tokens(stderr) if provider == "codex-cli" else "UNKNOWN"
    try:
        data = json.loads(stdout)
        if isinstance(data, dict):
            unwrapped = unwrap_provider_envelope(data)
            text = unwrapped if unwrapped is not None else stdout
            u_raw = data.get("usage") if isinstance(data.get("usage"), dict) else {}
            cost = data.get("total_cost_usd", "UNKNOWN")
            has_usage = bool(u_raw or isinstance(cost, (int, float)) or isinstance(codex_total, int))
            usage_meta = {
                "cost_usd": cost if isinstance(cost, (int, float)) else "UNKNOWN",
                "premium_requests": "UNKNOWN",
                "input_tokens": u_raw.get("input_tokens", "UNKNOWN"),
                "output_tokens": u_raw.get("output_tokens", "UNKNOWN"),
                "total_tokens": codex_total,
                "provider_specific_usage": {"http_attempts": "NOT_OBSERVABLE"},
                "measurement_availability": "provider_reported" if has_usage else "UNKNOWN",
                "provenance": f"{provider}-json",
            }
            return text, usage_meta
    except json.JSONDecodeError:
        pass
    meas_avail = "provider_reported" if isinstance(codex_total, int) else "UNKNOWN"
    return stdout, {
        "cost_usd": "UNKNOWN",
        "premium_requests": "UNKNOWN",
        "input_tokens": "UNKNOWN",
        "output_tokens": "UNKNOWN",
        "total_tokens": codex_total,
        "provider_specific_usage": {"http_attempts": "NOT_OBSERVABLE"},
        "measurement_availability": meas_avail,
        "provenance": f"{provider}-raw",
    }
