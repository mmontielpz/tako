# Core Contracts

Status: draft contracts for TAKO-003. Minimal on purpose; fields marked *(later)* are not in the first slice. Names are conceptual; wire format is JSON with an explicit `schema_version`.

## Entities

**Project** — an existing Git repository the user opened.
`id`, `path`, `default_branch`, `head` (commit at open time), `dirty` (bool). TAKO never modifies the user's working tree except on Accept.

**Workspace** — isolated place where an Execution runs.
`id`, `project_id`, `kind` (`git_worktree` for MVP), `path` (under TAKO's data dir, never inside the project), `base_commit`, `branch` (`tako/<mission-id>`), `isolation_level` (`L0_worktree`; *(later)* `L1_sandbox`), `state` (`created|active|sealed|discarded|merged`).

**Mission** — one engineering objective and its lifecycle.
`id`, `project_id`, `goal` (text), `constraints[]`, `acceptance_criteria[]` (required, user-confirmed), `verification_commands[]`, `state` (see workflow), `created_at`.

**EngineeringPlan** — proposed approach, produced by an agent, approved by the human.
`id`, `mission_id`, `version`, `summary`, `steps[]`, `files_expected[]`, `requested_permissions[]`, `risks[]`, `approved_by` (human) / `approved_at` / `approved_hash`. Any edit creates a new version and clears approval.

**Takos (specialized agents)** and **Provider** — configuration only.
`Provider`: `id`, `kind` (`cli_agent` for MVP), `command`, `args_template`, `env_allowlist[]`, `secrets_ref` (reference into OS keychain or the provider's own login; never the secret value). `Tako` (an agent definition): `id`, `name`, `provider_id`, `role_prompt` (user-authored), `default_permissions[]`. MVP ships one provider and one default Tako.

**PermissionGrant** — explicit, scoped, revocable authorization.
`id`, `mission_id`, `capability` (`fs_write_workspace`, `exec_command:<pattern>`, `network`, `read_env:<NAME>`…), `scope`, `granted_by`, `granted_at`, `expires` (end of mission by default). Default: nothing granted. Grants are shown in the plan and approved with it.

**Execution** — one supervised agent run.
`id`, `mission_id`, `plan_version`, `workspace_id`, `provider_id`, `started_at`, `ended_at`, `status` (`running|succeeded|failed|timeout|cancelled`), `limits` (`timeout_s`, *(later)* budget), `events[]` (append-only log), `output_ref`.

**VerificationEvidence** — what was observed, not claimed.
`id`, `execution_id`, `subject_tree_hash` (Git tree hash of the sealed result), `checks[]` (`command`, `exit_code`, `duration`, `stdout_ref`, `stderr_ref`), `diff_ref`, `files_changed[]`, `tree_hash_before_verify`, `tree_hash_after_verify`, `verification_modified_subject` (must be false), `limitations[]`, `environment` (OS, tool versions). Overall `result` is derived only from `checks[]`.

**AcceptanceDecision** — the human's final call.
`id`, `mission_id`, `evidence_id`, `decision` (`accept|reject`), `decided_by`, `decided_at`, `note`. Accept requires evidence whose `result` and tree hash match the current sealed workspace.

## Invariants

1. Nothing executes without an approved plan version and recorded grants.
2. Execution only writes inside its Workspace.
3. Verification runs against a sealed snapshot and must leave its tree hash unchanged; otherwise evidence is invalid.
4. Agent claims are never evidence; only `checks[]` are.
5. Accept is the only operation that touches the user's project, and it is a human act.
6. Secrets are referenced, never stored in missions, logs or evidence; output is scrubbed of allowlisted env values.
7. All state changes are recorded as events and the mission state is reconstructable from disk without the app.

## Persistence

Plain files under `<project>/.tako/` for mission records (JSON, text) is **rejected** to keep the user's repo clean; use TAKO's data dir keyed by project id, with a human-readable `mission.json` + `events.jsonl` per mission. No database in the MVP.

## Engine interface (sketch)

Commands: `open_project`, `create_mission`, `submit_plan` / `approve_plan`, `run_execution`, `cancel`, `run_verification`, `decide`. Events: state changes, agent output lines, check results. Versioned JSON; the UI is just a client.

## Implementation status (TAKO-003)

Implemented in `src/` (Rust, `tako-engine`) and exercised by `tests/engine.rs`: Project (read-only open), Mission, EngineeringPlan with content hash, PermissionGrant (as approval-bound capability strings), Execution (fake-agent tested), VerificationEvidence, AcceptanceDecision. Provider/Tako configuration objects are **not** implemented; an `AgentSpec` (program, args, env names) stands in.

Deviations from the draft above, all deliberate:

- Persistence is `<data_dir>/missions/<mission-id>/` (`mission.json`, `plan-vN.json`, `exec-N/`, `evidence-N.json`, `decision.json`, `events.jsonl`), not keyed by project id. Timestamps are Unix seconds.
- Grants are stored inside the `Approval`, not as separate objects, and are single-use per approval.
- `network` capabilities are refused at plan time (`Unenforceable`) because L0 cannot enforce them.
- Failed verification yields stored evidence with `acceptable=false`; Accept refuses it. No override yet.
- There is no distinct persisted `verifying` state and no `cancel` command yet.
- Capability grants are program-level (`exec_command:sh` permits whatever `sh` runs); they are authorization records, not a sandbox.

### Additions in TAKO-003H

- New mission states: `verifying`, `cancelled`. `Engine::cancel` requests a stop of a running execution or verification; `Engine::recover` resolves `executing`, `verifying` and crashed-start residue.
- Per-mission lock (`Busy` error); `UnsafeRepository`, `IsolationRequired`, `Cancelled` errors.
- `AgentSpec.kind` (`Scripted` | `Provider`); `Provider` is refused until an L1 sandbox exists.
- Evidence gains `integrity_violations`, `failed_checks`, `ignored_artifacts`; it is write-once/read-only, with a content id stored in the mission and checked at accept.
- Accept completes idempotently if a previous accept merged but crashed before recording the decision.
