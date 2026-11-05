# Dependency security review — 5 November 2026

Last automated review: **2026-11-05**

**Scope:** Kaspa Pulse clean-root Rust source, crate 1.2.10. This review applies
to the **unreleased release-qualification candidate**, not to the running
production binary or the existing GitHub `v2.0.0-rc.1` source snapshot.

## Current evidence

- `cargo audit` using a refreshed RustSec advisory database: **0 vulnerable
  advisories**, **8 unmaintained notices**, **0 yanked packages**.
- `cargo deny --offline check`: **PASS**, all categories (advisories, bans,
  licenses, and sources), **with exact-ID exceptions for eight upstream
  unmaintained notices** listed below.
- Existing `osv-scanner.toml` exemptions that expired on **22 September 2026**
  were removed. **No OSV finding is ignored** in the current configuration.
  An independent fresh OSV scan is still required for stable-release acceptance.
- This review is tied to the resolved `Cargo.lock`, **not** a claim that no
  future vulnerabilities exist. Never edit the lockfile without rerunning
  security checks and compatibility tests.

## Patched advisories (zero unresolved vulnerabilities in refreshed RustSec)

| Resolved package | Old | Patched |
| --- | --- | --- |
| `ctutils` | 0.4.2 | **0.4.5** |
| `quinn-proto` | 0.11.16 | **0.11.19** |
| `rustls` | 0.23.45 | **0.23.46** |
| `rustls-webpki` | 0.103.15 | **0.103.16** |
| `webpki-roots` | 1.0.9 | **1.0.10** |
| `webpki-root-certs` | 1.0.9 | **1.0.10** |
| `tokio` | 1.53.1 | **1.53.3** (RUSTSEC-2026-0386; unsound Unix `AsyncFd` constructors) |
| `async-executor` | 1.14.0 (yanked) | **1.13.1** (not yanked; **downgrade** in compatible series) |

RustSec originally reported **13 vulnerability advisories** against the old
locked graph. `cargo deny` subsequently identified the separate Tokio
**RUSTSEC-2026-0386 unsoundness advisory**, also addressed by a patched version.
These are fixed via actual dependency updates, not ignore rules.

## Reviewed transitive maintenance warnings

`deny.toml` has **exactly eight** advisory-ID exclusions solely for known
**unmaintained** transitive dependencies; no patched exploit/unsoundness
advisory is excluded. Exclusions are recorded, not silently suppressed.

| RustSec | Package | Transitive path | Plan |
| --- | --- | --- | --- |
| RUSTSEC-2024-0388 | derivative | Kaspa txscript → Arkworks | Replace via reviewed Kaspa SDK update |
| RUSTSEC-2026-0173 | proc-macro-error2 | Teloxide → aquamarine (build) | Track maintainer replacement and future compiler compatibility |
| RUSTSEC-2024-0449 | unsafe-libyaml | Kaspa core → log4rs → serde_yaml | Upstream migration off unmaintained YAML |
| RUSTSEC-2025-0052 | async-std | Kaspa workflow-core | Upstream runtime removal |
| RUSTSEC-2024-0375 | atty | Kaspa txscript → hexplay | Upstream replacement |
| RUSTSEC-2024-0384 | instant | Kaspa workflow-core | Upstream replacement |
| RUSTSEC-2024-0436 | paste | Kaspa SDK and risc0-zkp macros | Upstream replacement |
| RUSTSEC-2024-0370 | proc-macro-error | Kaspa RPC/workflow-core macros | Upstream replacement |

With **zero cargo-audit ignored IDs**, the refreshed `cargo audit` reports
all **eight** upstream unmaintained notices listed above. `cargo deny`
applies the eight exact-ID exclusions as a separately documented maintenance
policy, without waiving patched exploit/unsoundness vulnerabilities.

**Review expires 20 December 2026, or immediately on any dependency or RustSec
advisory change, whichever happens first.** Do not extend the review by simply
changing the date; recheck full reverse dependency trees and upstream releases.

## Independent acceptance and residual risk

The pinned Kaspa SDK remains **2.0.1** at Git revision
`cfafeb4c093fa37a303f1b9f19c58f986b870ce3`. We deliberately
did **not** merge the older GitHub tree or update to a different SDK major/minor
without isolated compatibility evidence.

A cleared `cargo audit` and a reviewed `cargo deny` do **not** prove that
the release is deployable. Additional gates: GitHub CI, PostgreSQL migration and
rollback, English/Arabic shared-wallet messages, release packaging on the
target architecture, container smoke, and signing/SBOM provenance.

To verify on the candidate with stable Rust 1.99.1:

```bash
cargo audit
cargo deny check
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --lib
```

No production passwords, database records, Telegram tokens, or secret scan
matches may be committed to this repository.
