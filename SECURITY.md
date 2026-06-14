# Security Policy

## Supported Versions

| Version | Supported |
| ------- | --------- |
| latest  | ✅        |

Only the latest published version on [crates.io](https://crates.io/crates/steplock-core) receives security fixes.

## Reporting a Vulnerability

Please **do not** open a public GitHub issue for security vulnerabilities.

Report by emailing the maintainers at the address listed on the crates.io page, or open a [GitHub private security advisory](https://github.com/polyhook/steplock/security/advisories/new).

Include:

- A description of the vulnerability and its potential impact
- Steps to reproduce or a proof-of-concept
- The version(s) affected

You can expect an initial response within 72 hours. We will coordinate a fix and disclosure timeline with you.

## Scope

steplock runs as a local CLI tool invoked by AI coding agents. It reads files from `.steplock/` in the current repository and writes `state.json` and shell scripts under `.steplock/sessions/`. It does **not** make network requests or handle untrusted remote input in production deployments.

Known limitations relevant to security:

- CEL expressions in `config.toml` are evaluated by the host process; malicious checklist configs can run arbitrary CEL expressions (but not arbitrary shell commands).
- Shell scripts written to `.steplock/sessions/` are intended to be run by the agent in the same repo context; they do not accept external input.
