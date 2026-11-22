"""Unified experimental TAKO CLI exposing SOTA discovery, Research Mate AI workflows, and doctor checks."""

from __future__ import annotations

import argparse
import difflib
import sys

from tools.doctor import run_doctor
from tools.install_lifecycle import cmd_uninstall, cmd_update
from tools.research_workspace import cmd_evidence, cmd_export, cmd_init, cmd_status
from tools.researcher_setup import cmd_providers, cmd_setup
from tools.sota_ai_native.workflow import main as sota_ai_main
from tools.sota_discovery.sota import main as sota_main

__version__ = "0.1.0a2"

_TOP_FLAGS = ("--version", "--help", "-V", "-h")
_SUBCOMMANDS = (
    "doctor",
    "setup",
    "providers",
    "init",
    "status",
    "sota",
    "sota-ai",
    "evidence",
    "export",
    "update",
    "uninstall",
)


def main(argv: list[str] | None = None) -> int:
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            try:
                stream.reconfigure(encoding="utf-8", errors="replace")
            except Exception:
                pass
    argv = list(sys.argv[1:] if argv is None else argv)

    # Reject unknown top-level flags (e.g. `tako --verison`) with deterministic typo suggestions
    if argv and argv[0].startswith("-") and argv[0] not in _TOP_FLAGS:
        bad = argv[0]
        matches = difflib.get_close_matches(bad, _TOP_FLAGS, n=1, cutoff=0.6)
        hint = f" Did you mean '{matches[0]}'?" if matches else " Run 'tako --help' for usage."
        print(f"tako: error: unrecognized argument '{bad}'.{hint}", file=sys.stderr)
        return 2

    parser = argparse.ArgumentParser(
        prog="tako",
        description="TAKO Experimental Researcher CLI (ARDE)",
        epilog=(
            "Examples:\n"
            "  tako setup --profile free\n"
            "  tako doctor\n"
            "  tako sota --topic 'Edge AI' --objective 'Survey latency' --iterative\n"
            "  tako sota-ai --topic 'Edge AI' --objective 'Survey latency' --provider claude-cli --allow-cli-session"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "-V", "--version",
        action="version",
        version=f"tako {__version__} (experimental researcher preview)",
    )
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("doctor", help="Concise environment & provider readiness (--verbose for full details)", add_help=False)
    sub.add_parser("setup", help="Configure research access profile, sources, and preferred AI CLI", add_help=False)
    sub.add_parser("providers", help="Inspect scientific discovery provider capabilities", add_help=False)
    sub.add_parser("init", help="Initialize a portable research workspace (tako_workspace.json)", add_help=False)
    sub.add_parser("status", help="Inspect research workspace stage status and quality verdict", add_help=False)
    sub.add_parser("sota", help="Multi-source SOTA discovery (OpenAlex, arXiv, Crossref, Semantic Scholar)", add_help=False)
    sub.add_parser("sota-ai", help="Research Mate / AI-native SOTA workflow (--allow-cli-session or --allow-paid-api)", add_help=False)
    sub.add_parser("evidence", help="Evidence ingestion, correspondence, comparative SOTA, and Tier 1 gating", add_help=False)
    sub.add_parser("export", help="Export portable scientific artifacts (Markdown, CSV, JSON, BibTeX, RIS)", add_help=False)
    sub.add_parser("update", help="Update a TAKO-managed Linux CLI installation safely", add_help=False)
    sub.add_parser("uninstall", help="Uninstall TAKO-managed CLI files (preserves workspaces and config)", add_help=False)

    if argv and not argv[0].startswith("-") and argv[0] not in _SUBCOMMANDS:
        bad_cmd = argv[0]
        matches = difflib.get_close_matches(bad_cmd, _SUBCOMMANDS, n=1, cutoff=0.6)
        hint = f" Did you mean '{matches[0]}'?" if matches else " Run 'tako --help' for available commands."
        print(f"tako: error: unknown command '{bad_cmd}'.{hint}", file=sys.stderr)
        return 2

    ns, rest = parser.parse_known_args(argv)
    if ns.command == "doctor":
        return run_doctor(rest)
    if ns.command == "setup":
        return cmd_setup(rest)
    if ns.command == "providers":
        return cmd_providers(rest)
    if ns.command == "init":
        return cmd_init(rest)
    if ns.command == "status":
        return cmd_status(rest)
    if ns.command == "sota":
        return sota_main(rest)
    if ns.command == "sota-ai":
        return sota_ai_main(rest)
    if ns.command == "evidence":
        return cmd_evidence(rest)
    if ns.command == "export":
        return cmd_export(rest)
    if ns.command == "update":
        return cmd_update(rest)
    if ns.command == "uninstall":
        return cmd_uninstall(rest)
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
