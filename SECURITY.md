# Security Policy

TAKO is an experimental open-source Agentic Research Development Environment (ARDE) CLI (`0.1.0a2` on `develop`, hosted at `https://github.com/mmontielpz/tako`). See [Security Limitations](docs/architecture/SECURITY_LIMITATIONS.md) for current trust boundaries and known rollback limitations.

## Reporting a vulnerability

Do not open a public issue for a security problem. Report security vulnerabilities privately via GitHub's **Security** tab ("Security" → "Report a vulnerability") on `mmontielpz/tako`.

## Scope

- Accidentally committed secrets or private data.
- Unsafe defaults in configuration, CLI permissions, isolation, or secret handling.
- Vulnerabilities in Python CLI modules, Rust engine/bridge code, or packaging/CI workflows.

## Principles

Secure by default: explicit permissions, no secrets in user config or research workspaces, human-controlled authorization for paid/live operations, and honest reporting of known limitations.
