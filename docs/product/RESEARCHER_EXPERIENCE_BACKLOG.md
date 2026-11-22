# Researcher Experience Backlog (`M028D` Reconciled)

## Reconciliation of `M028D` Audit Inconsistencies
1. **Finding Counts:** `M028D` recorded `26` raw `RX` items (`RX-001`–`RX-026`, with `RX-001` resolved in `M028B`) + `1` AGY workspace gap (`AGY-WS-01`), deduplicated into `10` consolidated findings (`F-001`–`F-010`: 4 HIGH, 5 MEDIUM, 1 LOW).
2. **Execution Times:** Human pilot `sota` wall time was `9.544s` (`discovery_trace.json`); AGY live `sota` CLI wall time was `7.323s` (vs `7.2188s` inner trace `wall_time_seconds`, diff `~0.104s` Python startup). `RX-026` `claude-cli` block was `80.17ms` (human) vs `77.59ms` (AGY).
3. **Missing Primary Evidence:** Local `ResearchOS` AI project artifacts were **not found** in the local filesystem (`RESEARCHOS_SOURCES=UNAVAILABLE`); no ResearchOS claims were fabricated. Agent token/cost metrics remain `NOT_OBSERVABLE` in `M028D`.
4. **Roadmap Alignment:** `M028D` preliminarily grouped `F-001`–`F-010` into `M029A`–`M029C`; this canonical backlog aligns them cleanly to `P6` (`M029A`, `M029B`, `M029C`) and `P7` (`M029E`–`M029H`).

## Prioritized Implementation Backlog

| ID | RX Refs | Severity | Summary | Effort | Risk | Dependencies | Target Mission | Acceptance Criteria |
|---|---|---|---|---|---|---|---|---|
| **F-001** | `RX-005,007,008,021,022,023,025,026` | HIGH | CLI-first AI provider architecture (`agy`, `codex`, `claude`, `copilot`) separating CLI subscriptions from `--allow-paid-api` | M | Medium | None | `M029A` | All 4 CLIs detected, configurable via `setup`, runnable under explicit CLI session policy |
| **F-003** | `RX-003` | HIGH | Fix `tako setup --profile` displaying numbered menu then failing on `1` | S | Low | None | `M029A` | `--profile free` skips profile menu; interactive choices accept numbers or names |
| **F-002** | `RX-002` | MEDIUM | Unknown top-level flags (`tako --verison`) fail without typo suggestion | XS | Low | None | `M029A` | `tako --verison` suggests `--version` and exits 2 |
| **F-004** | `RX-006,009` | MEDIUM | Concise default `tako doctor` (`--verbose` for full details) & fix stale `windows_native` note | S | Low | None | `M029A` | Default `tako doctor` <= 12 lines; accurate platform status |
| **F-005** | `RX-010,011,012` | MEDIUM | Update OpenAlex-only help text, progressive flag disclosure, practical examples | S | Low | None | `M029A` | `tako --help` & `tako sota --help` reflect multi-source + examples |
| **F-006** | `RX-013,014,015` | HIGH | Live `tako sota` progress, timing/HTTP stats, and human summary (`--json` option) | M | Low | None | `M029B` | Live runs emit round progress, timing, and readable summary |
| **F-010** | `AGY-WS-01` | MEDIUM | Add `--workspace` integration to `tako sota` and `tako sota-ai` | S | Low | None | `M029B` | `tako status` accurately shows `discovery: COMPLETED` |
| **F-009** | `RX-004` | LOW | Clean `NO_COLOR`-compliant terminal hierarchy across installer & CLI | S | Low | None | `M029B` | Clean output respecting `NO_COLOR` |
| **F-007** | `RX-016,018,019` | HIGH | Citation-weighted foundational paper discovery, relevance ranking, and `MIN_TOPIC_BREADTH_REACHED` naming | L | Medium | `M029B` | `M029C` | Surfaces seminal architectures without weakening evidence gates |
| **F-008** | `RX-017,020,024` | MEDIUM | CLI-first agentic extraction pass preserving `ABSTRACT_ONLY` / provenance boundaries | L | Medium | `F-001` | `M029C` | Extracted claims linked to provenance and `ABSTRACT_ONLY` vs `FULL_TEXT` |
