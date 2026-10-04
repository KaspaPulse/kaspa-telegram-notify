<div align="center">

# 🦀 Kaspa Pulse

### Community mining alerts for Kaspa solo miners

[![Rust](https://img.shields.io/badge/Rust-1.99.0-orange.svg?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
[![CI](https://github.com/KaspaPulse/kaspa-telegram-notify/actions/workflows/rust-ci.yml/badge.svg)](https://github.com/KaspaPulse/kaspa-telegram-notify/actions/workflows/rust-ci.yml)
[![Latest Release](https://img.shields.io/github/v/release/KaspaPulse/kaspa-telegram-notify?style=for-the-badge)](https://github.com/KaspaPulse/kaspa-telegram-notify/releases)
[![License](https://img.shields.io/badge/License-MIT-green.svg?style=for-the-badge)](LICENSE)

A Rust service that tracks Kaspa wallets and solo-mining rewards, persists state in PostgreSQL, and delivers operational and mining alerts through Telegram.

</div>

## What it does

Kaspa Pulse connects to a Kaspa node over wRPC/WebSocket, maintains wallet and reward state in PostgreSQL, evaluates confirmed mining rewards and BlockDAG acceptance, and queues Telegram delivery with retry and deduplication controls.

It is intended for operators who want a small, auditable notification service rather than a general-purpose mining platform.

## Key capabilities

- Track multiple Kaspa wallets with per-user isolation.
- Detect and confirm solo-mining rewards.
- Analyze BlockDAG acceptance and network state.
- Persist wallet, event, deduplication, and delivery state in PostgreSQL.
- Deliver Telegram alerts through a durable queue with retry/backoff.
- Expose local health, readiness, and metrics endpoints.
- Apply actor-scoped authorization and rate limits to administrative actions.
- Run reproducible Rust-native operational qualification and security gates.
- Produce immutable GitHub release artifacts with checksums, SBOMs, provenance, and attestations.

## Quick start

Use the repository-pinned Rust toolchain. The product MSRV is declared in `Cargo.toml`.

Copy the example configuration:

```bash
cp .env.example .env
```

Set at least:

```env
BOT_TOKEN=PUT_YOUR_TELEGRAM_BOT_TOKEN_HERE
ADMIN_USER_ID=PUT_YOUR_TELEGRAM_ADMIN_USER_ID_HERE
ADMIN_CHAT_ID=PUT_THE_SAME_PRIVATE_CHAT_ID_HERE

NODE_URL_01=wss://your-kaspa-node.example.com/json
DATABASE_URL=postgres://kaspa_pulse_app:PUT_APP_PASSWORD_HERE@127.0.0.1:5433/kaspa_dev?sslmode=disable

APP_ENV=production
SQLX_OFFLINE=true
ALLOW_RUNTIME_SCHEMA_ENSURE=false
ENABLE_TELEGRAM_DELIVERY_QUEUE=true
ENABLE_ALERT_DELIVERY=true
```

`ADMIN_ID` remains a backward-compatible fallback. New configurations should use the explicit user/chat identity fields.

See [.env.example](.env.example) for the complete typed configuration surface.

## Architecture

```text
Telegram
   │
   ▼
command / callback boundary
   │
   ▼
application use cases ───────► Kaspa wRPC
   │
   ▼
PostgreSQL
   │
   ├── wallet and reward state
   ├── deduplication and events
   └── durable Telegram delivery queue
```

The owned executable implementation is Rust. Third-party native/FFI dependencies are permitted only when discovered, classified, and explicitly approved by repository policy.

## Running locally

Core developer checks:

```bash
cargo fmt --all -- --check
cargo check --locked --all-targets --all-features
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
```

Run the application after configuring the required environment:

```bash
cargo run --locked --release
```

Database schema changes belong in `migrations/`. Production runtime should use a least-privilege PostgreSQL role; see [Database security](docs/security/DATABASE_SECURITY.md).

## Container testing

Local container builds are **qualification artifacts only**:

```bash
docker build --pull -t kaspa-pulse:local .
docker compose up -d --build
```

These commands are useful for validating container/runtime behavior. They do **not** create a canonical project release and a locally rebuilt binary or image must not substitute for published release bytes.

The container runs non-root on Debian Trixie, uses a multi-stage Rust build, and keeps operational endpoints suitable for loopback or reverse-proxy use.

## Operational qualification

The Rust-native qualification harness lives under `xtask/src/opqual/`, with deterministic scenario inputs under `opqual/`.

It uses task-owned synthetic identities/data and isolated resources. Local material qualification requires an explicit non-production opt-in and remains fail closed on inherited credentials or ambiguous resource ownership.

See [Operational qualification](docs/OPERATIONAL_QUALIFICATION.md) for dry-run, material-run, resume, cleanup, and evidence semantics.

## Production releases

The canonical project artifact source is the protected, attested GitHub Actions release workflow.

Production deployment must use the **exact published artifact bytes** after verifying:

- SHA-256 checksum;
- expected source commit;
- canonical signer workflow;
- artifact attestation/provenance;
- target architecture and release manifest as applicable.

A local build is never a release substitute.

Browse [GitHub Releases](https://github.com/KaspaPulse/kaspa-telegram-notify/releases) and read [Supply-chain security and release verification](SUPPLY_CHAIN.md) before deploying a published artifact.

## Release verification

For a downloaded release archive, verify its checksum and GitHub attestation as documented in [SUPPLY_CHAIN.md](SUPPLY_CHAIN.md). Verification is fail closed: do not install or run an artifact when its checksum, source identity, signer workflow, or attestation cannot be established.

The project records provenance but does not claim an external SLSA level solely because attestations exist.

## Security

Repository security controls include:

- locked Rust builds and strict Clippy;
- CodeQL and secret scanning;
- OSV, Cargo Audit, and Cargo Deny dependency checks;
- immutable GitHub Action references;
- dependency review;
- fuzzing and hermetic E2E coverage;
- machine-readable Rust/native/supply-chain proof artifacts;
- time-bounded, fail-closed advisory exceptions.

The canonical machine-readable policy is `proof/policy.toml`. Active exception rationale is summarized in [SECURITY_ADVISORIES.md](SECURITY_ADVISORIES.md).

Report unpatched vulnerabilities privately as described in [SECURITY.md](SECURITY.md).

## Documentation

- [Supply-chain verification](SUPPLY_CHAIN.md)
- [Security policy](SECURITY.md)
- [Active advisory exceptions](SECURITY_ADVISORIES.md)
- [Security control map](docs/security/CONTROL_MAP.md)
- [Database security](docs/security/DATABASE_SECURITY.md)
- [Operational qualification](docs/OPERATIONAL_QUALIFICATION.md)
- [Fuzzing](docs/FUZZING.md)
- [External contract E2E](docs/EXTERNAL_CONTRACT_E2E.md)

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for the supported workflow, local quality gates, security expectations, and pull-request requirements. Coding agents should also follow [AGENTS.md](AGENTS.md).

## Support

Kaspa donation address:

```text
kaspa:qz0yqq8z3twwgg7lq2mjzg6w4edqys45w2wslz7tym2tc6s84580vvx9zr44g
```

## License

Kaspa Pulse is licensed under the [MIT License](LICENSE).
