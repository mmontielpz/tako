# Product Vision

Status: draft, foundation stage. Nothing here is implemented.

## Mission

Help engineers understand, design, implement, test, secure and deliver real systems with AI assistance, while keeping engineering rigor, technical ownership and verifiable results.

## Target users

- Engineers across software, AI/ML, data, cybersecurity, cloud/DevOps/platform and scientific computing.
- T-shaped practitioners who are deep in one discipline and need reliable breadth in others.
- Small teams and individuals who want control over their AI tooling rather than a single-vendor lock-in.

## Core problems

1. AI-generated code is easy to produce and hard to trust; verification is left to the human.
2. Agent tools run with broad ambient permissions and weak isolation.
3. Context and decisions are trapped in chat history, so handoffs are not reproducible.
4. Tooling is fragmented per provider and per discipline.

## Engineering-first differentiators (targets)

- Plans are reviewed before execution.
- Execution happens in isolated workspaces with explicit permissions.
- Results come with evidence: diffs, test runs, recorded limitations.
- Accept/reject remains a human decision.
- Durable project state that a new agent or engineer can pick up cold.

## Provider and tool independence

The user chooses providers, models, agents, credentials, tools and autonomy levels. TAKO must work without any private service and without a specific vendor.

## Possible future integrations

Claude Code, Codex, GitHub, Databricks, dbt, MLflow, container runtimes. These are candidates, not commitments, and none exist today.

## Non-goals

- Not a vibe-coding tool that optimizes for unreviewed output.
- Not an autonomous system that pushes, deploys or deletes without authorization.
- Not tied to a single model provider.
- Not dependent on T-Ecosystem or any private service.
- Not a new model, framework or agent runtime built for its own sake.
- No final technology stack is decided yet (see TAKO-001B).
