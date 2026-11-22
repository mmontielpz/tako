# Product Decision Register (`M028E`)

| ID | Decision | Rationale | Status |
|---|---|---|---|
| **PD-001** | Adopt `ARDE` V2.0 vision: GUI-first researcher workbench (`P7`) + CLI-first automation/agent interface | Resolves CLI-only friction while preserving reproducibility and agent interop | **Approved** |
| **PD-002** | `LINUX_FIRST` strategy (`CachyOS` primary, `Ubuntu 24.04 LTS` secondary; Windows/macOS deferred) | Focuses limited engineering capacity on owner's active research environment | **Approved** |
| **PD-003** | CLI-first AI providers (`agy`, `codex`, `claude`, `copilot`) separate from `--allow-paid-api` | Researchers use existing authenticated CLI sessions without extra API keys | **Approved** |
| **PD-004** | Shared Python scientific core + Rust/Tauri Desktop via versioned JSON bridge | Zero duplicated scientific logic or integrity gates | **Approved** |
| **PD-005** | `Create a State of the Art` is the first end-to-end workflow | Validates discovery -> verification -> comparison -> synthesis before broader studios | **Approved** |
| **PD-006** | Open local formats (`BibTeX`, `RIS`, `LaTeX`, `MD`, `CSV`, `JSON`, `.ipynb`) before cloud sync | Avoids premature vendor lock-in and sync complexity | **Approved** |
| **PD-007** | Preserve all Tier 1 evidence gates (`INSUFFICIENT_EVIDENCE`, verified vs candidate split) | Prevents citation fabrication and overclaimed sufficiency | **Approved** |
| **PD-008** | Retain `ResearchOS` concepts conceptually only; local ResearchOS artifacts unavailable | Zero fabrication; do not adopt external stack blindly | **Recorded** |
