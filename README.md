<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/tako-header-dark.svg" />
    <source media="(prefers-color-scheme: light)" srcset="docs/assets/tako-header-light.svg" />
    <img alt="TAKO — Agentic Research Development Environment" src="docs/assets/tako-header-dark.svg" width="840" />
  </picture>
</p>

# TAKO

**From research questions to reproducible scientific knowledge.**

TAKO is an open-source, experimental **Agentic Research Development Environment (ARDE)** built for researchers and engineers who want bounded, traceable, evidence-grounded literature discovery and synthesis without sacrificing scientific integrity or local ownership.

> **Status:** Experimental CLI (`v0.1.0a2`). Qualified natively on **CachyOS Linux** (Python `3.14`); CI-tested on **Ubuntu 24.04 LTS** (Python `3.9`–`3.14`, native Ubuntu desktop `NOT_TESTED`) and **Windows** (Python `3.12`). Native macOS is `NOT_TESTED`. TAKO Desktop is **planned** (`P7.0–P7.4`).

---

## Why TAKO?

AI-assisted literature workflows often fabricate citations, conflate model recall with verified evidence, and run unbounded network or paid API calls. TAKO enforces:

- **Bounded execution:** Explicit `--max-http-requests`, `--max-requests`, `--max-rounds`, and `--max-papers` caps, plus provider-specific pacing and `Retry-After` handling.
- **Bibliographic verification:** Verified entries (`references.bib`) are strictly separated from unverified or ambiguous candidates (`references_candidates.bib`).
- **Transparent limitations:** When full-text evidence or claim support is absent, TAKO abstains (`EXPLORATORY_LITERATURE_MAP` / `INSUFFICIENT_EVIDENCE`) rather than manufacturing confidence.
- **Portable workspaces:** Deterministic `tako_workspace.json`, Markdown, CSV, JSON, BibTeX, and RIS exports with zero external Python runtime dependencies.

---

## Installation

### Native Linux CLI Bootstrap (CachyOS & Ubuntu 24.04 LTS)

> **Note:** See [Linux Distribution Guide](docs/LINUX_DISTRIBUTION.md) for ownership boundaries, version pinning (`--version v0.1.0a2`), security/integrity disclosures, and offline Starter Kit recovery.

```bash
# Single-command bootstrap (after v0.1.0a2 release)
curl -fsSL https://raw.githubusercontent.com/mmontielpz/tako/main/install.sh | bash
```

Download-inspect-execute alternative:

```bash
curl -fsSL https://raw.githubusercontent.com/mmontielpz/tako/main/install.sh -o install-tako.sh
less install-tako.sh
bash install-tako.sh
```

Update and uninstall:

```bash
tako update
tako uninstall
```

### Offline / Manual Virtual Environment Fallback (`--no-deps`, Python `>= 3.9`)

See [Researcher Quickstart](docs/RESEARCHER_QUICKSTART.md) or the `TAKO-Researcher-Starter-Kit` archive:

```powershell
# Windows (PowerShell)
python -m venv .tako-venv
.\.tako-venv\Scripts\python.exe -m pip install --no-deps tako_researcher_preview-0.1.0a2-py3-none-any.whl
.\.tako-venv\Scripts\tako.exe --version
```

```bash
# Linux (offline wheel / Starter Kit fallback)
python3 -m venv .tako-venv
.tako-venv/bin/pip install --no-deps tako_researcher_preview-0.1.0a2-py3-none-any.whl
.tako-venv/bin/tako --version
```

---

## Quickstart

```bash
# 1. Configure a research access profile (recommended default: free)
tako setup --profile free --non-interactive
tako providers list
tako doctor

# 2. Initialize a portable research workspace
tako init --workspace ./my_research \
  --topic "Graph Neural Networks for Molecular Property Prediction" \
  --objective "Identify foundational message-passing methods and reported limitations"

# 3a. Offline verification workflow (zero network calls)
tako sota-ai --topic "Graph Neural Networks for Molecular Property Prediction" \
  --objective "Identify foundational message-passing methods and reported limitations" \
  --provider fixture --out-dir ./my_research/ai_out
tako evidence --workspace ./my_research --from-workflow-dir ./my_research/ai_out --out-dir ./my_research/evidence_ai

# 3b. Or bounded live open-access discovery (OpenAlex + arXiv + Crossref)
tako sota --topic "Graph Neural Networks for Molecular Property Prediction" \
  --objective "Identify foundational message-passing methods and reported limitations" \
  --iterative --sources openalex,arxiv,crossref --max-papers 10 --max-rounds 2 \
  --max-requests 4 --max-http-requests 16 --out-dir ./my_research/sota_out
tako evidence --workspace ./my_research --from-workflow-dir ./my_research/sota_out --out-dir ./my_research/evidence_sota

# 4. Inspect status and export portable artifacts
tako status --workspace ./my_research
tako export --workspace ./my_research --out-dir ./my_research/export_out --format all
```

---

## Scientific Source & Assistant Support (`M024E` Verified)

| Source / Assistant | Status | Notes |
| --- | --- | --- |
| **OpenAlex** | `LIVE_QUALIFIED` | Default free open-access source (`tako sota`) |
| **arXiv** | `LIVE_QUALIFIED` | Atom 1.0 API, 1 req/3s pacing (`--sources arxiv`) |
| **Crossref** | `LIVE_QUALIFIED` | Polite pool REST API (`--sources crossref`) |
| **Semantic Scholar** | `RATE_LIMITED_UNQUALIFIED_WITHOUT_KEY` | Optional; anonymous shared pool hit HTTP `429` in `M024E` |
| **Google Scholar** | `MANUAL_ASSISTED` | Generates search URLs & imports user exports; zero scraping |
| **Copilot CLI** | `OPERATIONAL_WITH_LIMITATIONS` | Requires `--allow-paid-api`; tokens/USD `UNKNOWN`; CLI HTTP `NOT_OBSERVABLE` |
| **Antigravity (AGY)** | `EXTERNAL_CLI_ASSISTANT` | Operates `tako` CLI externally; not a native `sota-ai` provider |

Additional documented sources (Mendeley, Scopus, IEEE Xplore, Web of Science, PubMed, Europe PMC, CORE, Unpaywall, Zotero, ScienceDirect, Springer Nature, DBLP, OpenAIRE, OAI-PMH) appear in `tako providers list` as `DOCUMENTED_ONLY` and remain disabled until adapters are implemented. API key possession never implies verified institutional entitlement.

See [Agent Starter Prompts](docs/AGENT_STARTER_PROMPTS.md) for copy-paste prompts for AGY and GitHub Copilot.

---

## Scientific Integrity Guarantees & Limitations

- TAKO does **not** claim autonomous scientific discovery, complete literature coverage, guaranteed citation accuracy, or scientific superiority.
- **No PDF parsing:** Zero-dependency mode ingests `.txt`, `.md`, `.xml` (JATS), `.json`, `.bib`, and `.ris`; PDF ingestion is unsupported.
- **No semantic extraction:** Evidence processing normalizes supplied fields and metadata; unsupported claims remain `NOT_VERIFIED` or `INSUFFICIENT_EVIDENCE`.
- **No secrets in workspaces or config:** User preferences live in `%APPDATA%\tako\config.json` (Windows) or `~/.config/tako/config.json` (Linux); secrets are never stored in config or workspace files.

---

## Architecture & Roadmap

- **Architecture Diagrams:** See [ARDE Architecture Diagrams](docs/architecture/ARDE_ARCHITECTURE_DIAGRAMS.md) (Workflow, Multi-Source Discovery, Evidence & Integrity, Assistant Integration, CLI/Desktop Boundary, Roadmap) and [Branding & Provenance](docs/assets/BRANDING_AND_PROVENANCE.md).
- **Roadmap:** `P4` Closed (`v0.1.0a1`) → `P5` JetSeg Researcher Pilot (`CachyOS` CLI; `RX-001` native distribution `v0.1.0a2`) → `P6` Researcher Experience and Optimization → `P7.0–P7.4` TAKO Desktop (`LINUX_FIRST`: CachyOS then Ubuntu 24.04 LTS) → `P8` Research Ecosystem (Deferred). See [Roadmap](docs/product/ROADMAP.md) and [Current State](CURRENT_STATE.md).

---

## Contributing, Security & License

- Contributions: [CONTRIBUTING.md](CONTRIBUTING.md) · Code of Conduct: [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)
- Security & known rollback limitations: [SECURITY.md](SECURITY.md) · [Security Limitations](docs/architecture/SECURITY_LIMITATIONS.md)
- License: [Apache License 2.0](LICENSE) ([NOTICE](NOTICE), [0001-license.md](docs/decisions/0001-license.md)) · Citation: [CITATION.cff](CITATION.cff)
