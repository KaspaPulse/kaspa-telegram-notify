# Kaspa Pulse — verified live production OP092

**Accepted:** 9 November 2026, native Linux ARM64 on `dns`.
Guarded production cutover and independent postflight both **PASS**.

| Identifier | Verified value |
| --- | --- |
| Application version | `1.2.10` |
| Live release | `op092-595a1dc25372cb6a` |
| Full Git source commit | `0c4650a5ff89f59c5ba9402085ffe6ecd4e1c5a8` |
| Git commit UTC | `2026-11-09T07:51:55Z` |
| Build UTC | `2026-11-09T08:38:04Z` |
| Production activation UTC | `2026-11-09T08:46:22Z` |
| Source snapshot SHA256 | `595a1dc25372cb6a7bc372881e120172505bf6bb329e3b169fd8ab855b34903d` |
| Actual running binary SHA256 | `d7285d9eca53f2a15fba855ca10263c4a4f6e8ca9d038e1615a9097da22d352b` |
| Preserved rollback | `op090-d2e3817d422f7500` |

The version card is **More → 📦 Version**, or type `/version`.
Authorized owners additionally see the full commit and source/binary hashes
via **Admin → 📦 Live Release**. Dates display in **Riyadh UTC+03:00**.
The displayed identity is embedded in the actual running executable,
not taken from the repository default-branch HEAD.

Local-only trustworthy recovery artifacts:

- Production release directory:
  `/opt/kaspa-pulse/releases/op092-rust-op092-595a1dc25372cb6a`.
- Root-only release record beside executable: `release_provenance.json`.
- Root-only continuity, verified database backup and independent postflight:
  `KASPA_PULSE_OP092_LIVE_RELEASE_GIT_COMMIT_PROVENANCE_20261109`.
- The Git commit above is the **specific built source**. Subsequent
  README-only Git commits do not change the binary.

The production release confirmed HTTP 200 health/readiness, public PNN
sources 5/5, no queued Telegram updates and 68 excluded historical
proof rows/81,221 attempts unchanged. No historical reward replay.
No automatic GitHub builds, workflow files, paid runner or scheduled task
is part of the production path.
