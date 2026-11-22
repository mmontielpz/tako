# Known Limitations (`0.0.1.dev3`)

1. **Windows Runtime Not Tested:** Built as `py3-none-any` and verified on `Linux x86_64` (Python `3.14.10`). No Windows host was available (`WINDOWS_RUNTIME_STATUS=NOT_TESTED`).
2. **Fish Shell Note (`F01`):** On Linux Fish shell, use `source .venv/bin/activate.fish` or invoke `./.venv/bin/tako` directly instead of Bash `. .venv/bin/activate`.
3. **OpenAlex Scientific Quality Limits (`F04`–`F06`):** Top-10 lexical/seed ranking can miss early foundational papers (e.g., Jaeger 2001/2004, Maass 2002, Fujii & Nakajima 2017); OpenAlex `concepts` can include noisy disambiguations; abstract sentence-cue matching for limitations can trigger on negations (e.g., `"noiseless"`).
4. **Live LLM Providers Not Verified End-to-End:** `copilot-cli` uses `--output-format json --stream off --no-ask-user --no-custom-instructions`; live LLM inference requires user authentication and explicit `--allow-paid-api` authorization.
5. **Copilot Grounded Search:** Copilot CLI `1.0.114` documents `--allow-url` and GitHub MCP, not general web search; pair `--provider copilot-cli` with `--verify-bib` or use `tako sota` (OpenAlex).
6. **Codex & Gemini:** Marked `NOT_IMPLEMENTED` in `tako sota-ai`.
