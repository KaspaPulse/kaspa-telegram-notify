# Operational Qualification Harness

Task family: Kaspa Telegram operational qualification (`EXEC-01`).

The harness provides a reproducible, resumable, fail-closed way to qualify the exact Kaspa Pulse candidate in an isolated, trusted, non-production development environment without using production systems, real users, real Telegram traffic, real wallets, public listeners, or production credentials.

## Scope and safety boundary

- Run qualification only in an isolated, trusted, non-production development environment.
- Default task identity: `kaspa-telegram-opqual-v1`.
- Docker network: `kp-opqual-v1`, internal-only, no host port publishing.
- PostgreSQL 18 database: `kaspa_opqual_v1`.
- Runtime role: `kaspa_pulse_app`.
- PostgreSQL application name: `kaspa-opqual-v1`.
- All containers, network, and volume carry the task label and are ownership-checked before mutation or cleanup.
- Synthetic Telegram identities and generated/synthetic Kaspa data only.
- Provider fixtures run locally on the internal Docker network and do not require Internet access.
- Merge, release, deployment, and production operations are outside this harness.

The harness refuses inherited runtime credential environment variables and validates the expected source commit and qualified binary SHA-256 before material work.

## One-command usage

Local qualification requires an explicit non-production boundary opt-in:

```bash
export KASPA_PULSE_OPQUAL_ENV=ISOLATED_NON_PRODUCTION
```

This opt-in does not weaken the harness: inherited runtime credentials are still refused, provider fixtures remain internal, and task-owned resources remain isolated and ownership-checked.

Dry-run first:

```bash
cargo run --locked -p xtask -- opqual dry-run --binary target/release/kaspa-pulse
```

A dry-run performs preflight and prints the plan. It must not create Docker resources, initialize a database, start the candidate, or send signals.

Material run:

```bash
cargo run --locked -p xtask -- opqual run --binary target/release/kaspa-pulse
```

Resume an interrupted material run only after inspecting its state and resources:

```bash
cargo run --locked -p xtask -- opqual resume <run-id> --binary target/release/kaspa-pulse
```

A phase marked `VERIFIED` is skipped only when its current side effects still match. If the receipt and reality disagree, the harness records an `UNKNOWN` resume condition and refuses blind replay.

## Execution model

The one-command lifecycle is:

`preflight → provision → migrate → fixtures → start-app → create-lock → EXEC-01 shutdown proof → restart → scenarios → cleanup → final evidence`.

`EXEC-01` uses the real Telegram delivery worker against synthetic task-owned state. The external observer owns the conflicting advisory lock; the application claims a synthetic delivery row and must block inside its real `pg_advisory_xact_lock(chat_id)` path. Only then does the harness send SIGTERM to the exact task-owned application container/PID.

The shutdown proof requires the application backend to have been waiting on the advisory lock, the external lock to remain held while shutdown is observed, application sessions and application locks to reach zero, clean process exit, no SIGKILL success path, and no admission of new post-SIGTERM queue work.

The restart phase releases the external test lock only after zero-session/zero-lock proof, starts the exact same candidate, requires health/readiness and PostgreSQL connectivity, and executes a synthetic Telegram `/help` smoke.

The scenario runner consumes `opqual/scenario-map.csv` as mapping input only. It never treats the CSV as PASS evidence. Every executed row ends as `VERIFIED_PASS`, `VERIFIED_FAIL`, `BLOCKED`, or `NOT_APPLICABLE`; unsupported/unproven contracts remain `BLOCKED`.

The Rust scenario runner also executes the supplemental EDGE-001 full-dispatcher cycle. It proves both missing and negative HTTP-200 fee payloads through the exact main/dispatcher path without rendering invalid values as live estimates, and includes that journey in the final scenario summary.

## Evidence and interruption safety

Each material run writes under `evidence/opqual/<run-id>/` and keeps `state.json` plus append-only `operations.jsonl`. Material operations record `PLANNED` before effects and a verified terminal state afterward. Signal state is persisted so a resume never sends SIGTERM twice just because a previous client/session disappeared.

Successful runs produce source/binary identities, environment and Docker inventories, network map, migration receipt, synthetic identity manifest, fixture hashes, application stdout/stderr, PostgreSQL activity/lock timelines, signal timeline, shutdown and restart verification, scenario results, cleanup receipt, `FINAL_RESULT.json`, and `SHA256SUMS.txt`.

Test CA private keys are ephemeral runtime inputs and are deleted during cleanup; final evidence must contain no production credentials or real-user data.

## Result meanings

`HARNESS_PASS` means the harness implementation/guards/tests are qualified. It does **not** prove runtime shutdown behavior.

`EXEC_01_PASS` requires actual application-owned lock blocking, SIGTERM, zero application sessions/locks, clean exit without SIGKILL, and clean restart.

`AUDIT_COMPLETE` additionally requires the mapped operational scenarios to have no `VERIFIED_FAIL`, no `BLOCKED`, no `NOT_TESTED` rows, and at least **150 completed dependent full journeys**. Component-only evidence does not satisfy that full-journey gate. Harness implementation alone never upgrades the audit to complete.

## Troubleshooting

If a material phase fails, preserve its evidence and do not change the expected candidate to make the harness pass. If tool safety blocks an effect, classify the runtime execution as blocked and continue non-blocked harness validation only. Never route around tool safety, reuse a production database, publish ports, or clean unrelated resources.
