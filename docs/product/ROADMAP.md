# Roadmap

Status as of TAKO-001B. Dates are intentionally absent; order and gates are what matter.

## Done (documentation only)

| Mission | Result |
| --- | --- |
| TAKO-001A | Repository foundation, product docs, boundary, workflow |
| TAKO-001C | Apache-2.0 LICENSE/NOTICE, Contributor Covenant, contribution policy |
| TAKO-002 | AIEF audit: architecture GO, code reuse blocked pending ownership/licensing clearance |
| TAKO-001B | Stack, contracts, workflow, UX, companion, design system, roadmap |
| TAKO-003 | Headless engine implemented and tested with a fake agent (branch `mission/tako-003-engine-mvp`) |
| TAKO-004 | Bridge + UI + companion built and tested against the real engine (branch `mission/tako-004-desktop-mvp`) |
| TAKO-004B | Tauri shell compiled and launched on Linux; real-window smoke test; visual review and fixes (branch `mission/tako-004b-desktop-qualification`) |
| TAKO-003H | Hardening: cancellation, locking, recovery, Git-execution safety, evidence integrity, isolation decision (branch `mission/tako-003h-hardening`) |

## Upcoming

| Mission | Goal | Depends on |
| --- | --- | --- |
| TAKO-004C (optional) | UI polish backlog (P2 list), macOS/Windows render check, tauri-cli packaging | 004B |
| TAKO-005 | L1 isolation (Linux bubblewrap first): self-contained clone workspace, adversarial test suite; prerequisite for any AI provider | 003H; [decision](../architecture/ISOLATION_DECISION.md); [readiness](../missions/TAKO-005/READINESS.md) |
| TAKO-006 | First real provider (Claude Code CLI adapter) and Tako configuration, only on L1 | 005 |
| TAKO-007 | Companion behaviors wired to real provider output | 004 |
| AIEF-IMPORT (conditional) | Import cleared AIEF modules as a snapshot | owner clearance (TAKO-002 §Owner decisions) |

## AIEF position

AIEF **ideas** inform the engine (supervised execution, verification gate, JSON evidence). AIEF **code** is not imported: it has no LICENSE and its own docs mark it private pending IP review. TAKO-003 is clean-room from the contracts in this repo and does not read AIEF source during implementation. If the owner clears AIEF, an import mission may replace or compare against TAKO-003's equivalents; no work in TAKO-003 depends on that.

## Open owner decisions (carried)

CoC reporting contact (publication blocker); copyright holder confirmation; AIEF clearance; accepting this stack recommendation; creating a remote repo.

## Deliberately later

Cloud/data integrations, multi-agent orchestration, team features, marketplace, T-Ecosystem interop (never required).
