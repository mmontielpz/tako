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
| TAKO-005 | L1 isolation (Linux bubblewrap first): self-contained clone workspace, adversarial test suite; prerequisite for any AI provider | 003H; [decision](../architecture/ISOLATION_DECISION.md); readiness (`docs/missions/TAKO-005/READINESS.md`, internal archive) |
| TAKO-006 | First real provider (Claude Code CLI adapter) and Tako configuration, only on L1 | 005 |
| TAKO-007 | Companion behaviors wired to real provider output | 004 |
| AIEF-IMPORT (conditional) | Import cleared AIEF modules as a snapshot | owner clearance (TAKO-002 §Owner decisions) |

## AIEF position

AIEF **ideas** inform the engine (supervised execution, verification gate, JSON evidence). AIEF **code** is not imported: it has no LICENSE and its own docs mark it private pending IP review. TAKO-003 is clean-room from the contracts in this repo and does not read AIEF source during implementation. If the owner clears AIEF, an import mission may replace or compare against TAKO-003's equivalents; no work in TAKO-003 depends on that.

## Open owner decisions (carried)

CoC reporting contact (publication blocker); copyright holder confirmation; AIEF clearance; accepting this stack recommendation; creating a remote repo.

## Deliberately later

Cloud/data integrations, multi-agent orchestration, team features, marketplace, T-Ecosystem interop (never required).

---

## Canonical ARDE & CLI-First Roadmap (Reconciled Baseline)

- **Product Direction:** CLI-first (`tools/` Python CLI).
- **Proposed Research Positioning:** Agentic Research Development Environment (`ARDE`).
- **Methodology Baseline:** `M017A v0.2-draft` (`METHODOLOGY_APPROVAL=PENDING` — awaiting formal owner approval).
- **Architecture Direction:** Python scientific core and CLI (`tools/`) with a future versioned JSON boundary to Rust/Tauri (`src/`, `bridge/`, `ui/`). Do not implement the application boundary prematurely.
- **Platform Status:** `WINDOWS_RUNTIME_STATUS=NOT_TESTED`. Native Windows CLI qualification belongs to `P4` (CLI Product Readiness); native Desktop qualification belongs to `P7` (Desktop App).

### Canonical Phases

| Phase | Name | Scope & Notes |
|---|---|---|
| `P0` | Engineering Foundation | Public repo hygiene (`ENG-01`), OSS readiness (`ENG-02`), deterministic Python CLI CI, roadmap reconciliation |
| `P1` | Scientific Integrity / Increment A / `dev4` | Resolve canonical `v0.0.1.dev3` scientific defects (`F01`–`F06`) before `0.0.1.dev4` |
| `P2` | Discovery Intelligence / Increment B | Hybrid discovery, query expansion, snowballing, relevance improvements (`F07`) |
| `P3` | Evidence and Comparative SOTA / Increment C | Auditable screening, structured extraction, comparative synthesis & gap analysis |
| `P4` | CLI Product Readiness and Interoperability | Polished CLI workflows, export formats, native Windows CLI qualification |
| `P5` | Internal Scientific Benchmarks | Formal internal validation: `INTERNAL_BENCHMARK_SEQUENCE=B02,B04,B05,B06` |
| `P6` | Refinement and External Scientific Validation | External baselines & evaluation: `EXTERNAL_BENCHMARK_SEQUENCE=B01,B03` |
| `P7` | Desktop App | Connect Rust/Tauri desktop shell via versioned JSON boundary; native Desktop qualification |
| `P8` | Advanced Research Ecosystem Integrations | Deeper Zotero, Mendeley, Overleaf, Jupyter, and repository workflows |
| `PUB` | Research Evidence & Publication | Research evidence collection and eventual publication |

> **Verification & Benchmark Cadence:** Engineering tests, frozen replays, negative cases, and smoke tests run continuously in every increment. Formal `B01`–`B06` scientific benchmarks run after CLI readiness (`P5`–`P6`) and are not prerequisites for individual engineering increments.

### Preserved Canonical Scientific Finding IDs (`v0.0.1.dev3` / `M016`)

- `F01` — Wrong or unresolved DOI contamination of accepted bibliography.
- `F02` — Duplicate results/limitations extraction.
- `F03` — Misrepresented scientific evidence provenance.
- `F04` — Invalid BibTeX author separation.
- `F05` — Incorrect cost and premium-request accounting.
- `F06` — Incorrect evidence classification and traceability.
- `F07` — Discovery relevance limitations.
