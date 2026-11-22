# TAKO ARDE Architecture

TAKO preserves a single shared **Python Scientific Core** (`tools/`) paired with a future **Rust/Tauri Desktop Shell** (`P7`) communicating over explicit versioned JSON contracts. Scientific algorithms and Tier 1 quality gates are never duplicated between CLI and GUI.

## 1. ARDE System Architecture
```mermaid
flowchart TB
  GUI["TAKO Desktop GUI (Rust/Tauri — P7 Planned)"]
  CLI["tako CLI (First-Class Interface — Active)"]
  ORCH["Workflow & Progress Engine (JSON Events)"]
  CORE["Shared Python Scientific Core (Frozen Integrity Gates)"]
  AI["CLI-First AI Adapters: AGY, Codex, Claude Code, Copilot"]
  DISC["Discovery Providers: OpenAlex, arXiv, Crossref, Semantic Scholar"]
  WS["Local Workspace (tako_workspace.json, BibTeX, CSV, MD)"]

  GUI -->|"Versioned JSON Bridge"| ORCH
  CLI --> ORCH
  ORCH --> CORE
  CORE --> DISC
  ORCH --> AI
  CORE --> WS
```

## 2. AI Provider & Authorization Separation
- **CLI-First AI Targets:** Antigravity CLI (`agy`), Codex CLI (`codex`), Claude Code (`claude`), GitHub Copilot CLI (`copilot`).
- **No Direct LLM API Key Required:** Standard workflows use existing authenticated local CLI sessions (`subscription_cli` mode), never silently invoking nested agents or falling back to paid APIs.
- **Explicit Metered API Separation:** Direct metered LLM APIs (e.g., `OPENAI_API_KEY`) remain strictly gated behind `--allow-paid-api`.

## 3. Research Companion Interaction
```mermaid
sequenceDiagram
  actor R as Researcher
  participant UI as TAKO Workspace / CLI
  participant C as Research Companion (CLI-First AI)
  participant K as Verified Workspace Knowledge

  R->>UI: Ask question / select passage / start SOTA
  UI->>K: Retrieve verified references & locators
  K-->>UI: Grounded passages + uncertainty flags
  UI->>C: Bounded prompt with verified context
  C-->>UI: Evidence-grounded response (or explicit abstention)
  UI-->>R: Display citations, locators & approval controls
```

## 4. Scientific Knowledge & Evidence Lifecycle
```mermaid
stateDiagram-v2
  [*] --> RawCandidate: Discovery Search
  RawCandidate --> Deduplicated: Canonical ID / DOI Check
  Deduplicated --> BibVerified: OpenAlex / Crossref Metadata
  Deduplicated --> CandidateOnly: Unresolved DOI (references_candidates.bib)
  BibVerified --> AbstractOnly: No Full Text
  BibVerified --> EvidenceGrounded: Local Full Text + Locator
  AbstractOnly --> ExploratoryMap: INSUFFICIENT_EVIDENCE Gate
  EvidenceGrounded --> SupportedClaim: Claim + Passage Locator
```
