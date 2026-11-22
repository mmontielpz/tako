"""Unified experimental TAKO CLI exposing SOTA discovery and doctor checks."""

from __future__ import annotations

import argparse
import sys

from tools.doctor import run_doctor
from tools.sota_ai_native.workflow import main as sota_ai_main
from tools.sota_discovery.sota import main as sota_main

__version__ = "0.0.1.dev2"


def main(argv: list[str] | None = None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    parser = argparse.ArgumentParser(
        prog="tako",
        description="TAKO Experimental Researcher Preview CLI (SOTA Discovery)",
    )
    parser.add_argument(
        "-V", "--version",
        action="version",
        version=f"tako {__version__} (experimental researcher preview)",
    )
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("doctor", help="Inspect Python, OS, executables, and provider readiness (no secrets shown)")
    sub.add_parser("sota", help="Deterministic OpenAlex SOTA discovery (no API key required)", add_help=False)
    sub.add_parser("sota-ai", help="AI-native SOTA workflow (Researcher -> Analyst -> optional Reviewer)", add_help=False)

    ns, rest = parser.parse_known_args(argv)
    if ns.command == "doctor":
        return run_doctor()
    if ns.command == "sota":
        return sota_main(rest)
    if ns.command == "sota-ai":
        return sota_ai_main(rest)
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
