# Provider Compatibility Matrix (`0.0.1.dev2`)

| Provider | Interface | Auth Mechanism | Native Search | Status |
|---|---|---|---|---|
| **OpenAlex (`tako sota`)** | HTTPS REST (`api.openalex.org`) | None required | Yes (scholarly metadata + abstracts) | `AVAILABLE` (`LIVE_TESTED: PASS` on Linux) |
| **GitHub Copilot CLI (`copilot-cli`)** | `copilot -p <prompt> -s` | Researcher's `copilot login` session | `NOT_VERIFIED` (supports `--allow-url` & GitHub MCP; pair with `--verify-bib`) | `AVAILABLE` (`LIVE_TESTED: NOT_TESTED`) |
| **Claude Code CLI (`claude-cli`)** | `claude -p <prompt> --output-format json` | Researcher's `claude` session | Built-in CLI `WebSearch` (session-dependent) | `AVAILABLE` (`LIVE_TESTED: NOT_TESTED`) |
| **OpenAI Responses (`openai-responses`)** | `https://api.openai.com/v1/responses` | `OPENAI_API_KEY` env var | Yes (`web_search_preview`) | `CONFIG_REQUIRED` (`LIVE_TESTED: NOT_TESTED`) |
| **Offline Fixture (`fixture`)** | Bundled `example_fixture.json` | None | No (test fixture only) | `AVAILABLE` (`PASS` offline) |
| **Codex CLI (`codex-cli`)** | N/A | N/A | N/A | `NOT_IMPLEMENTED` |
| **Gemini CLI (`gemini-cli`)** | N/A | N/A | N/A | `NOT_IMPLEMENTED` |
