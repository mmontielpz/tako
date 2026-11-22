# Changelog

All notable changes to the public TAKO repository are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [PEP 440](https://peps.python.org/pep-0440/) / [SemVer](https://semver.org/) for releases.

## [Unreleased]

### Added
- `CHANGELOG.md` documenting verified `v0.0.1.dev3` capabilities, limitations, and canonical `P0`–`P8` roadmap.
- Deterministic Python CLI CI job (`python-cli`) in `.github/workflows/hygiene.yml` covering unit tests (`tests/sota_discovery`, `tests/sota_ai_native`), wheel build, SHA-256 generation, isolated wheel installation, `tako --help`, `tako doctor`, and offline fixture workflow verification.

### Fixed
- `scripts/check.sh`: removed assertions for private `docs/missions/*` files excluded from the sanitized public repository, and added required checks for `CHANGELOG.md`, `pyproject.toml`, and `tools/` CLI entrypoints.
- Replaced broken relative links to excluded `docs/missions/*` reports in `CURRENT_STATE.md`, `docs/architecture/DESKTOP_STACK_DECISION.md`, `docs/architecture/PROVIDER_ADAPTER_BOUNDARY.md`, `docs/product/ROADMAP.md`, and `docs/product/UX_SPEC.md`.

### Known Open Items
- `CODE_OF_CONDUCT.md` confidential reporting contact remains pending maintainer decision (`[INSERT CONTACT METHOD]`).
- Native Windows runtime remains `NOT_TESTED`.
- `0.0.1.dev4` (Increment A: scientific integrity & verification fixes `F01`–`F06`) is not yet released.

## [0.0.1.dev3] - 2026-11-18 (Experimental Researcher Preview)

### Added
- Python CLI (`tako`) with `doctor`, `sota` (deterministic OpenAlex discovery), and `sota-ai` (Researcher -> Analyst -> optional Reviewer workflow with bundled offline `fixture` mode and `copilot-cli` adapter).
- Stage persistence (`stage_01_researcher.json`, etc.), OpenAlex bibliographic verification (`--verify-bib`), and structured outputs (`sota_report.md`, `literature_matrix.csv`, `references.bib`, optional `review_report.md`).
- 16 offline Python regression tests (`11` in `tests/sota_ai_native`, `5` in `tests/sota_discovery`).

### Known Limitations (`v0.0.1.dev3`)
- Scientific integrity defects tracked for `0.0.1.dev4` (`F01`–`F07`): DOI contamination (`F01`), duplicate results/limitations (`F02`), misrepresented evidence provenance (`F03`), BibTeX author separation (`F04`), cost/premium-request accounting (`F05`), evidence classification/traceability (`F06`), and discovery relevance limitations (`F07`).
- Native Windows execution is `NOT_TESTED` (Linux CachyOS qualified; WSL2 pilot submitted in PR #3).
