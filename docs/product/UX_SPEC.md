# UX Specification

Status: design target for TAKO-004+. Nothing is implemented. Scope: the first mission flow in [FIRST_WORKFLOW.md](../architecture/FIRST_WORKFLOW.md). English only. Not an IDE: no code editor, no file-tree editing.

## Principles

- One primary action per state; the next safe step is always obvious.
- Progressive disclosure: summary first, detail on demand (plan steps, raw logs, check output).
- State is never hidden; every claim in the UI links to evidence or says "not verified".
- Approvals are explicit, specific and impossible to trigger by accident.

## Layout (single window)

```
┌──────────────┬────────────────────────────────────────────┬───────────────┐
│ Projects     │ Mission workspace (central)                │ Tako          │
│  ▸ repo-a    │  Header: goal · state chip · isolation tag │  companion    │
│    missions  │  Stage panel (changes per state)           │  panel        │
│  ▸ repo-b    │  ─ Goal | Plan | Run | Evidence | Decide   │  (collapsible)│
│ + Open       │  Tabs are a progress path, not free nav    │               │
│ ⚙ Settings   │                                            │               │
└──────────────┴────────────────────────────────────────────┴───────────────┘
 Status bar: provider · mission state · elapsed · limits
```

## Screens and states

1. **Open project.** Pick a Git folder. Shows branch, HEAD, dirty flag. Non-repo: explain and stop. Nothing is modified.
2. **Goal.** Text goal; required acceptance criteria list; verification commands (suggested from repo, user confirms). Primary action: "Ask Tako for a plan".
3. **Plan review.** Summary, steps, expected files, *requested permissions* (checklist, all off by default), risks. Actions: Approve (shows exact plan version + grants being approved), Request changes (text), Cancel. Editing the plan voids a prior approval, visibly.
4. **Run.** Live event log (collapsed by default to key events, expandable to raw output), elapsed time, timeout bar, Cancel. Isolation tag always visible: "Isolated worktree (not a security sandbox)".
5. **Evidence.** Diff (file list + unified diff), changed-file counts, each verification check with command, exit code, duration, expandable output, limitations list, and a validity banner (tree hash unchanged / verification modified subject). Agent summary appears separately, labeled "Agent's claim".
6. **Decide.** Accept / Reject. Accept shows the exact Git operation and target branch; disabled with reason if evidence is invalid or the project moved. Reject asks for confirmation before discarding. Optional note.
7. **Agent settings.** List providers and Takos; add a provider (command, env allowlist, credential reference), set a Tako's role. Credential values are never displayed.

## Approval controls

Primary buttons for approvals are visually distinct, labeled with the object ("Approve plan v2 with 1 permission"), and require a single deliberate click; no auto-approval setting in the MVP. Destructive actions (discard) use a confirmation naming what will be lost.

## Errors

Plain language, cause first, one suggested action, details expandable. Examples: "Provider isn't signed in. Nothing was run."

## Accessibility baseline

Keyboard-operable, visible focus, color never the only signal, contrast per [DESIGN_SYSTEM.md](DESIGN_SYSTEM.md), motion respects reduced-motion.

## Out of scope for the first slice

Multiple concurrent missions, editing code in-app, embedded terminal, multi-agent runs, themes beyond light/dark, localization.

## Acceptance (for the UI mission)

A new user can complete the seven-step flow on a fixture repo without reading docs; every state in the machine is reachable and shows its reason; approvals cannot be bypassed from the UI.

## Implementation status (TAKO-004)

Implemented in `ui/` and tested against the real engine in a DOM emulator (never in a real window): project open, goal form, plan with unchecked permission list and verbatim script, run with live output, evidence table with integrity violations separate from failed checks, diff, accept/reject with confirmation, error banners (Busy, UnsafeRepository, others), startup recovery banner, L0 badge and notice, progress path. Differences from the spec above: a single scrolling workspace column instead of tabs; "Agent settings" is not built (one scripted agent, AI provider shown disabled); polling instead of push events; no dark/light toggle (follows the OS). Cross-OS rendering is untested.

## Review against the running application (TAKO-004B)

Reviewed on the real desktop app; findings and fixes are recorded in `docs/missions/TAKO-004B/REPORT.md` and `docs/missions/TAKO-004B/review/README.md` (internal archive, excluded from public repo). Changes made to the layout rule: the card that holds the current primary action is shown first; earlier stages (the plan) move below later results; the decision card carries an evidence summary next to Accept/Reject.
