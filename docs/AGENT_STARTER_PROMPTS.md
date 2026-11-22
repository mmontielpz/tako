# TAKO Agent-Assisted Onboarding Starter Prompts

> **Important:** Antigravity (AGY) and other external AI coding assistants operate TAKO through local CLI commands (`tako --help`, `tako setup`, `tako init`, `tako sota`, `tako evidence`, `tako export`). AGY is **not** a TAKO-native synthesis provider inside `tako sota-ai`.

## Antigravity (AGY) Starter Prompt
```text
I want to run a scientific literature review using the installed `tako` CLI on this machine.
1. Run `tako --version`, `tako doctor`, `tako setup --show --json`, and `tako providers list --json` to inspect my environment and configured sources without reading or printing any secret values.
2. If not configured, guide me through `tako setup --profile free --assistant agy --non-interactive`.
3. Ask me for my research topic, scientific objective, and whether I want offline fixture mode or bounded live open-access discovery (with an explicit `--max-http-requests` cap).
4. Run `tako init`, `tako sota` (or `tako sota-ai --provider fixture`), `tako evidence`, `tako status`, and `tako export --format all`, then summarize the verified vs candidate references and any `INSUFFICIENT_EVIDENCE` / `EXPLORATORY_LITERATURE_MAP` limitations honestly.
```

## GitHub Copilot Starter Prompt
```text
Use the `tako` CLI in this terminal to help me set up and run a bounded literature review:
- Check readiness with `tako doctor` and `tako providers list --json`.
- Configure the Free or Hybrid profile with `tako setup --profile free --assistant copilot --non-interactive` (never store API keys in workspace or config files).
- Discover CLI options using `tako sota --help` and `tako evidence --help`.
- Create a workspace with `tako init`, run bounded discovery (`--max-http-requests`), run `tako evidence` and `tako export --format all`, and report the generated Markdown/CSV/BibTeX paths and verification status.
```
