"""Minimal versioned workflow event contract and NO_COLOR-aware terminal presenter (M029B).

Shared by the TAKO CLI and future Rust/Tauri ARDE Desktop bridge.
"""

from __future__ import annotations

import json
import os
import pathlib
import sys
import time
import uuid
from datetime import datetime, timezone
from typing import Any

EVENT_SCHEMA_VERSION = "tako-workflow-event/v1"

EVENT_TYPES = {
    "workflow_started",
    "stage_started",
    "stage_progress",
    "stage_completed",
    "warning",
    "stage_failed",
    "workflow_completed",
    "workflow_interrupted",
}


def utc_now_iso() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def make_run_id(workflow: str) -> str:
    ts = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    return f"{workflow}-{ts}-{uuid.uuid4().hex[:6]}"


def _use_color(stream: Any) -> bool:
    if os.environ.get("NO_COLOR", "") != "":
        return False
    return bool(hasattr(stream, "isatty") and stream.isatty())


def sanitize_error(exc: BaseException | str) -> str:
    text = str(exc).strip().splitlines()[0] if str(exc).strip() else type(exc).__name__
    for key_name in ("OPENAI_API_KEY", "ANTHROPIC_API_KEY", "SEMANTIC_SCHOLAR_API_KEY", "GH_TOKEN", "GITHUB_TOKEN"):
        val = os.environ.get(key_name, "").strip()
        if val and val in text:
            text = text.replace(val, "[REDACTED]")
    return text[:240]


class WorkflowEventEmitter:
    """Emits structured workflow events to optional JSONL and accessible CLI output."""

    def __init__(
        self,
        *,
        workflow: str,
        workspace_id: str | None = None,
        run_id: str | None = None,
        events_file: pathlib.Path | None = None,
        json_mode: bool = False,
        quiet_progress: bool = False,
    ) -> None:
        self.workflow = workflow
        self.workspace_id = workspace_id or "standalone"
        self.run_id = run_id or make_run_id(workflow)
        self.events_file = events_file
        self.json_mode = json_mode
        self.quiet_progress = quiet_progress
        self._t0 = time.monotonic()
        self.events: list[dict[str, Any]] = []
        if self.events_file is not None:
            self.events_file.parent.mkdir(parents=True, exist_ok=True)

    def elapsed(self) -> float:
        return round(time.monotonic() - self._t0, 3)

    def emit(
        self,
        event_type: str,
        *,
        stage: str = "",
        status: str = "",
        message: str = "",
        counters: dict[str, Any] | None = None,
        artifacts: list[str] | None = None,
        error: str | None = None,
    ) -> dict[str, Any]:
        if event_type not in EVENT_TYPES:
            raise ValueError(f"Unsupported event_type: {event_type}")
        ev: dict[str, Any] = {
            "schema_version": EVENT_SCHEMA_VERSION,
            "event_type": event_type,
            "run_id": self.run_id,
            "workspace_id": self.workspace_id,
            "workflow": self.workflow,
            "stage": stage or self.workflow,
            "timestamp": utc_now_iso(),
            "elapsed_seconds": self.elapsed(),
            "status": status,
            "counters": counters or {},
            "artifacts": list(artifacts or []),
            "error": error,
        }
        if message:
            ev["message"] = message
        self.events.append(ev)
        if self.events_file is not None:
            with self.events_file.open("a", encoding="utf-8") as fh:
                fh.write(json.dumps(ev, ensure_ascii=False) + "\n")
        if not self.json_mode and not self.quiet_progress and message:
            self._print_progress(event_type, ev, message)
        return ev

    def _print_progress(self, event_type: str, ev: dict[str, Any], message: str) -> None:
        stream = sys.stdout
        prefix = f"[{self.workflow} | {ev['elapsed_seconds']:.2f}s]"
        if _use_color(stream):
            if event_type in ("stage_failed", "workflow_interrupted"):
                prefix = f"\033[31m{prefix}\033[0m"
            elif event_type == "warning":
                prefix = f"\033[33m{prefix}\033[0m"
            else:
                prefix = f"\033[35m{prefix}\033[0m"
        print(f"{prefix} {message}", file=stream, flush=True)
