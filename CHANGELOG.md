# Changelog

All notable changes to the public TAKO repository are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [PEP 440](https://peps.python.org/pep-0440/) / [SemVer](https://semver.org/) for releases.

## [Unreleased] — `0.0.1.dev6` Candidate (Increment C: Evidence & Comparative SOTA)

### Added
- Evidence-grounded extraction, claim verification, comparative analysis, and Tier 1 quality gating (`tools/sota_evidence.py`), wired via `tako evidence`:
  - Evidence acquisition and availability classification (`acquire_evidence`) distinguishing `FULL_TEXT_LOCAL`, `ABSTRACT_BASED`, `METADATA_ONLY`, `UNAVAILABLE`, and `MODEL_RECALL` (no silent full-text fallback).
  - Structured scientific extraction (`normalize_extraction`) capturing study metadata, method, dataset, metric, result, limitations, and explicit uncertainty/absent-value semantics.
  - ResearchOS-adapted deterministic textual correspondence (`verify_textual_correspondence`) with provisional `0.85` / `0.50` thresholds, structured locators (`page`, `section`, `paragraph`, `line_range`, `char_range`), source-mismatch detection, and strict separation from semantic entailment (never auto-promoting `CLAIM_SUPPORT_STATUS` to `SUPPORTED`).
  - Traceable claim registry (`register_claims`, `claims_registry.csv`) enforcing `MODEL_RECALL` and `METADATA_ONLY` abstention.
  - Pairwise paper comparability (`compare_papers`, `comparisons.csv`, `comparative_synthesis.md`) classifying `DIRECTLY_COMPARABLE`, `CONDITIONALLY_COMPARABLE`, and `NOT_COMPARABLE` with explicit reasons and incompatibility notes.
  - Conservative research-gap assessment (`assess_research_gaps`, `research_gaps.csv`) separating `AUTHOR_REPORTED_LIMITATION`, `CORPUS_LIMITATION`, `MISSING_COMPARISON`, `RESEARCH_OPPORTUNITY`, `SPECULATIVE_HYPOTHESIS`, and `INSUFFICIENT_EVIDENCE`.
  - All 7 canonical M017A Tier 1 quality gates (`evaluate_tier1_quality`, `quality_report.json`): `QM-B02`, `QM-B03`, `QM-B04`, `QM-E03`, `QM-G04`, `QM-G05`, `QM-C03` with `PASS | FAIL | NOT_MEASURED | NOT_APPLICABLE | INSUFFICIENT_EVIDENCE` states.
  - Bounded remediation & downstream artifact regeneration (`run_evidence_and_comparative_pipeline`) and Minimum Viable SOTA report qualification (`EXPLORATORY_LITERATURE_MAP` vs `EVIDENCE_GROUNDED_COMPARATIVE_SOTA`, `sota_report.md`).
  - Offline scenario regression suite (`tests/sota_ai_native/test_evidence_comparative_sota.py`, `E01`–`E20`).

## [Unreleased] — `0.0.1.dev5` Candidate (Increment B: Discovery Intelligence)

### Added
- Bounded, coverage-aware iterative discovery engine (`tools/sota_discovery/engine.py`) with:
  - Inspectable research plan (`research_plan.json`) distinguishing user-provided vs. generated concept dimensions.
  - Review-first discovery seed step (`REVIEW_FIRST_APPLICABLE | ATTEMPTED | UNAVAILABLE | NOT_APPLICABLE`).
  - Bounded backward/forward citation exploration with cycle prevention and visited-node tracking.
  - Deterministic relevance screening (`RELEVANT | POTENTIALLY_RELEVANT | IRRELEVANT | INSUFFICIENT_EVIDENCE`) and 7-bucket state accounting.
  - Accepted-only coverage tracking (`coverage_report.json`), `ARDE-ALG-01` refinement on positive gain with insufficient coverage, and explicit termination reasons (`SATURATION`, `STAGNATION`, `BUDGET_EXHAUSTED`, `MAX_ROUNDS_REACHED`, `NO_VALID_QUERIES`, `PROVIDER_FAILURE`, `INSUFFICIENT_EVIDENCE`).
  - CLI `--iterative` mode in `tako sota` (`--max-rounds`, `--max-requests`, `--citation-depth`, `--no-review-first`) emitting `research_plan.json`, `discovery_trace.json`, `screening_decisions.csv`, and `coverage_report.json`.
  - Offline scenario regression suite (`tests/sota_discovery/test_discovery_intelligence.py`, `D01`–`D15`).

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
