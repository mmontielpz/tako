# TAKO Agent-Assisted Onboarding Starter Prompts

> **Important:** Antigravity (AGY) and other external AI coding assistants operate TAKO through local CLI commands (`tako --help`, `tako setup`, `tako init`, `tako sota`, `tako evidence`, `tako export`). AGY is **not** a TAKO-native synthesis provider inside `tako sota-ai`.

## Antigravity (AGY) Starter Prompt

```text
I want to run a scientific literature review using the installed `tako` CLI on this machine.
1. Run `tako --version`, `tako doctor`, `tako setup --show --json`, and `tako providers list --json` to inspect my environment and configured sources without reading or printing any secret values.
2. If not configured, configure the Free profile with `tako setup --profile free --assistant agy --non-interactive`.
3. Ask me for my research topic, scientific objective, and whether I want offline fixture mode (`tako sota-ai --provider fixture --out-dir <ws>/ai_out`) or bounded live open-access discovery (`tako sota --iterative --sources openalex,arxiv,crossref --max-http-requests 16 --out-dir <ws>/sota_out`).
4. Run `tako init`, the selected discovery command, `tako evidence --from-workflow-dir <selected_out>`, `tako status`, and `tako export --format all`, then summarize verified vs candidate references and any `INSUFFICIENT_EVIDENCE` / `EXPLORATORY_LITERATURE_MAP` limitations honestly.
```

## GitHub Copilot Starter Prompt

```text
Use the `tako` CLI in this terminal to help me set up and run a bounded literature review:
1. Check readiness with `tako --version`, `tako doctor`, `tako setup --show --json`, and `tako providers list --json` (never read or print secret values).
2. Configure the Free or Hybrid profile with `tako setup --profile free --assistant copilot --non-interactive` (never store API keys in workspace or config files).
3. Discover exact CLI flags using `tako sota --help` and `tako evidence --help`.
4. Create a workspace with `tako init`, run bounded discovery (`--max-http-requests 16` for live open-access sources or `--provider fixture` for offline verification), run `tako evidence --from-workflow-dir <out_dir>`, `tako status`, and `tako export --format all`, then report the generated Markdown/CSV/BibTeX paths and verification status.
```
