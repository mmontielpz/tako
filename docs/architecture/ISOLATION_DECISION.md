# Isolation Decision

Status: **DECIDED (TAKO-003H): no sandbox is implemented in this mission; untrusted execution stays disabled.** The next step is a dedicated mission, TAKO-005.

## Levels and guarantees

| Level | Mechanism | Guarantees | Does **not** guarantee | Status |
| --- | --- | --- | --- | --- |
| **L0** `L0_worktree` | Git worktree outside the project, cleared env, process group, timeout | Accidents are contained to a branch/worktree; the project tree is only changed on human Accept; env secrets are not inherited | Any confinement. The agent runs as the user: it can read and write everything the user can, use the network, and edit the project's `.git` | Implemented. **Not a sandbox.** |
| **L1** Linux namespaces | bubblewrap (`bwrap`): read-only system mounts, only the workspace writable, no network, minimal env, no `$HOME` | Agent cannot write outside the workspace, read the user's home or secrets, or use the network (to be proven by tests) | Kernel exploits; resource exhaustion unless limits added; side channels | Evaluated, not implemented |
| **L2** VM or rootless container | Container/VM with its own kernel or user namespace | Stronger separation, portable image, resource limits | Costs startup time and setup; weaker than a VM if the host kernel is shared | Evaluated, not implemented |

Rule: **a provider agent (`AgentKind::Provider`) is refused at any level below L1.** The engine enforces this in code today (`IsolationRequired`); only `AgentKind::Scripted` (human-authored deterministic scripts) may run, and that label is a caller promise the engine cannot verify.

## Evaluation

Observed on this machine (Linux, unprivileged user namespaces enabled, bubblewrap 0.13.1), with a throwaway probe directory only:

- `bwrap --unshare-all --ro-bind /usr /usr --proc /proc --dev /dev --tmpfs /tmp --bind <ws> /work --clearenv`: writing into the bound workspace worked; writing to `/usr` was denied (read-only); a file outside the binds and `/home/<user>` were not visible; no network (`--unshare-net`; only a loopback device in `/proc/net/dev`); the environment contained only the variables set explicitly. These are probe observations, **not a qualification**: no test suite exists for them yet.
- Docker 29.x is installed, but its security options show no rootless mode, so access to it is root-equivalent. Using it as the sandbox would grant the engine (and anything that can reach its socket) more privilege than it removes. Podman is not installed.

| Option | Pros | Cons / risks | Verdict |
| --- | --- | --- | --- |
| **bubblewrap (Linux)** | Small, no daemon, unprivileged, fast, easy to test | Linux only; needs user namespaces enabled (disabled on some distros/containers); policy is hand-built so mistakes are possible | **Recommended L1 for Linux** |
| **Rootless containers (podman, rootless docker)** | Portable images, resource limits | Heavier; rootless runtime may be absent; rootful docker is not acceptable as L1 | Optional L2 |
| **macOS** | `sandbox-exec`/Seatbelt exists but is deprecated and undocumented; App Sandbox needs signed apps; practical strong option is a Linux VM/container (Lima, Apple container tooling, Docker Desktop VM) | No lightweight native equivalent of bwrap | L2 via VM only; not verified in this mission |
| **Windows** | Job Objects (resource limits, not confinement), AppContainer/restricted tokens (complex), WSL2 or Windows Sandbox (VM-grade) | No simple namespace sandbox | L2 via WSL2/VM only; not verified in this mission |

## Why L1 was not implemented now

1. **The workspace design conflicts with it.** A Git worktree's `.git` file points into the project's `.git`. To make that work inside bwrap, the project's `.git` (refs, config, hooks, objects) would have to be mounted writable, which hands the agent the very repository we are protecting. L1 needs a self-contained workspace (for example a local clone with its own object store) and a controlled hand-off of the result. That is a workflow change, not a flag.
2. Verification checks also run unsandboxed today and would need the same treatment.
3. Qualification requires adversarial tests (write outside, read secrets, network, symlink and mount tricks, resource exhaustion, cleanup of the sandbox process tree) that do not fit in this mission without expanding scope.
4. A half-qualified sandbox that is trusted would be worse than an honest L0.

## Follow-up: TAKO-005 — L1 isolation (Linux first)

Scope: self-contained clone workspace; bwrap launcher with a deny-by-default policy; adversarial test suite; apply the same to verification; capability detection with a clear refusal when namespaces are unavailable; keep `Provider` refused wherever L1 cannot be established. Acceptance: tests demonstrate no write outside the workspace, no home/secret read, no network, process tree fully cleaned, resource limits enforced.

## Until then

Untrusted or AI-provider execution is disabled in code. The UI (TAKO-004) must show the isolation level and must not present L0 as a sandbox.
