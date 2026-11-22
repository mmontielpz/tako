# MVP Scope

Status: **target, not implemented.** This defines the smallest useful ADE vertical slice.

## Vertical slice

1. Open an existing Git project.
2. Configure an AI coding provider.
3. Submit an engineering objective.
4. Review a proposed plan.
5. Execute inside an appropriately isolated workspace.
6. Inspect code changes, tests and evidence.
7. Accept or reject the result.

## Acceptance criteria (draft)

| Step | Done when |
| --- | --- |
| 1 | A local Git repository can be opened and its state (branch, uncommitted changes) is shown without modifying it. |
| 2 | One provider can be configured; credentials are stored outside the repository and never logged. |
| 3 | An objective is captured as text with explicit constraints and acceptance criteria. |
| 4 | A plan is shown and nothing executes until the user approves it. |
| 5 | Changes are made in an isolated workspace (for example a Git worktree or container); the original working tree is untouched until acceptance. |
| 6 | Diff, test commands run, their real outputs and known limitations are viewable. |
| 7 | Accept applies the change; reject discards the isolated workspace; both are recorded. |

## Out of scope for the MVP

Multiple specialized Takos, cloud/data-platform integrations, team collaboration, marketplace, any T-Ecosystem integration. Stack selection is deferred to TAKO-001B.
