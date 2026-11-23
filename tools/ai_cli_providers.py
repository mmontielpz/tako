"""CLI-first AI provider registry, capability detection, and authorization boundary (M029A).

Designed as a shared layer usable by both the TAKO CLI and future Research Mate
Desktop bridge without duplicating provider or authorization logic.
"""

from __future__ import annotations

from dataclasses import dataclass, field
import os
import shutil
from typing import Any


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
        "sandbox_flags": [],
        "non_interactive_flags": ["-p", "--output-format", "json"],
        "os_confinement": "prompt_only_unconfined_fs",
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
        "live_inference_status": "NOT_TESTED",
        "notes": "GitHub Copilot CLI (`copilot -p ... --output-format json`). May consume premium requests.",
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
        return True, "subscription_cli"
    return False, f"Unsupported AI provider: {provider}"


def build_cli_invocation(provider: str, exe: str, prompt: str) -> list[str]:
    """Build non-interactive CLI invocation using verified flags for installed CLIs."""
    if provider == "agy-cli":
        return [exe, "-p", prompt, "--output-format", "json"]
    if provider == "codex-cli":
        return [exe, "exec", prompt]
    if provider == "claude-cli":
        return [exe, "-p", prompt, "--output-format", "json"]
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
