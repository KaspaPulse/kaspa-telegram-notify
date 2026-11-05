# Security Policy

## Supported versions

Security fixes are applied to the current `main` branch and the latest maintained release. Older releases may not receive backported fixes.

## Reporting a vulnerability

Do not open a public issue for an unpatched vulnerability.

Use GitHub's private Security Advisory reporting flow whenever it is available:

https://github.com/KaspaPulse/kaspa-telegram-notify/security/advisories/new

You can also start from the repository Security page:

https://github.com/KaspaPulse/kaspa-telegram-notify/security

If private vulnerability reporting is temporarily unavailable, contact the repository owner through an existing trusted private channel before public disclosure. Never place credentials, private keys, access tokens, or sensitive user data in a public issue.

Include, when possible:

- the affected version or commit;
- the impact and realistic attack scenario;
- a minimal reproduction or proof of concept;
- any suggested mitigation.

We aim to acknowledge a security report within **7 days**, provide an initial status update within **14 days**, and coordinate remediation and disclosure as quickly as practical. When a longer investigation is required, a **90-day** coordinated-disclosure target is the default upper bound unless active exploitation or another material risk requires a different timeline.

Reports should be validated privately, remediation coordinated, and a security advisory published when appropriate.

## Security automation

Dependency-sensitive changes are checked with `cargo audit`, `cargo deny`, and `cargo machete`; Rust application changes also pass formatting, locked compilation, strict Clippy, and tests. GitHub CodeQL performs Rust static analysis, and OpenSSF Scorecard continuously checks repository supply-chain posture.

Accepted upstream or transitive exceptions are documented in `SECURITY_ADVISORIES.md` and configured in `.cargo/audit.toml` / `deny.toml` as applicable.
