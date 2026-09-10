# Contributing to Kaspa Pulse

Thank you for improving Kaspa Pulse. Keep changes focused, reviewable, and compatible with the supported Rust toolchain.

## Development baseline

- Rust 1.98.1 pinned development/CI toolchain (see `rust-toolchain.toml`); MSRV 1.97.1 (see `Cargo.toml`)
- Rust Edition 2024
- PostgreSQL 18 for CI validation
- Docker/Compose only when changing container or deployment behavior

## Before opening a pull request

Run the same quality gates used by CI:

```bash
cargo fmt --all -- --check
cargo check --locked --all-targets --all-features
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
```

For dependency or supply-chain changes, also run:

```bash
cargo audit
cargo deny check
cargo machete
```

If Docker or production runtime behavior changes, also build and smoke-test the production image.

## Pull request expectations

- Explain the problem and the chosen approach.
- Keep unrelated refactors out of the same PR.
- Add or update tests for behavior changes and bug fixes.
- Do not commit secrets, tokens, production credentials, or private user data.
- Keep `Cargo.lock` in sync with dependency changes.
- Prefer immutable commit SHAs for third-party GitHub Actions.
- Preserve least-privilege permissions in workflows and runtime configuration.

## Security issues

Do not disclose unpatched vulnerabilities in public issues. Follow `SECURITY.md` for private reporting.
