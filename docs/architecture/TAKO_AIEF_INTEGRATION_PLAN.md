# TAKO ↔ AIEF Integration Plan

Status: proposal. Nothing here is authorized for implementation.

## Relationship

AIEF is the **seed of the TAKO Engineering Engine**, not a runtime dependency. The recommended relationship is **absorb after clearance, not vendor**: once the owner clears AIEF, selected modules are brought into TAKO history-free (clean snapshot) and AIEF becomes archived or a thin upstream. Reasons: AIEF is 908 lines of stdlib Python with one maintainer, it has no LICENSE, no remote and no release; an external dependency would add coupling for very little code.

Rejected alternatives: (a) pip/git dependency on a private repo violates "TAKO must not require private services"; (b) git submodule/subtree of full history inherits unaudited branches; (c) ignore AIEF and rewrite — viable fallback, costs ~900 lines, loses 39 tests as acceptance material.

## Preconditions (all must hold before any code moves)

1. Owner confirms sole authorship and absence of employer/third-party claims. Git history shows a single author across all branches, which is consistent with, but not proof of, sole ownership.
2. AIEF gets a real Apache-2.0 `LICENSE` + copyright notice (its `PROPOSED_LICENSE.md` is not the official text and says a "copyright assignment review" is pending).
3. The clean-room claim relative to private T is confirmed by the owner: AIEF's `IP_BOUNDARY.md` asserts it, but the same person owns both, so it is a policy statement, not independent evidence.
4. Full-history secret scan across all branches done (a grep of all-branch diffs for common secret patterns found none; this is not a full scan) — or import a clean snapshot instead of history.
5. Unreviewed branches triaged.

## Smallest viable integration path

1. **Clearance** (owner action, no code): satisfy the preconditions; record in `docs/decisions/0002-aief-consolidation.md`.
2. **Snapshot import** (a later mission): copy `runner.py`, `report.py`, `task.py` and their tests into TAKO, preserving authorship notices, with absolute paths removed. Keep zero runtime dependencies.
3. **Harden to TAKO's bar** before use: environment allowlist for child processes; workspace isolation (Git worktree first); no secrets passed to agents by default.
4. **Extend minimally** toward the MVP slice: plan-approval gate, diff capture in the report, accept/reject recorded.
5. **Reimplement** adapter and CLI against the contract defined in TAKO-001B.

Acceptance for step 2: imported tests pass; `scripts/check.sh` passes; no private-boundary terms; LICENSE/NOTICE correct.

## If clearance is not obtained

Fall back to reimplementing the same small design from the public concepts (supervised subprocess, verify gate, JSON report) with no AIEF code. Cost is low.

## Out of scope here

Desktop stack (TAKO-001B), provider contract details, sandbox technology.
