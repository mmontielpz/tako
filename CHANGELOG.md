# Changelog

All notable changes to the public TAKO repository are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [PEP 440](https://peps.python.org/pep-0440/) / [SemVer](https://semver.org/) for releases.

## [Unreleased] — `0.0.1.dev4` Candidate (Increment A: Scientific Integrity)

### Added
- Shared scientific integrity module (`tools/sota_integrity.py`) and regression tests (`tests/sota_ai_native/test_scientific_integrity.py`).
- Bibliographic segregation (`M017-F06`): `references.bib` contains only bibliographically verified identities; unresolved or mismatched records are routed to `references_candidates.bib` with discrepancy evidence and without emitting mismatched DOIs.
- Orthogonal provenance columns in `literature_matrix.csv` (`Bibliographic status`, `Evidence availability`, `Claim support status`, `Human review status`), keeping bibliographic identity separate from scientific claim support (`F03`, `F06`).

### Fixed
- `F01`: Independent bibliographic identity verification separating reported identifier, normalized identifier, retrieved authoritative metadata, verification outcome, discrepancy evidence, and corrected identity.
- `F02`: Deterministic sentence-level deduplication across `Reported results` and `Limitations` and list items without dropping distinct findings.
- `F03` / `F06`: System-enforced `EVIDENCE_AVAILABILITY` (`MODEL_RECALL` for `copilot-cli` recall outputs, `ABSTRACT_BASED` / `METADATA_ONLY` for OpenAlex), ignoring unsupported LLM self-attestations.
- `F04`: BibTeX ` and ` author separator in `references.bib` and `references_candidates.bib` without splitting on commas.
- `F05`: Separated `cost_usd` (`"UNKNOWN"` when monetary cost is unavailable; `0.0` only for local `fixture`), `premium_requests`, `request_count`, `input_tokens`, `output_tokens`, `total_tokens`, `provider_specific_usage`, and `wall_time_seconds` (breaking telemetry fix: `premium_requests` no longer populates `cost_usd`).

## [Unreleased] — Engineering Foundation (`P0`, Integrated into `develop`)

### Added
- `CHANGELOG.md` documenting verified `v0.0.1.dev3` capabilities, limitations, and canonical `P0`–`P8` roadmap.
- Deterministic Python CI job (`python-cli`, Python `3.12`) in `.github/workflows/hygiene.yml` covering all 41 Python unit tests (`tests/sota_discovery`, `tests/sota_ai_native`, `tests/evidence_auth`), wheel build, SHA-256 generation, isolated wheel installation, `tako --help`, `tako doctor`, and offline fixture workflow verification.
- Synthetic, public-safe `docs/contracts/backend-evidence/v1/conformance-example.json` restoring the 25 `tests/evidence_auth` conformance tests.

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
