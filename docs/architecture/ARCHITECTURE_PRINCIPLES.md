# Architecture Principles

Status: conceptual. **No technology stack is selected.** Selection happens in TAKO-001B after requirements and existing code (AIEF) are evaluated.

## Principles

KISS; evidence over claims; secure by default; local-first where practical; provider-agnostic; modular without premature abstraction; human-controlled authorization; durable state; minimal dependencies; no implementation without acceptance criteria.

## Conceptual boundaries

| Boundary | Responsibility |
| --- | --- |
| **TAKO desktop experience** | The user-facing surface: project, objective, plan review, evidence, accept/reject. Holds no business logic that the engine cannot also expose. |
| **TAKO Engineering Engine** | Orchestrates the objective → plan → execute → verify loop. Intended to incorporate AIEF, *subject to the consolidation audit (TAKO-002)*. |
| **Agent and tool adapters** | Thin, replaceable bridges to coding agents and tools (providers, CLIs). The engine depends on the adapter contract, not on a vendor. |
| **Context and workflow management** | Project context, mission state, checkpoints and handoffs, stored durably and readable without the app. |
| **Sandboxed execution** | Isolated workspaces with explicit filesystem, network and secret permissions. Default deny. |
| **Verification and evidence** | Runs checks, captures real outputs, records limitations. Status derives from observed results only. |
| **Optional external integrations** | GitHub, data platforms, MLOps tools, container runtimes. Always optional; the core works without them. |

## Constraints

- The engine must run without private T services.
- Secrets are never stored in the repository or in project state.
- Every boundary should be introduced only when a concrete requirement needs it.
