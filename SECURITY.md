# Security Policy

TAKO is pre-alpha and contains no executable application code yet. Security still matters for repository hygiene and for the design of future components.

## Reporting a vulnerability

Do not open a public issue for a security problem. Use GitHub's private vulnerability reporting ("Security" tab → "Report a vulnerability") once the repository is published. Until then, there is no public reporting channel because the repository is not public.

## Scope

- Accidentally committed secrets or private data.
- Unsafe defaults in documented designs (permissions, isolation, secret handling).
- Vulnerabilities in code, once code exists.

## Principles

Secure by default: explicit permissions, isolation, no secrets in the repository, human-controlled authorization.
