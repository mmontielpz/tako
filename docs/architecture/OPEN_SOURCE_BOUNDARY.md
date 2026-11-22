# Open Source Boundary

TAKO is fully open source. **T-Ecosystem remains private.** TAKO must not require any private T service to operate.

## Excluded from this repository

- T-Runtime private implementation
- T-Platform private implementation
- T-Control and internal control-plane details
- T-SWARM
- T-WORKFORCE
- T-Collective Intelligence
- Private T-State and internal memory
- Proprietary prompts, strategies and execution policies
- Credentials, tokens, private datasets and infrastructure configuration

## Permitted

General engineering principles and publicly known design patterns may be implemented independently in TAKO, written from scratch.

## AIEF

AIEF is a candidate for consolidation into the TAKO Engineering Engine, **subject to an ownership, licensing and dependency audit (TAKO-002)**. No AIEF source is migrated in TAKO-001A. Observed during preflight (read-only): AIEF declares `Apache-2.0` in its `pyproject.toml` but has no LICENSE file; this must be resolved in the audit.

## Contributor rule

If you are unsure whether something belongs on the private side, do not add it. Open an issue describing the need in general terms instead.
