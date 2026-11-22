# TAKO Researcher Quickstart (Windows & Ubuntu)

## 1. Prerequisites & Platform Compatibility
- **Minimum Supported Python:** Python `>= 3.9` (per `pyproject.toml`).
- **CI-Tested Matrix:** Ubuntu Linux (Python `3.9`, `3.12`, `3.13`, `3.14`) and Windows (Python `3.12`). Native macOS is `NOT_TESTED`.
- **Dependencies:** Zero external Python dependencies required (`--no-deps` wheel install inside an isolated virtual environment to avoid system Python / PEP 668 conflicts).

## 2. Install TAKO

### Native Linux Bootstrap (Recommended for CachyOS & Ubuntu 24.04 LTS)
See [Linux Distribution Guide](LINUX_DISTRIBUTION.md) for the download-inspect-execute alternative, `tako update`, `tako uninstall`, and integrity model:
```bash
curl -fsSL https://raw.githubusercontent.com/mmontielpz/tako/main/install.sh | bash
```

### Offline / Manual Virtual Environment Fallback
### Windows (PowerShell)
```powershell
python -m venv .tako-venv
.\.tako-venv\Scripts\python.exe -m pip install --no-deps tako_researcher_preview-0.1.0a2-py3-none-any.whl
.\.tako-venv\Scripts\tako.exe --version
```

### Ubuntu / Linux
```bash
python3 -m venv .tako-venv
.tako-venv/bin/pip install --no-deps tako_researcher_preview-0.1.0a2-py3-none-any.whl
.tako-venv/bin/tako --version
```

*(Activate `.tako-venv` or prefix commands with `.\.tako-venv\Scripts\tako.exe` on Windows / `.tako-venv/bin/tako` on Ubuntu.)*

## 3. Configure Your Research Profile
```bash
# Interactive wizard (recommended default: free)
tako setup

# Or non-interactive Free profile
tako setup --profile free --non-interactive

# Inspect active config, provider checklist, and readiness
tako setup --show
tako providers list
tako doctor
```

## 4. Initialize a Research Workspace
```bash
tako init --workspace ./my_research \
  --topic "Graph Neural Networks for Molecular Property Prediction" \
  --objective "Identify foundational message-passing methods, molecular property prediction architectures and reported methodological limitations"
```

## 5. Run Discovery & Export

### Path A — Offline Verification Workflow (`ai_out`, zero network calls)
```bash
tako sota-ai \
  --topic "Graph Neural Networks for Molecular Property Prediction" \
  --objective "Identify foundational message-passing methods, molecular property prediction architectures and reported methodological limitations" \
  --provider fixture \
  --out-dir ./my_research/ai_out

tako evidence --workspace ./my_research --from-workflow-dir ./my_research/ai_out --out-dir ./my_research/evidence_ai
tako status --workspace ./my_research
tako export --workspace ./my_research --out-dir ./my_research/export_ai --format all
```

### Path B — Bounded Live Open-Access Discovery Workflow (`sota_out`)
```bash
tako sota \
  --topic "Graph Neural Networks for Molecular Property Prediction" \
  --objective "Identify foundational message-passing methods, molecular property prediction architectures and reported methodological limitations" \
  --iterative \
  --sources openalex,arxiv,crossref \
  --max-papers 10 \
  --max-rounds 2 \
  --max-requests 4 \
  --max-http-requests 16 \
  --out-dir ./my_research/sota_out

tako evidence --workspace ./my_research --from-workflow-dir ./my_research/sota_out --out-dir ./my_research/evidence_sota
tako status --workspace ./my_research
tako export --workspace ./my_research --out-dir ./my_research/export_sota --format all
```
