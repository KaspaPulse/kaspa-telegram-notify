# Repository Engineering Policy

This file provides public engineering guidance for humans and coding agents working in this repository. The canonical machine-readable security policy is `proof/policy.toml`; this document must stay consistent with that policy but is not itself a machine-security authority.

## Engineering environment boundary

- Source edits, dependency resolution, Git mutations, builds, tests, security scans, and packaging must run in a trusted non-production development environment or in protected GitHub Actions.
- Production environments must not be used for source mutation, dependency resolution, CI execution, or source builds.
- If production verification exposes a defect, preserve the minimum evidence needed for diagnosis, keep or restore a safe runtime state, and return to a non-production development environment for the fix.
- Never patch application source in place on production.

## Release authority

- Local builds are qualification artifacts only.
- Canonical project releases are produced by the protected, attested GitHub Actions release workflow in `.github/workflows/release.yml`.
- A local rebuild must never substitute for a published release artifact.
- Production deployment artifacts must be the exact published release bytes after checksum, source identity, signer workflow, and attestation verification.
- Release verification is fail closed: do not install or run an artifact whose identity or provenance cannot be verified.

## Required engineering gates

For applicable changes, preserve and run the repository-native gates rather than weakening them:

```bash
cargo fmt --all -- --check
cargo check --locked --all-targets --all-features
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
cargo xtask security environment-boundary
cargo xtask security documentation
cargo xtask security advisories --max-age-days 45
cargo xtask proof verify
```

Dependency and supply-chain changes must also keep the independent audit, deny, OSV, secret-scan, CodeQL, dependency-review, and workflow-lint controls healthy.

## Repository safety

- Do not commit secrets, production credentials, private user data, runtime dumps, private deployment state, local task ledgers, or machine-specific recovery state.
- Preserve deterministic build and proof inputs such as `.sqlx/`, `Cargo.lock`, migrations, operational-qualification scenario data, and `proof/policy.toml`.
- Do not delete or consolidate security/workflow boundaries merely for visual simplicity.
- Unknown policy values and unclassified trust inputs fail closed.

## Continuity and side effects

Before repeating a material external effect, reconcile against the actual repository, CI, release, and runtime state. Preserve existing work, recover an already-started operation when possible, and avoid duplicate pushes, PRs, merges, tags, releases, deployments, or destructive history operations.

Operational continuity state belongs outside the tracked public repository.

This file grants no push, merge, release, deployment, or production authority by itself.
