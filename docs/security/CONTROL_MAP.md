# Security Control Map

This document maps security expectations to repository evidence. It is not a production-attestation checklist and does not claim controls that are not evidenced by source, CI, or documented operational policy.

| Control | Status | Primary evidence |
| --- | --- | --- |
| Private-chat admin identity is explicit and actor-scoped | Enforced in source/tests | `src/presentation/telegram/request_identity.rs`, `tests/telegram_identity_security_tests.rs` |
| Sensitive admin actions require bounded confirmation state | Enforced in source/tests | `src/presentation/telegram/handlers/admin_confirm.rs`, `tests/admin_confirmation_tests.rs` |
| Wallet/raw-message input is validated and bounded | Enforced in source/tests/fuzzing | `src/presentation/telegram/handlers/raw_message.rs`, `fuzz/fuzz_targets/text_safety.rs` |
| User-controlled Telegram HTML/log output is escaped or redacted | Enforced in source/tests/fuzzing | `src/presentation/telegram/formatting/`, `tests/logging_privacy_tests.rs`, `docs/FUZZING.md` |
| Webhook and operational listeners default to loopback/fail closed | Enforced in source/config/tests | `src/infrastructure/webhook_security.rs`, `.env.example`, `tests/webhook_security_tests.rs` |
| Public reverse-proxy reference exposes only the webhook path | Canonical reference enforced in repository | `ops/nginx/kaspa-pulse-webhook.conf` |
| Runtime database uses least privilege and migration files | Policy + tests | `docs/security/DATABASE_SECURITY.md`, `migrations/`, `tests/runtime_event_write_privilege_tests.rs` |
| Runtime schema mutation is disabled by default | Policy + config/tests | `.env.example`, `tests/migrations_only_runtime_contract_tests.rs` |
| Owned executable implementation is Rust-only, fail closed | Required CI proof | `cargo xtask proof verify`, `proof/policy.toml`, `.github/workflows/security.yml` |
| Dependency advisories/licenses are independently gated | Required CI | `cargo audit`, `cargo deny`, OSV, Dependency Review |
| Static analysis and secret scanning are required | Required CI | CodeQL and Secret Scan workflows |
| Release artifacts include checksum, SBOM, provenance and attestations | Enforced release workflow | `.github/workflows/release.yml`, `SUPPLY_CHAIN.md` |
| Production development boundary is explicit | Repository policy | `AGENTS.md`, `CONTRIBUTING.md` |

Operational values such as production credentials, deployment state, runtime health, and current task continuity are intentionally not stored in this public source tree. They must be verified from the appropriate live system when required.
