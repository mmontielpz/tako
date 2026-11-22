# AGENTS.md — Canonical Agent Workflow

This is the single canonical workflow for Claude Code, Codex and any other agent working in this repository. Agent-specific files (for example `CLAUDE.md`) must reference this file, not duplicate it.

TAKO is pre-alpha. Read [CURRENT_STATE.md](CURRENT_STATE.md) first.

## Workflow for every implementation mission

1. Inspect repository state (`git status`, branch, recent log).
2. Understand the objective and constraints.
3. Define acceptance criteria before writing anything.
4. Implement the smallest coherent change.
5. Execute the relevant checks.
6. Record evidence and limitations.
7. Produce a durable checkpoint (update `CURRENT_STATE.md` and the mission report in `docs/missions/<ID>/`).
8. Report next actions.

## Rules

- Preserve uncommitted work. Never discard changes you did not make.
- Never claim tests passed without executing them; report real output.
- Avoid unrelated refactoring.
- Respect the public/private boundary in [docs/architecture/OPEN_SOURCE_BOUNDARY.md](docs/architecture/OPEN_SOURCE_BOUNDARY.md). No private T code, prompts or policies.
- Avoid uncontrolled tool execution; prefer non-destructive, reviewable commands.
- Never push, publish, deploy or delete without explicit authorization.
- Never commit secrets or credentials.
- Record blockers and unknowns instead of guessing; do not invent implementation status.
- Add dependencies only with a stated reason.

## Checks

Run `scripts/check.sh` before declaring a mission done.
