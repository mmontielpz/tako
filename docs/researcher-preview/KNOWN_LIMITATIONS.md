# Known Limitations (`0.0.1.dev2`)

1. **Windows Runtime Not Tested:** Built as `py3-none-any` and verified on `Linux x86_64` (Python `3.14.10`). No Windows host was available (`WINDOWS_RUNTIME_STATUS=NOT_TESTED`).
2. **Live LLM Providers Not Tested:** `copilot-cli`, `claude-cli`, and `openai-responses` were verified via unit tests and fail-closed checks; live LLM execution requires researcher credentials/session and `--allow-paid-api`.
3. **Copilot Grounded Search:** Copilot CLI `1.0.114` documents `--allow-url` and GitHub MCP, not general web search; pair `--provider copilot-cli` with `--verify-bib` or use `tako sota` (OpenAlex).
4. **Codex & Gemini:** Marked `NOT_IMPLEMENTED` in `tako sota-ai`.
5. **AI-SDLC Lite Verification:** Repository verification on TAKO remains `PARTIAL` due to `>20,000` ignored Rust artifacts in `target/` / `src-tauri/target/`.
