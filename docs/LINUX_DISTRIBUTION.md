# TAKO Native Linux Distribution (`RX-001`)

> **Release Availability Notice:** The `v0.1.0a2` release and `main`-hosted `install.sh` bootstrap are prepared in `M028A` and become publicly operational only after `M028B` qualification and release publication. Until `v0.1.0a2` is published, use `--version v0.1.0a1` with a local `install.sh` checkout or the `v0.1.0a1` Researcher Starter Kit.

## 1. Installation (Target Experience for `v0.1.0a2+`)

### Single-Command Bootstrap
```bash
curl -fsSL https://raw.githubusercontent.com/mmontielpz/tako/main/install.sh | bash
```

### Download-Inspect-Execute Alternative (Recommended for Security Review)
```bash
curl -fsSL https://raw.githubusercontent.com/mmontielpz/tako/main/install.sh -o install-tako.sh
less install-tako.sh
bash install-tako.sh --version v0.1.0a2
```

### Offline & Recovery Option
The `TAKO-Researcher-Starter-Kit` ZIP archive remains supported for offline environments and recovery installs (`./install-linux.sh`).

## 2. Ownership Boundaries & Paths
| Path | Purpose | Owned by Installer | Touched by Uninstall |
|---|---|---|---|
| `${XDG_DATA_HOME:-~/.local/share}/tako/venv` | Isolated Python venv (`--no-deps`) | Yes | Yes (removed) |
| `${XDG_DATA_HOME:-~/.local/share}/tako/.tako-install-manifest` | Ownership & SHA-256 metadata | Yes | Yes (removed) |
| `~/.local/bin/tako` | Symlink to `$TAKO_HOME/venv/bin/tako` | Only if pointing into `$TAKO_HOME/venv` | Yes (only if owned symlink) |
| `~/.bashrc`, `~/.zshrc`, `~/.config/fish/config.fish` | User shell profiles | **Never modified** | **Never touched** |
| User config (`TAKO_CONFIG_PATH`) & research workspaces | Config, credentials, `tako_workspace.json` | **Never modified** | **Never touched** |

## 3. Update & Uninstall
```bash
tako update --version v0.1.0a2
tako uninstall
```
- `tako update` downloads `SHA256SUMS` and the target wheel over HTTPS, verifies SHA-256 before modifying the active venv, and restores the prior installation automatically if venv setup or smoke verification fails.
- `tako uninstall` removes only the owned `tako` symlink and `TAKO_HOME` venv/manifest.

## 4. Linux Support Matrix
| Platform | Status | Notes |
|---|---|---|
| CachyOS Linux (`x86_64`, Python `>= 3.9`, Bash/Zsh/Fish) | `QUALIFIED_LOCAL_FIXTURE` (`M028A`) | Native CachyOS lifecycle & shell checks passed in isolated env |
| Ubuntu 24.04 LTS | `NOT_TESTED` | Targeted by `install.sh` (including `python3-venv` check); native qualification pending `M028B` |
| Windows / macOS | `FROZEN` / `OUT_OF_SCOPE` | Historical Windows CI preserved; no new Windows/macOS distribution work |

## 5. Security & Integrity Trust Model
- Remote downloads require `https://` (`file://` supported only for local test fixtures).
- `install.sh` and `tako update` verify the wheel's SHA-256 against `SHA256SUMS` hosted alongside the GitHub release before installation.
- **Trust Model Disclosure:** Same-origin `SHA256SUMS` over HTTPS protects against transport corruption and partial downloads, **not** independent cryptographic publisher authentication (e.g., GPG or Sigstore).

## 6. `RX-001` Experience Record
- **Observed Friction (`v0.1.0a1`):** Owner rejected manual ZIP download, extraction, `SHA256SUMS` check, `./install-linux.sh`, and manual `.tako-venv` activation during initial onboarding.
- **Target Flow:** Single `curl ... | bash` (or download-inspect-execute) installing into `~/.local/share/tako` with `~/.local/bin/tako` symlink working directly in Bash, Zsh, and Fish, plus `tako update` and `tako uninstall`.
- **Human Validation Status:** `NOT_TESTED` (awaiting owner re-onboarding after `M028B` release).
