# First Engineering Workflow

Open Project → Describe Goal → Review Plan → Approve → Execute → Verify → Accept/Reject

## State machine (Mission)

| State | Entered when | Observable | Exits to |
| --- | --- | --- | --- |
| `project_open` | Project opened | branch, HEAD, dirty flag shown | `goal_draft` |
| `goal_draft` | Goal and acceptance criteria typed | criteria required to proceed | `planning` |
| `planning` | User requests a plan; agent runs read-only | spinner + streamed summary | `plan_ready`, `blocked` |
| `plan_ready` | Plan v(n) produced | plan, expected files, requested grants, risks | `approved` (human), `planning` (revise), `cancelled` |
| `approved` | Human approves plan hash + grants | recorded approval | `executing` |
| `executing` | Workspace created, agent started | live event log, elapsed time, limits | `sealed`, `failed`, `timeout`, `cancelled` |
| `sealed` | Execution ended; workspace snapshot committed to `tako/<id>` branch, tree hash recorded | diff available | `verifying` |
| `verifying` | Checks run on a clean checkout of the sealed commit | per-check progress | `evidence_ready`, `failed` |
| `evidence_ready` | Evidence stored | diff, checks, limitations | `accepted`, `rejected` |
| `accepted` / `rejected` | Human decision | recorded decision | terminal |

`blocked`, `failed`, `timeout`, `cancelled` are visible states with reason; each allows retry from `planning` or `approved` as appropriate. Every transition is an event.

## Authorization boundaries

1. **Plan approval** (human): authorizes execution and the listed grants only. Changing the plan voids approval.
2. **Grants**: default none. Network, extra commands or env access must be listed in the plan and approved.
3. **Accept** (human): the only action that changes the user's project (see below).

## Isolation

Execution runs in a Git worktree created outside the project at `base_commit`, on branch `tako/<mission-id>`. MVP level `L0_worktree` limits accidental damage to the user's tree but **is not a security boundary**: the agent process runs as the user. The UI must label it so. Stronger isolation (bubblewrap/container; both present on the maintainer machine but unevaluated) is a later slice.

Child environment is an allowlist (PATH, HOME-like basics, and explicitly granted variables), never the full parent environment.

## Verification without modifying the subject

1. Seal: commit all workspace changes to the mission branch; record tree hash T.
2. Verify in a separate clean checkout of that commit (or read-only bind where available).
3. After checks, recompute the tree hash of the verification checkout and of the sealed commit. If either differs from T (including new untracked files that checks created and that are not ignored), mark `verification_modified_subject = true` and evidence **invalid**.
4. Build outputs and caches stay in the verification checkout, which is discarded.

## Accept / reject

- **Accept**: merge or cherry-pick the sealed branch into the user's chosen branch **only if** the project HEAD still equals `base_commit` or the merge is clean, and the working tree is clean for touched paths; otherwise the UI stops and explains. Never force. User sees the exact Git operation first.
- **Reject**: discard workspace and branch (after confirmation); evidence and decision retained.

## Failure cases

| Case | Behavior |
| --- | --- |
| Project dirty at open | Allowed; warn that Accept needs a clean target; mission based on committed HEAD only |
| Agent times out / crashes | Process group killed; partial work sealed as-is and marked `timeout`/`failed`; user may inspect, retry, or discard |
| Agent writes outside workspace | Not prevented at L0; detected only by best-effort check (project tree hash change) and flagged. Limitation recorded |
| Verification command fails | Evidence stored with `result=fail`, `acceptable=false`; Accept is refused (TAKO-003 is stricter than the original draft: no override in the first slice; reject remains possible) |
| Verification modifies subject | Evidence invalid; accept disabled until re-verified |
| Project HEAD moved | Accept blocked or requires a clean merge; user decides |
| Provider unavailable / unauthenticated | `blocked` with provider message; nothing executed |
| App closes mid-run | On restart, orphaned runs shown as `interrupted`; process groups cleaned up |
