# TAKO Researcher Quickstart (Windows & Linux)

## 1. Prerequisites
- Python 3.9–3.14 (`python --version` or `py -3 --version` on Windows).
- Zero external Python dependencies required (`--no-deps` wheel install).

## 2. Install TAKO
```powershell
# Windows (PowerShell)
python -m pip install --no-deps tako_researcher_preview-0.0.1.dev10-py3-none-any.whl
tako --version
```
```bash
# Linux / macOS
python3 -m pip install --no-deps tako_researcher_preview-0.0.1.dev10-py3-none-any.whl
tako --version
```

## 3. Configure Your Research Profile
```bash
# Interactive wizard (recommended default: free)
tako setup

# Non-interactive Free profile
tako setup --profile free --non-interactive

# Inspect active config and provider checklist
tako setup --show
tako providers list
tako doctor
```

## 4. Run Your First Research Workspace
```bash
tako init --workspace ./my_research --topic "Graph Neural Networks for Molecular Property Prediction" --objective "Survey foundational methods and limitations"

# Offline verification (no network calls)
tako sota-ai --topic "Graph Neural Networks for Molecular Property Prediction" --objective "Survey foundational methods and limitations" --provider fixture --out-dir ./my_research/ai_out

# Or bounded live open-access discovery (when ready for network access)
tako sota --topic "Graph Neural Networks for Molecular Property Prediction" --objective "Survey foundational methods and limitations" --iterative --sources openalex,arxiv,crossref --max-papers 10 --max-rounds 2 --max-requests 4 --max-http-requests 12 --out-dir ./my_research/sota_out

# Evidence & export
tako evidence --workspace ./my_research --from-workflow-dir ./my_research/sota_out --out-dir ./my_research/evidence_out
tako status --workspace ./my_research
tako export --workspace ./my_research --out-dir ./my_research/export_out --format all
```
