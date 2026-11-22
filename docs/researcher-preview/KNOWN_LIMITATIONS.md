# Known Limitations (`0.0.1.dev6` Candidate)

1. **Windows Runtime Not Tested:** Built as `py3-none-any` and verified on `Linux x86_64` (Python `3.14.10`). No Windows host was available (`WINDOWS_RUNTIME_STATUS=NOT_TESTED`).
2. **Verified vs. Candidate Bibliography:** `references.bib` contains only bibliographically verified records (`BIBLIOGRAPHIC_STATUS=VERIFIED`). Unverified or mismatched records are written to `references_candidates.bib`. An empty `references.bib` is valid when no identities qualify and does not imply a complete review.
3. **Evidence Availability vs. Claim Support:** `BIBLIOGRAPHIC_STATUS=VERIFIED` does not establish scientific claim support (`CLAIM_SUPPORT_STATUS=NOT_CHECKED`). `copilot-cli` outputs are clamped to `MODEL_RECALL`.
4. **Unknown Cost Telemetry:** `cost_usd` remains `UNKNOWN` when providers report `premiumRequests` or credits without monetary USD charges (`0.0` only for local `fixture`).
5. **Discovery Request Accounting:** In `tools/sota_discovery/engine.py`, `request_count` tracks logical citation expansions and query calls rather than exact underlying HTTP requests when a single expansion triggers multiple adapter calls.
6. **Provisional Textual Correspondence Thresholds:** `verify_textual_correspondence` uses provisional `0.85` (`FUZZY_MATCH`) and `0.50` (`NEEDS_REVIEW`) sequence similarity thresholds adapted from ResearchOS. Textual correspondence checks lexical overlap only—not semantic entailment—and never auto-promotes `CLAIM_SUPPORT_STATUS` to `SUPPORTED`.
7. **Codex & Gemini:** Marked `NOT_IMPLEMENTED` in `tako sota-ai`.
