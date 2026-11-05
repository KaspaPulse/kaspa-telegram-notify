<div align="center">

# ⚡ Kaspa Pulse

**Kaspa solo-mining rewards and wallet monitoring through Telegram — written in Rust.**

[![Rust 1.99.1](https://img.shields.io/badge/Rust-1.99.1-orange?logo=rust)](rust-toolchain.toml)
[![Rust CI](https://github.com/KaspaPulse/kaspa-telegram-notify/actions/workflows/ci.yml/badge.svg)](https://github.com/KaspaPulse/kaspa-telegram-notify/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![Release](https://img.shields.io/badge/Release-v2.0.0--rc.1-yellow.svg)](https://github.com/KaspaPulse/kaspa-telegram-notify/releases)

**English by default · Arabic by choice · Public Kaspa PNN · PostgreSQL-backed delivery**

</div>

---

> **Repository status — 5 November 2026:** This is the owner-approved, independently rooted source repository. The current Rust crate version is **1.2.10** (`Cargo.toml`). The GitHub **v2.0.0-rc.1** tag is a **source-only prerelease** marking the clean-history baseline; it is not a new production binary. The operating Telegram service is managed separately and is not deployed from GitHub Actions.
>
> **Release qualification is in progress:** The original clean-root CI run failed strict Clippy and reported security advisories. A separately tested release candidate now passes strict Clippy and its refreshed RustSec and cargo-deny gates; **GitHub CI and complete distributable-image qualification remain open**. See [Release status and known issues](#release-status-and-known-issues).

## What it does

Kaspa Pulse watches registered Kaspa wallets and sends Telegram alerts about detected mining rewards. It uses the **public Kaspa Peer-to-Peer Network (PNN)** for chain data, checks reward-related data, and stores wallet state, notification events and delivery deduplication records in PostgreSQL.

| Area | Capabilities |
| --- | --- |
| **Wallets** | Add, select, remove and inspect Kaspa wallets; view balances and mined blocks. |
| **Mining rewards** | Coinbase/reward event detection, confirmation handling, block information and deduplicated notifications. |
| **Telegram** | Compact inline menus, network/market views, wallet actions and a private owner-admin panel. |
| **Languages** | **English is the default** for new users, regardless of Telegram app language. Users can explicitly select Arabic under **More → Language**; preference is stored per user. |
| **Alerts** | A shared wallet can notify multiple recipients in their selected languages. Alerts show **Time / الوقت** with the timestamp and UTC precision preserved. |
| **Donations** | **❤️ Donate** is directly on the Home menu, not hidden under More. |
| **Reliability** | PostgreSQL-backed transactional alert outbox, event deduplication, delivery retries, health and readiness endpoints. |

The native Telegram slash-command picker presents only **`/start`** and **`/help`**. Other legacy commands may still be accepted for compatibility, but are not advertised. Owner-only actions remain authorization-checked and are available from the protected **Admin** button.

### Telegram home menu

```text
👛 Wallets       💹 Market
🌐 Network       ⚙️ More
❤️ Donate
🛡️ Admin        (authorized owner only)
```

The bot edits an existing menu where appropriate and avoids showing replies quoted against an old button message.

## How notifications flow

```text
Telegram wallet registration
         │
         ▼
PostgreSQL wallet state
         │
         ▼
Public Kaspa PNN → UTXO / reward processing → confirmation checks
         │
         ▼
PostgreSQL event and wallet-level deduplication
         │
         ▼
Per-recipient EN / AR message formatting
         │
         ▼
Transactional Telegram delivery queue → Telegram
```

**No local Kaspa node URL is required.** This source explicitly rejects the old `NODE_URL_01`, `UTXO_STANDBY_NODE_URL`, and `UTXO_STANDBY_NODE_URL_FILE` overrides. Do not copy those settings from older deployments.

## Technology baseline

| Component | Pinned/current baseline |
| --- | --- |
| Application | `kaspa-pulse` crate **1.2.10**; Rust Edition 2024 |
| Toolchain | **Rust stable 1.99.1** (`rust-toolchain.toml`); declared crate MSRV **1.97.1** |
| Kaspa Rust crates | **2.0.1** at immutable `rusty-kaspa` commit `cfafeb4c093fa37a303f1b9f19c58f986b870ce3` |
| Telegram | Teloxide **0.17** with a locally vendored `teloxide-core` patch |
| Database | PostgreSQL; SQLx **0.9** (Postgres feature set) |
| HTTP | Axum **0.8**, Reqwest **0.13**, Rustls |

The repository sets `publish = false` to prevent accidental publication to crates.io.

## Local development

Prerequisites: Rust 1.99.1, a dedicated PostgreSQL database and application role, and a Telegram bot token. Use the least-privilege database role described in [DATABASE_SECURITY.md](DATABASE_SECURITY.md).

```bash
git clone https://github.com/KaspaPulse/kaspa-telegram-notify.git
cd kaspa-telegram-notify
rustup show
cargo fmt --all -- --check
cargo check --locked --all-targets
cargo test --locked --lib
```

Prepare your **private environment file** outside version control, for example:

```dotenv
BOT_TOKEN=<your-telegram-bot-token>
ADMIN_USER_ID=<your-numeric-telegram-user-id>
ADMIN_CHAT_ID=<same-private-chat-id-as-admin-user-id>
DATABASE_URL=postgres://<db-user>:<password>@127.0.0.1:5432/<database>
APP_ENV=development
USE_WEBHOOK=false
ALLOW_RUNTIME_SCHEMA_ENSURE=false
```

Replace **every** angle-bracketed placeholder. `ADMIN_CHAT_ID` must match `ADMIN_USER_ID` for the owner's private chat. Do not put tokens, passwords, actual chat IDs, logs or live database files in Git.

Apply the reviewed SQL migrations from [migrations/](migrations/) using an administrative migration role **before** running the service. Schema creation by the runtime is disabled by default.

For a local, isolated instance using polling rather than a webhook:

```bash
cargo run --locked --release --bin kaspa-block-alert-bot
```

A production webhook additionally needs `USE_WEBHOOK=true`, `WEBHOOK_DOMAIN` and `WEBHOOK_SECRET_TOKEN`, plus an appropriately secured ingress. See [SECURITY.md](SECURITY.md). Bind metrics, health and readiness endpoints to a trusted local interface rather than the public internet.

### Quality checks

```bash
cargo fmt --all -- --check
cargo check --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --lib
cargo audit
cargo deny check
```

The repository's [Rust CI](.github/workflows/ci.yml) runs on pushes and pull requests with **read-only permissions**. It does **not** build or deploy production releases.

**Quality evidence for the updated candidate (5 November 2026):** formatting and strict all-target Clippy passed locally; 154 Rust library tests passed with one ignored, transactional outbox 21/21, wallet/queue tests 14/14, runtime DB role 1/1, UI closure 10/10, and runtime wiring 15/15. Refreshed RustSec reported **zero vulnerability advisories**; cargo-deny passed after the **eight explicitly documented unmaintained transitive exceptions** in [SECURITY_ADVISORIES.md](SECURITY_ADVISORIES.md). **Wait for a green GitHub CI run on the new commit before calling these remotely verified.**

### Container note

The [Dockerfile](Dockerfile) uses Rust **1.99.1**, the correct `kaspa-block-alert-bot` binary name, immutable image digests, a non-root runtime user and a **required source commit SHA**. Use `docker build --build-arg SOURCE_REVISION="$(git rev-parse HEAD)" .` when qualifying an image. Dockerfile policy checks and native ARM64 fail-closed startup were verified, but **the full multi-stage Docker image and its production startup remain unqualified**. See [SUPPLY_CHAIN.md](SUPPLY_CHAIN.md).

## Release status and known issues

**v2.0.0-rc.1 is a source-only preview, not a production release.**

- **Passed on a newer isolated candidate:** Rust formatting, strict Clippy, ARM64 all-targets compilation, 154 Rust library tests, 21 transactional-outbox tests, 14 queue/wallet tests, 10 UI closure tests, 15 wiring tests, and a full isolated PostgreSQL 18 migration sequence. The **13 original RustSec vulnerabilities and the additional Tokio unsoundness advisory were fixed with patched dependencies**, not ignored.
- **Dependency policy:** Refreshed RustSec shows **zero vulnerabilities and eight unmaintained warnings**. `cargo deny` passes with **eight exact-ID exceptions solely for documented upstream maintenance warnings**. No yanked dependencies remain and no expired OSV exclusions are retained.
- **Not yet verified:** green GitHub CI on the new commit, full container build and non-root runtime smoke, reproducible release binaries, SBOM and signed provenance, and every optional database/Telegram integration test. Keep stable release on HOLD.
- **Production:** the live Telegram bot is managed and rolled back separately; publishing a GitHub tag does not change its binary, data or subscriber notifications.

See [SECURITY_ADVISORIES.md](SECURITY_ADVISORIES.md), [SUPPLY_CHAIN.md](SUPPLY_CHAIN.md), and [GitHub Releases](https://github.com/KaspaPulse/kaspa-telegram-notify/releases) for the current evidence and release notes.

## Repository history and maintenance

On **5 November 2026**, the repository began a **new, independent root commit** from the owner-approved Rust source. Older GitHub branches, tags and releases were retired. Public forks, cached objects or historical pull-request references may still retain content from the previous history; a history reset does **not** guarantee global erasure. See [REPOSITORY_RESET_NOTICE.md](REPOSITORY_RESET_NOTICE.md).

**Dependabot version updates and automated security-fix pull requests are disabled.** Auto-merge is disabled; no scheduled update or production release workflows are included in this source tree. Security alerts should still be reviewed and dependencies updated **manually**, with tests and owner approval.

## Privacy and responsible disclosure

Do not post private chat messages, wallet identifiers tied to users, real bot tokens, database exports, or host access details in public issues or commits. Submit security vulnerabilities privately through [SECURITY.md](SECURITY.md) and the repository's GitHub private vulnerability reporting flow.

A clean repository history does not replace credential rotation or remediation if a secret was previously exposed.

## Donate

If Kaspa Pulse is useful, voluntary Kaspa donations help support maintenance:

```text
kaspa:qz0yqq8z3twwgg7lq2mjzg6w4edqys45w2wslz7tym2tc6s84580vvx9zr44g
```

The same address is available under **❤️ Donate** in the Telegram Home menu.

## License and contributions

Kaspa Pulse is licensed under [MIT](LICENSE). Vendored components retain their applicable notices and licenses. See [CONTRIBUTING.md](CONTRIBUTING.md) and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) before contributing.
