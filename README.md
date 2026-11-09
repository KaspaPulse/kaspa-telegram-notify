<div align="center">

# ⚡ Kaspa Pulse

**Kaspa solo-mining rewards and wallet monitoring through Telegram — written in Rust.**

[![Rust 1.99.1](https://img.shields.io/badge/Rust-1.99.1-orange?logo=rust)](rust-toolchain.toml)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![Production](https://img.shields.io/badge/Production-OP092-success-brightgreen.svg)](PRODUCTION_RELEASE.md)

**English by default · Arabic by choice · Public Kaspa PNN · PostgreSQL-backed delivery**

</div>

---

**Production source of truth — 9 November 2026:** The Telegram bot is running
**Kaspa Pulse v1.2.10**, production release **`op092-595a1dc25372cb6a`**.
Its independently verified full Git commit is
**`0c4650a5ff89f59c5ba9402085ffe6ecd4e1c5a8`**. This commit is the
deployed Rust source; a later documentation-only branch commit is not a new
production binary. See [PRODUCTION_RELEASE.md](PRODUCTION_RELEASE.md) for
SHA-256, Git commit date, build time, production activation and rollback.
In Telegram use **More → 📦 Version**, **`/version`**, or
**Admin → 📦 Live Release** (authorized owner only).

**No automated cloud builds:** GitHub Actions is disabled in repository
settings and this source tree contains **no `.github/workflows` files**.
Dependabot automatic updates and automated security fixes are disabled.
Builds and tests run manually and locally with a locked Rust toolchain.
A Git push never deploys or restarts the bot.

### Production-first source reconciliation (OP095)

**Authoritative state is the installed, verified OP092 executable**, not a
newer repository timestamp. The recorded OP092 build was compiled from Git
commit **`0c4650a5ff89f59c5ba9402085ffe6ecd4e1c5a8`**. The current
`main` branch retains that exact application source, Cargo dependency lock,
vendored Rust code, migrations, and deployment scripts. Subsequent commits
may refresh documentation and remove old CI workflows **without changing
the deployed application**. The current Git commit and the source commit
embedded into the binary therefore serve different, explicitly documented
purposes.

The canonical *development* checkout is maintained on the `dns` host; the
public GitHub `main` branch is its reviewed remote mirror. Obsolete local
checkouts and sensitive environment backups must be retained only in
restricted local recovery storage, never committed or uploaded. Neither the
working checkout nor GitHub is a production database or credentials backup.

Before a manual release or push, verify the checked-out source against the
published production-source commit; for example, this **read-only** check
must report no application-source changes for the current OP092 deployment:

```bash
git diff --exit-code \
  0c4650a5ff89f59c5ba9402085ffe6ecd4e1c5a8 HEAD -- \
  src/ vendor/ migrations/ tests/ scripts/ Cargo.toml Cargo.lock \
  build.rs rust-toolchain.toml Dockerfile
```

A local, fail-closed `pre-push` hook on the maintained checkout rejects
known secret patterns, sensitive file paths, unreviewed history rewrites,
and stale pre-OP093 source. **Git hooks are not transported by a clone**:
a new workstation must install and verify an equivalent local guard, and
manual review of all added content remains mandatory. This guard does not
claim comprehensive detection of every possible secret.

The OP094 secret audit examined the published source and reachable
repository history; it did not prove that any previously exposed, unreachable
or externally cached data had been erased. Release provenance and rollback
details are recorded in [PRODUCTION_RELEASE.md](PRODUCTION_RELEASE.md).
**No application deployment or automatic GitHub workflow was triggered by
the OP095 repository reconciliation.**

## What it does

Kaspa Pulse watches registered Kaspa wallets and sends Telegram alerts about detected mining rewards. It uses the **public Kaspa Peer-to-Peer Network (PNN)** for chain data, checks reward-related data, and stores wallet state, notification events and delivery deduplication records in PostgreSQL.

| Area | Capabilities |
| --- | --- |
| **Wallets** | Add, select, remove and inspect Kaspa wallets; view balances and mined blocks. |
| **Mining rewards** | Coinbase/reward event detection, confirmation handling, block information and deduplicated notifications. |
| **Telegram** | Compact menus, network/market views, wallets, private owner administration and verified production release information. |
| **Languages** | **English is the default** for new users, regardless of Telegram app language. Users can explicitly select Arabic under **More → Language**; preference is stored per user. |
| **Alerts** | A shared wallet can notify multiple recipients in their selected languages. Alerts show **Time / الوقت** with the timestamp and UTC precision preserved. |
| **Donations** | **❤️ Donate** is directly on the Home menu, not hidden under More. |
| **Reliability** | PostgreSQL-backed transactional alert outbox, event deduplication, delivery retries, health and readiness endpoints. |

The native Telegram slash-command picker presents only **`/start`** and **`/help`**. The manually typed **`/version`** command also works, and the same card is available from **More → Version**. Other legacy commands may still be accepted for compatibility, but are not advertised. Owner-only actions remain authorization-checked and are available from the protected **Admin** button.

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

### Manual offline quality checks

GitHub Actions is **disabled**, and the repository has **no workflow files**.
Run quality checks manually on trusted hardware with the pinned toolchain:

```bash
cargo fmt --all -- --check
cargo clippy --release --locked --offline --all-targets -- -D warnings
cargo test --release --locked --offline --lib
cargo test --release --locked --offline --test production_regression_tests
# Run PNN, reward-proof, Telegram and sensitive integration tests manually.
```

**Native ARM64 production OP092 (9 November 2026):** 221 library tests
(one inherited ignored), four release-information tests, 32 production
regressions, 37 sensitive PNN/proof/dedup tests and 15 runtime integration
tests passed. Strict Clippy and rustfmt passed. The existing informational
proc-macro-error2 future-compatibility warning is **not** a production
failure. A native release is not automatic Docker-image certification.
Use [SECURITY_ADVISORIES.md](SECURITY_ADVISORIES.md) and
[SUPPLY_CHAIN.md](SUPPLY_CHAIN.md) for additional manual reviews.

### Container note

The [Dockerfile](Dockerfile) uses Rust **1.99.1**, the correct `kaspa-block-alert-bot` binary name, immutable image digests, a non-root runtime user and a **required source commit SHA**. Use `docker build --build-arg SOURCE_REVISION="$(git rev-parse HEAD)" .` when qualifying an image. Dockerfile policy checks and native ARM64 fail-closed startup were verified, but **the full multi-stage Docker image and its production startup remain unqualified**. See [SUPPLY_CHAIN.md](SUPPLY_CHAIN.md).

## Verified live production and version provenance

| Property | Verified running production |
| --- | --- |
| Crate/application | `kaspa-pulse 1.2.10` |
| Live release | `op092-595a1dc25372cb6a` |
| **Full source Git commit** | `0c4650a5ff89f59c5ba9402085ffe6ecd4e1c5a8` |
| Source commit UTC | `2026-11-09 07:51:55` |
| Build UTC | `2026-11-09 08:38:04` |
| Production activation UTC | `2026-11-09 08:46:22` |
| Live binary SHA-256 | `d7285d9eca53f2a15fba855ca10263c4a4f6e8ca9d038e1615a9097da22d352b` |
| Previous release for rollback | `op090-d2e3817d422f7500` |
| GitHub Actions | **Disabled; no workflow files** |

The Telegram **`/version`** command reads the identity embedded in the
actual running binary. Users see a compact public card; authenticated
private-chat owners see the full commit, source digest and live binary digest.
A later README-only Git commit does **not** create a new production binary.
See [PRODUCTION_RELEASE.md](PRODUCTION_RELEASE.md) for attestation and recovery.

At independent production acceptance the bot was healthy/ready (HTTP 200),
the public PNN source pool was qualified 5/5, Telegram delivery queue was
empty and the 68 historic excluded proof rows/81,221 attempts were unchanged.
No historical replay or test notification was sent.

The native Linux ARM64 executable is verified. Optional Docker image
packaging, an independent reproducible-build check and every optional
integration environment have not all been qualified. Do not imply they have.

## Repository policy and history

**No paid automation from this repository:** GitHub Actions is disabled;
no workflow files, Git LFS, paid runners, Codespaces, hosted CI or scheduled
builds are configured. Dependabot automated version/security updates
and automatic fixes are disabled; security alerts are reviewed manually.
This does not control subscriptions or billing from **other** repositories.

On 9 November 2026 the owner-approved local repository replaced the
older GitHub `main` branch tip. The specific source commit used to build
the live OP092 binary remains reachable from the refreshed branch for
accurate production traceability. An overwritten branch or deleted tag
does not guarantee globally erasing older commits, forks or cached objects.
See [REPOSITORY_RESET_NOTICE.md](REPOSITORY_RESET_NOTICE.md).

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
