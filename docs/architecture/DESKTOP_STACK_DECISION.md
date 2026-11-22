# Desktop Stack Decision

Status: **RECOMMENDED** (TAKO-001B). Nothing is installed or scaffolded. Decision record 0003 should be created when the owner accepts it.

## Criteria (weighted by TAKO principles)

| # | Criterion | Weight |
| --- | --- | --- |
| 1 | Security model: least privilege, small attack surface | High |
| 2 | Process, PTY and Git integration for the engine | High |
| 3 | Minimal dependencies and maintenance cost | High |
| 4 | Linux and macOS support now, Windows portability later | Medium |
| 5 | Resource use and install size | Medium |
| 6 | Contributor accessibility and developer experience | Medium |
| 7 | UI rendering consistency (pixel-art companion, diffs) | Medium |

## Options

| | **A. Tauri 2 + Rust core + TS UI** | **B. Electron + Node/TS** | **C. Native UI (e.g. Rust GUI toolkit)** |
| --- | --- | --- | --- |
| Security | Capability-based permissions, small Rust surface; UI sandboxed in webview | Must be hardened by hand (context isolation, no node in renderer); large Chromium+Node surface | Smallest surface, no webview |
| Process / Git | `std::process`, Git CLI; PTY needs one crate | Mature (`child_process`, node-pty) | Same as A |
| Dependencies | Rust toolchain + system webview; Cargo ecosystem | Node + Chromium bundled; large npm tree | Toolkit-dependent; rich diff/terminal widgets immature |
| Platforms | Linux, macOS, Windows | Linux, macOS, Windows | Varies |
| Footprint | Smaller installers; memory advantage is smaller and inconsistent across sources | Larger installers (~100s of MB) | Smallest |
| Rendering | **Differs per OS** (WebKitGTK / WKWebView / WebView2) | Identical Chromium everywhere | Toolkit-dependent |
| Contributor DX | Needs Rust + web skills | Single language (TS) | Narrow talent pool |

External facts used (secondary sources, not independently measured here): Tauri uses the OS webview (WebView2 on Windows, WKWebView on macOS, webkit2gtk on Linux; versions vary by distro on Linux) per the [official webview reference](https://v2.tauri.app/reference/webview-versions/); size comparisons from a [2026 test](https://betterstack.com/community/guides/scaling-nodejs/tauri-vs-electron-vs-deno-vs-electrobun/) show a large installer gap (about 57 MB vs 323 MB for one app) but a small memory gap (109 vs 128 MB); other sources vary and advise measuring on target. Granular capability files are Tauri's permission mechanism (secondary source; verify against official docs in TAKO-004).

## Recommendation

**Option A: Tauri 2 shell, a UI-independent Rust engine crate, and a TypeScript web UI.**

Why: the product's core promise is governed, least-privilege execution; a capability-based shell with a small Rust core fits that better than hardening Electron. Installer size and dependency count are lower. Both Rust (1.99.1) and Node (24.16) exist on the maintainer's machine, so the toolchain cost is nil there.

Costs accepted: per-OS webview differences (mitigated by a plain, conservative UI and CSS, and by a cross-OS render check later); higher contributor bar for Rust; Linux needs `webkit2gtk` (not installed on the maintainer machine as of this mission, `pacman -Q webkit2gtk-4.1` not found).

## The decision that matters more: engine separation

The **engine is a headless library with a small, versioned JSON event/command interface**; the desktop shell is a client. Consequences:

- The engine is testable without any UI (TAKO-003 builds and verifies it before any window exists).
- The stack choice stays reversible: if webview inconsistency hurts, an Electron or other shell can host the same engine process.
- It keeps the MVP honest: no UI work is needed to prove the engine works.

## UI framework

Deferred to TAKO-004's first task, which must choose one small framework (or none) with a stated reason; no framework is chosen here.

## Re-evaluation triggers

Switch the shell to Electron if: webview rendering bugs on any supported OS cost more than they save in two consecutive missions, or a required terminal/editor component is unavailable for the webview. Record the evidence.

## Not decided

Editor/terminal components, auto-update, packaging/signing, Windows support timeline.

## Outcome (TAKO-004B)

Tauri 2 shell qualified on Linux: compiled, launched, native folder picker and the engine bridge working, strict CSP, a single granted plugin permission (`dialog:allow-open`). Observed cost of the webview model: WebKitGTK needed `WEBKIT_DISABLE_DMABUF_RENDERER=1` on the test machine (blank window otherwise). macOS and Windows rendering remain unverified, so the re-evaluation triggers above still apply. See `docs/missions/TAKO-004B/REPORT.md` (internal archive, excluded from public repo).
