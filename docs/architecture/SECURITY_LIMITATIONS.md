# Security Limitations (engine, as of TAKO-003H)

This is a deliberately blunt list. It describes what the engine does **not** protect against. Nothing here is a vulnerability disclosure channel; see [SECURITY.md](../../SECURITY.md).

## Threat model

Trusted: the human operator and their own project. Semi-trusted: scripted fixtures written by the operator. **Untrusted: AI providers and anything they write.** Untrusted execution is disabled until L1 isolation exists ([ISOLATION_DECISION.md](ISOLATION_DECISION.md)).

## What the engine enforces today

- Approval is bound to one plan version and its content hash; grants are a subset of the plan's request; approvals are single-use; only humans can approve, accept or reject (an engine-level check; agents never receive an engine handle).
- Child processes get a cleared environment plus explicit grants. They run in their own process group with timeout and cancellation; the group is killed with escalation, the leader is reaped, leftovers are killed (leftover detection uses `/proc`, Linux only).
- Git is run with hooks, fsmonitor, signing, auto-gc, external diff and pager neutralized on the command line, ambient `GIT_*` location/helper variables removed, and external diff/textconv disabled for the diffs it runs.
- Repository-scoped config (local and worktree scope) that can make Git run a program (`filter.*.clean/smudge/process`, `merge.*.driver`, `diff.*.textconv/command`, `diff.external`, `include*`, `core.worktree`, `gpg*.program`) causes the project to be **refused**. The audit is repeated before execution, after the agent stops (before sealing), around verification, and before accept. If it fails, no further Git command that could execute helpers is run for that step. Workspace `.git` redirection is detected before sealing.
- Per-mission `flock` lock: concurrent mutations get `Busy`; recovery requires the lock, which proves no live engine holds the mission.
- Evidence is write-once and read-only; its content id is stored in the mission and re-checked at accept. Verification runs on a clean checkout of the sealed commit and is invalidated by changes to tracked or untracked non-ignored files, modes, the sealed branch or the Git configuration.

## What it does NOT protect against

1. **L0 is not a sandbox.** An agent can read and write anything the user can, use the network, signal other processes, and tamper with the project's `.git` (hooks, refs, objects, config). The audit above detects some tampering at defined checkpoints; it cannot prevent it, and it cannot see changes made and reverted between checkpoints.
2. **Verification checks are unsandboxed** and run as the user. A check can do everything an agent can. Mutation detection covers the verification checkout and the sealed workspace only.
3. **Globally configured Git helpers are trusted.** User/system Git config (for example a Git LFS filter) is not audited and is still honored when a repository's `.gitattributes` selects it. Hooks and fsmonitor are neutralized; filters configured globally are not.
4. **Other repository-controlled behavior** is not audited: submodule contents, `.gitattributes` content itself, symlink targets in the workspace, very large files, or objects that exploit Git bugs. Only open projects you trust.
5. **Tamper-evident, not tamper-proof.** Evidence and mission files are protected by permissions and a content id stored beside them. Code running as the same user can rewrite both. The check detects accidents and naive tampering.
6. **Capability grants are authorization records.** `exec_command:sh` allows whatever `sh` then runs. `network` cannot be enforced, so it is refused at plan time. `read_env` is enforced only by what the engine passes.
7. **Ignored files** (for example build output) are excluded from the tree-hash comparison; they are listed (max 50) in the evidence but do not invalidate it.
8. **Recovery kills an orphaned agent only if the recorded leader is still alive and its start time matches** (Linux `/proc`). If the leader already died but descendants remain, they are not found. On other platforms recovery marks the mission interrupted without killing anything.
9. **Platform scope.** Linux is tested. macOS has no tests (the `waitid`/`siginfo` code path is untested there). Windows is unsupported (the crate does not compile).
10. **Locking is advisory and per host.** It protects cooperating engines on one machine; it does not protect against a process that ignores it, or network filesystems with weak `flock`.
11. **Plan identity uses Git's blob id (SHA-1).** It binds approval to exact content; it is not a defense against a determined local attacker.
12. **Resource exhaustion**: no CPU, memory, disk or process-count limits.
13. **Project `.git` growth**: missions add objects, a branch and worktree metadata to the project's repository; reject removes the branch and worktree but not objects.

## Verification of these claims

The enforced behaviors above are covered by tests in `tests/engine.rs`; each limitation above is, by definition, not covered.
