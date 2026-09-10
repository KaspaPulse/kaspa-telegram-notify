<div align="center">

# 🦀 Kaspa Pulse
### Community Mining Alerts for Kaspa Solo Miners

[![Rust](https://img.shields.io/badge/Rust-1.98.1-orange.svg?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
[![Edition](https://img.shields.io/badge/Rust%20Edition-2024-orange.svg?style=for-the-badge&logo=rust)](https://doc.rust-lang.org/edition-guide/rust-2024/)
[![Kaspa](https://img.shields.io/badge/Kaspa-Network-70D4CB.svg?style=for-the-badge)](https://kaspa.org/)
[![Database](https://img.shields.io/badge/Database-PostgreSQL-336791.svg?style=for-the-badge&logo=postgresql)](https://www.postgresql.org/)
[![License](https://img.shields.io/badge/License-MIT-green.svg?style=for-the-badge)](LICENSE)

A production-oriented Telegram bot for tracking Kaspa wallets, confirmed solo-mining rewards, wallet balances, BlockDAG/network metrics, and mining alerts.

</div>

---

## Overview

Kaspa Pulse is a Rust application that connects to a Kaspa node through wRPC/WebSocket and stores wallet, reward, deduplication, event, and delivery state in PostgreSQL.

The repository intentionally uses a small auditable stack. It does **not** add Node.js, npm, TypeScript, ESLint, or a JavaScript web framework because the application does not contain a JavaScript/TypeScript runtime that needs them.

### Production flow

```text
Telegram user
    ↓
Command / wallet input
    ↓
Validation + actor-scoped rate limits
    ↓
PostgreSQL wallet state
    ↓
UTXO monitor
    ↓
Reward confirmation gate
    ↓
DAG analysis
    ↓
Event log + wallet-scoped deduplication
    ↓
telegram_delivery_queue
    ↓
Telegram delivery worker
```

---

## Current platform baseline

- Rust **1.98.1** is the pinned development/CI toolchain in `rust-toolchain.toml`; crate MSRV remains **1.97.1** in `Cargo.toml`.
- Rust **Edition 2024**.
- PostgreSQL **18** validation baseline with PostgreSQL-only SQLx 0.9 feature selection.
- Teloxide 0.17 and Axum 0.8.
- Reqwest 0.13 with Rustls.
- `rusty-kaspa` dependencies pinned to the approved `v2.0.1` Git tag.
- Debian 13 (Trixie) production container.
- Non-root container runtime using UID/GID `10001`.

The crate has `publish = false` to prevent accidental publication to crates.io.

---

## Features

### Wallet and mining monitoring

- Add, remove, and list Kaspa wallets.
- Track wallet balances and UTXOs.
- Detect coinbase mining rewards.
- Wait for the configured reward-confirmation threshold.
- Analyze BlockDAG acceptance.
- Persist mined-block history.
- Apply wallet-scoped deduplication.
- Queue Telegram delivery in PostgreSQL with retry/backoff state.

### Operational safety

- Private-chat admin authorization.
- Actor-scoped command and callback rate limits.
- One-time CSPRNG confirmation nonces for sensitive admin actions.
- SHA-256-indexed confirmation state.
- Fail-closed behavior on sensitive database paths.
- Privacy-aware logging and input limits.
- Local health, readiness, and Prometheus-style metrics endpoints.
- Graceful shutdown handling.
- Panic/restart marker support.

### Alert controls

```text
/pause       = pause live monitoring
/mute_alerts = keep monitoring but suppress outgoing mining alerts
```

---

## Requirements

- Rust 1.98.1 for the pinned development/CI toolchain (MSRV: 1.97.1).
- PostgreSQL 18 recommended.
- A reachable Kaspa wRPC endpoint.
- A Telegram bot token.
- Docker/Compose only for container deployment.

Use the repository-pinned Rust toolchain rather than relying on a machine-global default.

---

## Environment setup

Copy `.env.example` to `.env` and replace every placeholder. Never commit `.env`.

Minimum production configuration:

```env
BOT_TOKEN=PUT_YOUR_TELEGRAM_BOT_TOKEN_HERE
ADMIN_USER_ID=PUT_YOUR_TELEGRAM_ADMIN_USER_ID_HERE
ADMIN_CHAT_ID=PUT_THE_SAME_PRIVATE_CHAT_ID_HERE

NODE_URL_01=wss://your-kaspa-node.example.com/json
DATABASE_URL=postgres://kaspa_pulse_app:PUT_APP_PASSWORD_HERE@127.0.0.1:5433/kaspa_dev?sslmode=disable

APP_ENV=production
RUST_LOG=info
SQLX_OFFLINE=true
ALLOW_RUNTIME_SCHEMA_ENSURE=false

ENABLE_TELEGRAM_DELIVERY_QUEUE=true
ENABLE_ALERT_DELIVERY=true
MIN_REWARD_CONFIRMATIONS=10
```

`ADMIN_ID` remains a backward-compatible fallback. New deployments should use explicit `ADMIN_USER_ID` and `ADMIN_CHAT_ID`; admin commands require the configured private admin chat.

See `.env.example` for health/readiness, timeouts, market history, webhook, retention, and monitoring settings.

---

## Database policy

Production runtime must use a least-privilege application role such as `kaspa_pulse_app`. Do not run the application as the PostgreSQL `postgres` superuser.

Schema changes belong in `migrations/`. Runtime schema creation is disabled by default:

```env
ALLOW_RUNTIME_SCHEMA_ENSURE=false
```

Use an administrative database role only for migrations or privileged maintenance.

---

## Running locally

```bash
cargo fmt --all -- --check
cargo check --locked --all-targets --all-features
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
cargo run --locked --release
```

---

## Container deployment

Build the production image:

```bash
docker build --pull -t kaspa-pulse:latest .
```

Start with Compose:

```bash
docker compose up -d --build
```

The production image:

- uses a Rust 1.98.1 / Debian 13 Trixie builder;
- uses a Debian 13 Trixie slim runtime;
- runs as non-root UID/GID `10001`;
- keeps the panic-recovery marker under `/var/lib/kaspa-pulse`;
- includes only the compiled binary and required runtime CA certificates.

Compose additionally enables an init process, drops Linux capabilities, sets `no-new-privileges`, limits JSON log rotation, and binds the published webhook port to host loopback by default.

---

## Health and metrics

When enabled, the service exposes local endpoints such as:

```text
/healthz
/readyz
/metrics
```

Example checks:

```bash
curl http://127.0.0.1:18080/healthz
curl http://127.0.0.1:18080/readyz
curl http://127.0.0.1:18080/metrics
```

Do not expose operational endpoints or the bot service directly to the public internet when a reverse proxy is expected.

---

## Quality and supply chain

Pull requests that change the Rust application run the core quality gate:

```bash
cargo fmt --all -- --check
cargo check --locked --all-targets --all-features
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
```

Production release, Docker build, and container smoke tests run inside the protected Rust CI gate for pull requests, `main`/`dev` pushes, and explicit manual runs. This keeps container/runtime compatibility verified before merge as well as after integration.

Dependency/security automation includes:

```bash
cargo audit
cargo deny check
cargo machete
cargo tree --locked -d
```

Additional controls:

- GitHub Actions are pinned to immutable commit SHAs.
- Normal CI checkout does not persist repository credentials.
- Git dependencies are allow-listed in `deny.toml`.
- `rusty-kaspa` is pinned to an explicit release tag rather than a floating branch.
- Dependabot checks Cargo, GitHub Actions, Docker, Rust toolchain, and Compose dependencies on staggered weekly schedules.
- The scheduled `rusty-kaspa` updater validates changes before publishing an update branch/PR.
- Accepted upstream/transitive RustSec exceptions are documented in `SECURITY_ADVISORIES.md`; they are not silently hidden.
- Releases include a CycloneDX SBOM, SHA-256 checksum, Sigstore bundles, and SLSA/in-toto provenance.
- [SUPPLY_CHAIN.md](SUPPLY_CHAIN.md) documents fail-closed verification of release provenance.
- [security-insights.yml](security-insights.yml) publishes the repository's OpenSSF Security Insights posture in a machine-readable format.

---

## Security reporting

Read [SECURITY.md](SECURITY.md) before reporting a vulnerability. Submit unpatched vulnerabilities privately through GitHub's repository Security Advisories / **Report a vulnerability** flow rather than a public issue.

See [SECURITY_ADVISORIES.md](SECURITY_ADVISORIES.md) for documented upstream/transitive exceptions and their review policy, and [SUPPLY_CHAIN.md](SUPPLY_CHAIN.md) for signed-release verification.

---

## Repository hygiene

The repository ignores local secrets, database dumps, backups, logs, panic marker files, generated exports, and Rust build output.

If a real secret was ever committed, removing the latest file is not enough: rotate the credential immediately and rewrite Git history when the exposure requires it.

---

## Support

```text
kaspa:qz0yqq8z3twwgg7lq2mjzg6w4edqys45w2wslz7tym2tc6s84580vvx9zr44g
```

---

## License

Kaspa Pulse is licensed under the [MIT License](LICENSE).
