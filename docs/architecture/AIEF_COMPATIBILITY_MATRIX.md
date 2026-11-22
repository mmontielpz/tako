# AIEF Compatibility Matrix

Source: read-only audit of AIEF at `~/repos/aief` (HEAD `c6c2d5e`, branch `checkpoint/p005-pause-20261029`). Method: file reading and one test run on a throwaway snapshot outside both repos.

**Global gate:** every REUSE/ADAPT below is *conditional on IP/licensing clearance of AIEF* (see REPORT §Ownership). Until cleared, treat the disposition as the intended outcome, and the practical action as REIMPLEMENT-from-spec or wait. Uncertain ownership/licensing blocks reuse.

Dispositions: REUSE (take as is) · ADAPT (take, modify) · REIMPLEMENT (take the idea, write fresh) · EXCLUDE · NEEDS_REVIEW.

## Code modules (`src/aief/`, 908 lines, stdlib only)

| Module | Capability | Observed | Disposition | Notes |
| --- | --- | --- | --- | --- |
| `runner.py` (242 lines) | Execution supervision | `subprocess.Popen`, `shell=False`, new session, timeout, process-group SIGTERM→SIGKILL, structured `RunResult`. Tested. | **ADAPT** | Sound primitive. It inherits the parent environment by default (secret leakage) and does not confine filesystem or network; TAKO needs an env allowlist and real isolation on top. |
| `adapter.py` (33 lines) | Provider/agent adapter | One CLI adapter; builds `[exe, *args, "--print", task]`. Hard-coded `--print` flag and `agy` default. | **REIMPLEMENT** | Too small and vendor-shaped to port; TAKO needs a provider-agnostic adapter contract (TAKO-001B). |
| `task.py` (205 lines) | Orchestration | Validates task/workspace, runs agent, then runs verify command, aggregates PASS/FAIL/TIMEOUT. | **ADAPT** | Agent→verify gating matches "evidence over claims". No plan/approval step, no workspace isolation. |
| `report.py` (222 lines) | Verification/evidence, resource metrics | Versioned JSON report schema, validator, rusage metrics. | **ADAPT** | Good seed for the evidence record. Schema must grow (diff, plan, approvals). Resource data is observation only, not governance. |
| `cli.py` (181 lines) | Developer workflow | `argparse` `aief run`. | **REIMPLEMENT** | TAKO's surface is the desktop app; the engine needs an API/CLI shaped by 001B. |
| `__init__.py` | Public API | Re-exports. | **EXCLUDE** | Trivial. |

## Tests and tooling

| Item | Observed | Disposition |
| --- | --- | --- |
| `tests/` (4 modules, 39 tests) | All pass on snapshot of HEAD (Python 3.14.8). | **NEEDS_REVIEW** — useful as acceptance material for ported modules, subject to clearance. |
| `scripts/verify.py` | Docs check, compile, unittest, zero-dependency check. Pass 4/4. | **REIMPLEMENT** — TAKO already has `scripts/check.sh`; the idea of one consolidated gate is shared. |

## Documents

| Item | Disposition | Notes |
| --- | --- | --- |
| `AGENTS.md` (10 engineering rules) | **REIMPLEMENT** | Overlaps TAKO `AGENTS.md`; contains a hard-coded `/workspace/...` path. Do not copy. |
| `docs/IP_BOUNDARY.md` | **NEEDS_REVIEW** | Provenance policy; states AIEF stays private until IP review completes. |
| `docs/PROPOSED_LICENSE.md` | **EXCLUDE** | Not the official Apache text (shortened, 8 sections); status "pending owner approval". |
| `docs/slices/`, `docs/handoffs/`, `docs/CHANGELOG.md`, `docs/MVP.md` | **EXCLUDE** from import | Contain absolute local paths and process history; read for lessons only. |
| `aief_backups/*.bundle` (outside repo) | **EXCLUDE** | Backup artifact, not source. |

## Capability coverage vs. TAKO needs

| TAKO need | In AIEF? | Verdict |
| --- | --- | --- |
| Agent orchestration | Minimal: one agent, one verify step | Seed only |
| Context management | Task file as prompt; no context assembly | Gap |
| Provider adapters | One CLI wrapper | Gap (REIMPLEMENT) |
| Tool / MCP integration | None | Gap |
| Verification and evals | Verify-command gate + JSON report | Partial; no evals |
| Resource and cost governance | rusage observation, timeouts; no budgets/tokens | Partial |
| Security and sandboxing | Timeouts + process groups only | Gap |
| Developer workflows | CLI + slice process | Partial |

## Unreviewed branches

Seven other branches and four worktrees exist (`main`, `candidate/readiness-staging-001`, `feat/task-rdy-002-report-loader`, `dogfood-072-p003`, `feat/task-aief-002-schema-guard`, `feat/task-aief-004-zombie-reaping`, `feat/task-aief-005-timeout-cli-guard`). Each differs from HEAD by small diffs (e.g. schema guard, zombie reaping, timeout CLI guard). **NEEDS_REVIEW**; their content was not analyzed.
