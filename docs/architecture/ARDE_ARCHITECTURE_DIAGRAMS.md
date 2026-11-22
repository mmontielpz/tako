# TAKO ARDE Architecture Diagrams

## 01 — Researcher Workflow
```mermaid
flowchart LR
  Install["Install Wheel (venv, --no-deps)"] --> Setup["tako setup (free/hybrid/offline)"]
  Setup --> Doctor["tako doctor & providers list"]
  Doctor --> Init["tako init (tako_workspace.json)"]
  Init --> Discover["tako sota / tako sota-ai"]
  Discover --> Evidence["tako evidence (Tier 1 gating)"]
  Evidence --> Export["tako export (MD, CSV, JSON, BibTeX, RIS)"]
```

## 02 — Multi-Source Discovery
```mermaid
flowchart TD
  CLI["tako sota --iterative --sources ..."] --> Tracker["Global HttpRequestTracker (--max-http-requests, pacing, Retry-After)"]
  Tracker --> OA["OpenAlex (LIVE_QUALIFIED)"]
  Tracker --> AX["arXiv Atom API (LIVE_QUALIFIED)"]
  Tracker --> CR["Crossref REST API (LIVE_QUALIFIED)"]
  Tracker --> S2["Semantic Scholar (RATE_LIMITED without key)"]
  GS["Google Scholar (MANUAL_ASSISTED URLs only)"] -. manual import .-> Dedup
  OA & AX & CR & S2 --> Dedup["DOI / arXiv ID Dedup & Preprint/Published Linking"]
  Dedup --> Screen["Multi-term Screening -> Accepted / Limited-Use / Excluded"]
```

## 03 — Evidence and Scientific Integrity
```mermaid
flowchart LR
  Inputs["Discovery Outputs / Local .txt .md .xml .json .bib .ris"] --> Verify["Bibliographic Identity Verification"]
  Verify --> Verified["references.bib (VERIFIED only)"]
  Verify --> Cand["references_candidates.bib (UNVERIFIABLE / MISMATCH)"]
  Inputs --> Claims["Claim Support & Comparability Assessment"]
  Claims --> Gate["Conservative Verdict: EXPLORATORY_LITERATURE_MAP / INSUFFICIENT_EVIDENCE when warranted"]
```

## 04 — External AI Assistant Integration
```mermaid
flowchart LR
  Researcher["Researcher"] --> Assistant["External Assistant (AGY / GitHub Copilot CLI / Other)"]
  Assistant -- "Runs CLI & reads --help / --json" --> TAKO["tako CLI (setup, init, sota, evidence, status, export)"]
  TAKO -- "Structured MD / CSV / JSON / BibTeX" --> Assistant
  Assistant -- "Grounded summary + explicit limitations" --> Researcher
```

## 05 — Current CLI and Future Desktop Boundary
```mermaid
flowchart TB
  subgraph Implemented["Implemented Today (CLI-First, 0.0.1.dev10)"]
    CLI["tako CLI (tools/cli.py, researcher_setup.py)"]
    Core["Shared Scientific Core (sota_discovery, sota_integrity, sota_evidence, research_workspace)"]
    CLI --> Core
  end
  subgraph Planned["Planned Desktop Interfaces (P7.1–P7.3)"]
    WinUI["TAKO Desktop — Windows (Tauri + UI)"]
    UbuUI["TAKO Desktop — Ubuntu (Tauri + UI)"]
    WinUI -. reuses contracts .-> Core
    UbuUI -. reuses contracts .-> Core
  end
```

## 06 — High-Level Product Roadmap
```mermaid
flowchart LR
  P4["P4: CLI Readiness (M024B–E Done -> M025 OSS -> M026 Starter Kit)"] --> P5["P5: Brother Researcher Pilot"]
  P5 --> P6["P6: Researcher Feedback & RX"]
  P6 --> P7["P7: TAKO Desktop (P7.1 Win -> P7.2 Refine -> P7.3 Ubuntu)"]
  P7 --> P8["P8: Research Ecosystem"]
```
