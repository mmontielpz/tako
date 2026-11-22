# TAKO Researcher Preview — Windows Quickstart (`0.0.1.dev3`)

**Experimental research software. No Administrator privileges, PowerShell ExecutionPolicy changes, venv activation, or Rust compilation required.**  
**Platform Notice:** Validated on Linux (`x86_64`, Python `3.14.10`); Windows runtime is `NOT_TESTED`.

## Prerequisites
- **Python 3.9+** (`py --version`)
- **Internet access** for live OpenAlex queries (`tako sota` / `--verify-bib`)
- **Authenticated Copilot CLI** (`copilot login`) for AI-native `copilot-cli` experiments

## 1. Option A — Direct Wheel Download & Install (Recommended)
Download `tako_researcher_preview-0.0.1.dev3-py3-none-any.whl` from [v0.0.1.dev3](https://github.com/mmontielpz/tako/releases/tag/v0.0.1.dev3) (`SHA-256: f2fa79ff14878595657fa85d67f1d60fbf0ac7a3f74c9b90aa326f17f8bb6606`), then run:
```powershell
py -m venv .tako-venv
.\.tako-venv\Scripts\python.exe -m pip install --no-index --no-deps .\tako_researcher_preview-0.0.1.dev3-py3-none-any.whl
```

## 1. Option B — Install Directly from Branch
```powershell
git clone --branch experiment/researcher-preview --single-branch https://github.com/mmontielpz/tako.git tako-preview
cd tako-preview
py -m venv .tako-venv
.\.tako-venv\Scripts\python.exe -m pip install --no-deps .
```

## 2. Check Environment & Providers (`doctor`)
```powershell
.\.tako-venv\Scripts\tako.exe --version
.\.tako-venv\Scripts\tako.exe doctor
```

## 3. Offline Fixture Experiment (No Internet or API Key)
```powershell
.\.tako-venv\Scripts\tako.exe sota-ai `
  --topic "Quantum Reservoir Computing" `
  --objective "Offline fixture test" `
  --provider fixture `
  --with-reviewer `
  --out-dir .\experiments\fixture
```

## 4. OpenAlex SOTA Discovery (No LLM Credentials Required)
```powershell
.\.tako-venv\Scripts\tako.exe sota `
  --topic "Quantum Reservoir Computing" `
  --objective "Identify foundational classical and quantum reservoir computing works, important methodological developments, contributions and limitations." `
  --max-papers 10 `
  --seed "Classical Reservoir Computing" `
  --seed "Echo State Networks" `
  --seed "Liquid State Machines" `
  --seed "Quantum Reservoir Computing" `
  --seed "Quantum circuit architectures" `
  --out-dir .\experiments\openalex
```

## 5. Full Copilot Researcher + Analyst + Reviewer Experiment
Ensure `copilot login` is authenticated, then pass `--allow-paid-api`:
```powershell
.\.tako-venv\Scripts\tako.exe sota-ai `
  --topic "Quantum Reservoir Computing" `
  --objective "Identify foundational classical and quantum reservoir computing works, important methodological developments, contributions, reported results and limitations." `
  --provider copilot-cli `
  --allow-paid-api `
  --verify-bib `
  --with-reviewer `
  --out-dir .\experiments\copilot
```

## 6. Using Your Own Research Topic & Objective
Replace `--topic`, `--objective`, `--seed`, and `--out-dir` with your own domain:
```powershell
.\.tako-venv\Scripts\tako.exe sota-ai `
  --topic "<Your Research Topic>" `
  --objective "<Your specific research question or comparison goal>" `
  --provider copilot-cli `
  --allow-paid-api `
  --verify-bib `
  --with-reviewer `
  --out-dir .\experiments\my-topic
```

## Expected Outputs & Feedback
Each `sota-ai` run writes `sota_report.md`, `literature_matrix.csv`, `references.bib`, `review_report.md`, and `stage_01`–`stage_04` JSONs inside `--out-dir`.  
Submit feedback at https://github.com/mmontielpz/tako/issues (**PUBLIC repository**: never post credentials or confidential research).
