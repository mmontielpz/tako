# TAKO Product & Architecture Principles

1. **Researcher-First:** Organize experiences around scientific tasks rather than internal engineering flags.
2. **GUI-First Researcher Experience:** Target an intuitive Linux-first Desktop workbench (`P7`) backed by the same core.
3. **CLI-First AI Integration:** Leverage existing authenticated AI CLIs (`AGY`, `Codex CLI`, `Claude Code`, `Copilot CLI`) without requiring direct LLM API keys; separate CLI subscription usage from `--allow-paid-api`.
4. **Evidence-First Scientific Reasoning:** Every claim must trace to verifiable sources; abstain (`INSUFFICIENT_EVIDENCE`) when support is absent.
5. **Deterministic-First Processing:** Prefer deterministic parsing, deduplication, and bibliographic checks before stochastic LLM calls.
6. **Minimum Sufficient Intelligence:** Invoke AI only where deterministic logic is insufficient, never silently falling back to paid APIs or nested agents.
7. **Progressive Complexity:** Keep default UX concise and approachable; reveal advanced knobs progressively (`--verbose`, advanced panels).
8. **Local-First:** Workspaces, configs, and knowledge artifacts live on the researcher's machine in open formats.
9. **Human-in-the-Loop:** Researchers approve plans, literature inclusion, and manuscript claims.
10. **Reproducibility:** Pin versions, record HTTP/provider telemetry, and preserve deterministic outputs.
11. **Interoperability:** Prefer open standards (`JSON`, `CSV`, `Markdown`, `BibTeX`, `RIS`, `LaTeX`, `.ipynb`) before complex cloud sync.
12. **KISS:** Avoid duplicated scientific algorithms or quality gates between CLI and Desktop; use explicit versioned JSON contracts over a shared Python scientific core.
