# Configuring proposal-only AI

TAKO starts and runs local scripted workflows without any provider configuration
or T service. AI requests happen only when **Send proposal request** is clicked in
**AI proposals**. This action sends the typed prompt, or explicitly previewed
project context, to the selected external
provider and may incur charges. No project content is collected automatically.

## Native configuration

The implemented adapter is **OpenAI**, using the official Responses API. Configure
OPENAI_API_KEY in the environment of the native desktop process using your normal
local secret-management method. Do not put keys in the repository, frontend,
prompt, .env file checked into Git, logs or mission evidence. No key-entry UI or
key export command exists. Missing/invalid credentials produce missing_credentials;
local work remains available. The native app must have the intended environment
when launched; already-running processes do not acquire later shell exports.

The transport requires `/usr/bin/curl` with HTTPS and the system CA store. It uses
no shell, custom curl config, inherited proxy or TLS override. Missing transport,
TLS/connectivity failure, authentication failure and provider errors are explicit.
This path is qualified on Linux with mock HTTP; Windows packaging/transport and
live provider account access are not qualified. No dependencies need downloading
for the offline tests in an already provisioned development environment.

## Request workflow

1. Open **AI proposals**. Select OpenAI; the other families are visibly unavailable.
2. Enter a model ID available to your own account and supporting text Responses.
   There is no model-list request, guessed fallback model or account access claim.
3. Set the timeout (1–120 seconds) and maximum output tokens (16–4096). Defaults
   are 30 seconds and 1024 output tokens. The token limit includes provider-specific
   generated reasoning where applicable; it is not a currency spending guarantee.
4. Enter only the engineering text you intend to transmit. The native prompt limit
   is 16384 UTF-8 bytes; multibyte text may reach that before the UI character limit.
   For project-aware questions, open a local project, check **Use selected local
   project for AI context**, enter individual relative file paths, request **Preview
   exact provider input** and check consent. Prior questions/answers are included
   only if you check the history option and preview them. The entire input shares
   the 16384-byte limit. See [context policy](PROJECT_AI_CONTEXT.md).
5. Click **Send proposal request**. Review the untrusted plain-text response, model
   and optional provider-reported token usage. Nothing applies it to a project,
   runs it, grants permissions or marks any mission verified/accepted.
6. Use **Cancel request** to stop local transport. Previously sent input may still
   be processed/billed. Cancellation cannot recall data or undo provider charges.

One request can run at a time. Local project selection, planning and evidence
inspection remain independent. Starting another proposal replaces the previous
in-memory result. Project mode retains the last 8 successful question/answer pairs
per project for this session, up to 16 projects, with explicit history inclusion
and a clear control. Switching context projects cancels the old request and clears
its draft and preview. Closing the app loses all context and conversation history.
There is no disk transcript persistence, evidence export, streaming, tool calling,
automatic retry or automatic provider fallback.

The request sets store=false; that does not promise zero retention under the
provider's policies. Do not include secrets. Errors and response parsing are
redacted, but arbitrary secrets manually entered as prompt text cannot be reliably
recognized. Environment credentials are accessible to the native process and are
not an OS vault; existing L0 same-user security limitations still apply.

## Offline qualification

Run `cargo test --workspace --locked` and `npm --prefix ui test`. Native adapter tests
use ephemeral loopback HTTP servers and fake credentials; test-only endpoints
cannot be configured through desktop IPC. UI test children explicitly clear the
provider credential, and native environment tests isolate fake/missing keys in
child processes. Do not set up a live-key smoke test as part of these commands.

Protocol reference: [official OpenAI Responses create documentation](https://developers.openai.com/api/reference/typescript/resources/responses/methods/create).
See [interface, transport and trust boundary](PROVIDER_ADAPTER_BOUNDARY.md).
