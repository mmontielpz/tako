# Project-aware proposal context

TAKO-013 adds an explicit context boundary in `bridge/src/workspace.rs` above the
unchanged proposal-only provider service. Local project selection does not grant
provider filesystem access. The project-context toggle starts unchecked; T project
views cannot supply context. There is no recursive browser, repository walker,
automatic upload, provider tool, code execution or file application.

## User and native flow

1. Open a local Git project and enable project context in AI proposals.
2. Enter individual relative source/documentation paths, one per line. Type a
   question. Optionally select this project's conversation history (off by default).
3. Request a preview. The native bridge validates the local project through the
   existing engine, reads only the selected files, and assembles the provider's
   exact input string. No provider or credential access occurs during preview.
4. Read the complete escaped-text preview and its UTF-8 byte count. Check the
   separate consent checkbox, then send using the visible provider/model/limits.
5. The native bridge requires the preview ID, matching canonical project path and
   `consent=true`. It sends the frozen text through the existing provider service.
   It never rereads files on send. A successful submit consumes the snapshot.

The preview shows the complete `input` value sent to the provider, including file
labels, selected contents, optional prior questions/answers and the new question.
Provider/model/output-token settings remain visible controls; the transport wraps
this input in the existing reviewed JSON envelope (`tools=[]`, `tool_choice=none`,
`store=false`, `stream=false`). Absolute project paths are not added to the input.
File contents and history are untrusted data; delimiters are explanatory labels,
not a prompt-injection security boundary. Output is rendered as escaped text.

Editing the question, selected paths or history option removes preview and consent.
Late preview responses are discarded by a local generation check. Switching context
projects remounts the proposal view, clears drafts/previews/consent and cancels the
old request, including a late start acknowledgement. Cancellation cannot recall
already transmitted data. Text-only requests retain the TAKO-012 path.

## Filesystem and size policy

- At most 8 explicitly named files, 4096 bytes per file; **16384 UTF-8 bytes total**
  for the fully assembled input, including question, labels and optional history.
  UTF-8 bytes, rather than JavaScript character counts, are authoritative.
- Relative normal components only; absolute paths, parent traversal, backslashes,
  control characters and duplicates are rejected. All dot-prefixed components,
  dependency/build directories (`node_modules`, `target`, `vendor`), SSH identity
  names and components containing secret/credential/token/password/private are
  refused, case-insensitively.
- Allowed extensions: rs, ts, tsx, js, jsx, py, md, txt, css, html, c, h, cpp, go,
  java, sh. Configuration/data formats such as env, JSON, YAML, TOML, certificates,
  keys, archives and executables are excluded. No unsupported-type override exists.
- On Unix every component, including canonical root ancestors, is opened relative
  to a held directory descriptor with `O_NOFOLLOW`; directory components require
  `O_DIRECTORY`. There is no canonicalize-then-read race. Final files must be regular
  with link count one; hard-link aliases and all symlinks, even internal ones, fail.
  `O_NONBLOCK` prevents special-file opens from hanging. Reads are capped at limit+1
  bytes to detect growth. Invalid UTF-8 and non-text control characters fail closed.
- Obvious credential markers in permitted text (private key, API key assignment,
  password/secret assignment, bearer headers, AWS access key and common token
  prefixes) are refused. This is conservative defense in depth, **not universal
  secret detection**: renamed or embedded secrets may evade patterns. Users must
  inspect selected content and the preview; never paste credentials into questions.
- Non-Unix file-context reads fail closed pending a descriptor-equivalent design.
  Linux is qualified; other native platforms have not been exercised.

Reusing the workspace's already pinned `libc` crate directly in the bridge enables
`openat` and no-follow flags; no dependency version was added or upgraded. Held
file descriptors prevent path redirection, not malicious same-user file writes or
mount manipulation. Snapshot bytes may reflect concurrent edits during a read;
only the resulting preview bytes are sent. TAKO does not claim an OS sandbox here.

## Conversation isolation and bounds

Native memory holds the last 8 successful question/answer pairs for each canonical
local project path, up to 16 projects per bridge session. Adding a seventeenth may
evict an existing project's session history. The current file contents are not
stored as conversation turns and are never implicitly reused. If a follow-up needs
files, select them again. History inclusion is a separate unchecked option, and all
included history appears in the next preview and shares its total input limit.
Oversized history fails closed; clear it or omit it to continue.

Terminal success is reconciled during polling and before context/history operations
or another submission. Request IDs bind replies to their original project, so a
late result cannot populate another project's history. Cancelled/failed requests
add no turn. Clear removes that project's stored turns and pending history binding,
and revokes the snapshot. No history is persisted, exported as evidence, or loaded
from T. Restarting TAKO clears all context and history. The provider service retains
only its most recent job as before; there is no cross-project shared conversation.

## IPC and retained boundaries

| Command | Native behavior |
| --- | --- |
| context_preview | Project path, explicit relative paths, question, include_history; returns frozen text, byte count and preview ID |
| workspace_start | Preview ID, same project, consent and provider settings; cannot supply replacement text or filesystem tools |
| conversation_history | Reads session history for a validated canonical local project |
| conversation_clear | Clears that project's history and revokes preview |
| proposal_poll / proposal_cancel | Existing ID-bound lifecycle; observes terminal results for history |

Context errors use `context_path`, `context_limit` or `context_consent` with static
messages and no file contents. Existing provider selection, single-flight limit,
timeouts, output bounds, cancellation/reaping and structured redacted errors remain
in [the provider boundary](PROVIDER_ADAPTER_BOUNDARY.md). Provider execution remains
refused by the engine. No service, credential configuration, T adapter, evidence
Authority or isolation claim changes.
