# TAKO

**Your tools. Your agents. Your intelligence.**

TAKO is an open-source **Agentic Development Environment (ADE)** for engineers who work across disciplines and want AI assistance without giving up engineering rigor, technical ownership or verifiable results.

> **Status: pre-alpha.** TAKO has a local scripted desktop workflow and starts in standalone mode. T integration is optional and read-only; AI provider execution remains disabled. Packaging and cross-platform qualification remain incomplete. See [current state](CURRENT_STATE.md).

## Local standalone operation

Open a local Git project, define a goal and scripted plan, grant the requested
permissions explicitly, then run, verify and inspect local evidence. No T service,
operator token or provider key is required. L0 worktrees are not a security sandbox;
only run scripts you wrote or trust.

With the existing Rust/Node dependencies and Linux desktop prerequisites available:

```sh
cargo test --workspace --locked
npm --prefix ui test
npm --prefix ui run build
cargo run --manifest-path src-tauri/Cargo.toml --features custom-protocol --locked
```

Use **Enable optional T integration** only when you want read-only backend
inspection. Its availability does not authorize local execution or verify backend
claims. Local workflows remain available when it is disabled or unreachable.
The **AI proposals** workspace supports explicit OpenAI text requests with a
native-configured credential and user-selected model. No project files or tools
are sent automatically; responses cannot execute or modify a project. Other
provider families remain unavailable. Opt-in [project context](docs/architecture/PROJECT_AI_CONTEXT.md)
supports selected files, exact preview, consent and project-scoped session history.
See [provider configuration](docs/architecture/PROVIDER_CONFIGURATION.md).
See [standalone architecture and migration](docs/architecture/STANDALONE_CORE.md)
and [provider boundary](docs/architecture/PROVIDER_ADAPTER_BOUNDARY.md).

## Who it is for

Engineers who need to understand, design, implement, test, secure and deliver real systems across:

- Software Engineering
- AI / ML Engineering
- Data Engineering
- Cybersecurity Engineering
- Cloud / DevOps / Platform Engineering
- Scientific Computing and Research

## Philosophy: T-Shaped Engineering, AI-Augmented

A T-shaped engineer has deep expertise in one area and working breadth in many. TAKO aims to extend that breadth with AI while the engineer keeps ownership of decisions, authorization and acceptance.

## Engineering assistance vs. unverified code generation

TAKO is engineering-first, not vibe-coding-first.

| Unverified code generation | Engineering assistance (TAKO's target) |
| --- | --- |
| Produces code and asks you to trust it | Proposes a plan you review before execution |
| Runs with ambient permissions | Runs with explicit, human-controlled permissions in isolated workspaces |
| "Done" is a claim | "Done" is backed by tests, diffs and recorded evidence |
| State lives in a chat | State is durable and handoffs are reproducible |

## Tako and Takos

- **Tako** is the primary companion: the engineer's single point of interaction.
- **Takos** are specialized agents (for example security, data or infrastructure) that Tako can work with.

You choose the providers, models, agents, credentials, tools and autonomy levels. TAKO is useful on its own and does not require any private service.

## T-Ship

T-Ship is TAKO's delivery methodology: small, verified increments with explicit acceptance criteria and durable checkpoints. See [AGENTS.md](AGENTS.md) for the working workflow.

## Open-source vision

TAKO is fully open source. A small number of related projects remain private; the boundary is documented in [docs/architecture/OPEN_SOURCE_BOUNDARY.md](docs/architecture/OPEN_SOURCE_BOUNDARY.md). Licensed under the [Apache License 2.0](LICENSE); see [docs/decisions/0001-license.md](docs/decisions/0001-license.md).

## Documentation

- [Product vision](docs/product/PRODUCT_VISION.md)
- [MVP scope](docs/product/MVP_SCOPE.md)
- [Architecture principles](docs/architecture/ARCHITECTURE_PRINCIPLES.md)
- [Open-source boundary](docs/architecture/OPEN_SOURCE_BOUNDARY.md)
- [Roadmap](docs/product/ROADMAP.md)
- [Core contracts](docs/architecture/CORE_CONTRACTS.md)
- [Current state](CURRENT_STATE.md)

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Report security issues as described in [SECURITY.md](SECURITY.md). All participation is governed by the [Code of Conduct](CODE_OF_CONDUCT.md).
