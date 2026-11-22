# TAKO Researcher Preview — Windows Quickstart (`0.0.1.dev2`)

**No Administrator privileges, PowerShell ExecutionPolicy changes, or Rust compilation required.**

## 1. Clone the Experimental Branch
```powershell
git clone --branch experiment/researcher-preview --single-branch https://github.com/mmontielpz/tako.git tako-preview
cd tako-preview
```

## 2. Create Python Virtual Environment & Install Directly from Branch
```powershell
py -m venv .tako-venv
.\.tako-venv\Scripts\python.exe -m pip install --no-deps .
```

## 3. Check Environment & Providers
```powershell
.\.tako-venv\Scripts\tako.exe --version
.\.tako-venv\Scripts\tako.exe doctor
```

## 4. Offline First-Use Experiment (Bundled Fixture)
```powershell
.\.tako-venv\Scripts\tako.exe sota-ai `
  --topic "Quantum Reservoir Computing" `
  --objective "Offline fixture test" `
  --provider fixture `
  --with-reviewer `
  --out-dir .\sota_fixture_test
```

## 5. Live OpenAlex SOTA Discovery (No LLM Credentials Required)
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
  --out-dir .\sota_output
```

## 6. Experiment with Authenticated Copilot or Claude CLI
Ensure `copilot login` (or `claude`) is authenticated in your session, then pass `--allow-paid-api`:
```powershell
.\.tako-venv\Scripts\tako.exe sota-ai `
  --topic "Quantum Reservoir Computing" `
  --objective "Identify foundational classical and quantum reservoir computing works, important methodological developments, contributions and limitations." `
  --provider copilot-cli `
  --allow-paid-api `
  --verify-bib `
  --out-dir .\sota_copilot_output
```
(Replace `--provider copilot-cli` with `--provider claude-cli` to use Claude Code CLI.)
