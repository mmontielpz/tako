# Contributing to TAKO

TAKO is pre-alpha. There is no application code yet, so useful contributions right now are documentation review, product and architecture feedback, and issues describing real engineering needs.

## Before you start

- Read [README.md](README.md), [AGENTS.md](AGENTS.md) and [docs/architecture/OPEN_SOURCE_BOUNDARY.md](docs/architecture/OPEN_SOURCE_BOUNDARY.md).
- Open an issue before large changes.
- Do not contribute private, proprietary or third-party code you are not licensed to share, and never include secrets.

## Making a change

1. Fork and branch from `main`.
2. Keep the change small and focused; avoid unrelated refactoring.
3. State acceptance criteria in the pull request.
4. Run `scripts/check.sh` and report the real result.
5. Open a pull request using the template.

## Licensing and contribution terms

TAKO is licensed under the [Apache License 2.0](LICENSE). There is no separate CLA.

Per Section 5 of the license, any contribution you intentionally submit is licensed under Apache-2.0 (inbound = outbound), unless you explicitly state otherwise. By opening a pull request you confirm that you have the right to submit the work under these terms. Do not submit code copied from sources whose license is incompatible with Apache-2.0, or from private/proprietary projects.

Sign-off (`git commit -s`, DCO) is welcome but not required.

## Conduct and security

See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) and [SECURITY.md](SECURITY.md).
