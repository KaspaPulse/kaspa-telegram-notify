# Contributing to Kaspa Pulse

Thank you for improving Kaspa Pulse. Keep changes focused, reviewable, and compatible with the repository-pinned toolchain and declared package MSRV.

## Development baseline

- Use the Rust toolchain pinned by `rust-toolchain.toml`.
- The product MSRV is declared by `Cargo.toml`.
- Rust Edition 2024 is used throughout the owned Rust code.
- PostgreSQL is required for database-backed integration validation.
- Use Docker/Compose when changing container or runtime behavior.

Source changes, builds, tests, dependency work, and packaging belong in a trusted non-production development environment or protected GitHub Actions. Production is not a development or CI environment.

## Before opening a pull request

Run the core quality gates:

```bash
cargo fmt --all -- --check
cargo check --locked --all-targets --all-features
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
```

Run the repository policy and proof gates relevant to the change:

```bash
cargo xtask security environment-boundary
cargo xtask security documentation
cargo xtask security advisories --max-age-days 45
cargo xtask proof verify
```

For dependency or supply-chain changes, also run:

```bash
cargo audit
cargo deny check
cargo machete
```

If container/runtime behavior changes, build and smoke-test the production image locally as qualification. Local images and locally rebuilt binaries are not canonical release artifacts.

## Pull request expectations

- Explain the problem, security or reliability implications, and chosen approach.
- Keep unrelated refactors out of the same PR.
- Add or update positive and negative tests for behavior or policy changes.
- Do not commit secrets, tokens, production credentials, private user data, or private operational topology.
- Keep `Cargo.lock` synchronized with intentional dependency changes.
- Prefer immutable commit SHAs for third-party GitHub Actions.
- Preserve least-privilege workflow permissions and runtime configuration.
- Preserve required reproducibility and proof inputs.

## Release and deployment boundary

Canonical release artifacts come only from the protected attested GitHub Actions release workflow. A local build is for development or qualification; it must not be presented as a project release or substituted for published release bytes.

See [SUPPLY_CHAIN.md](SUPPLY_CHAIN.md) for release verification.

## Security issues

Do not disclose an unpatched vulnerability in a public issue. Follow [SECURITY.md](SECURITY.md) for private reporting.
