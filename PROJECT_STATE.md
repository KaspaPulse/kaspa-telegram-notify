# Project State — runtime migration contract correction v1.2.7

Repository: KaspaPulse/kaspa-telegram-notify only. AGENTS.md remains authoritative.
Development: kas only. Production: dns; no production access/change during this continuation.
Worktree: `/home/kas/kaspa-telegram-runtime-contract-v127-20260912`
Branch: `fix/runtime-schema-contract-v1.2.7-20260912`
Base: `71fb0aaf3acd2e519acb90f11e990efae5ab373c` (protected main, PR #58).
Always verify actual Git HEAD, branch and working tree before resuming.
AGENTS.override.md was absent. No PLANS.md was needed/read.

## Reconciled publication history

The user approved publication/deployment after local qualification. The single
real branch push was completed; PR #58 passed all protected CI and was squash
merged to the base SHA above. GitHub published signed release v1.2.6 at that SHA.
The original eleven local checkpoints and their worktree are preserved:
`/home/kas/kaspa-telegram-functional-closure-v126-20260912` at `a5bbc129...`.

An ARM64 binary was then built on kas from the exact merged SHA. Its Ubuntu 24.04
runtime qualification exposed F-12 before production was accessed: a database
prepared exclusively by versioned migrations lacked system_settings access and
wallet/reward write privileges. The CI preparation script had supplied some of
those grants separately and hid the missing production migration contract.
Therefore v1.2.6 is NOT qualified for production deployment. Do not deploy it,
rewrite its published commit/tag/assets, or apply undocumented production grants.

## Current correction

The v1.2.7 candidate adds one migration defining the required runtime DML for
system_settings, user_wallets, wallet_seen_utxos, pending_rewards, mined_blocks
and wallet_alert_dedup, plus the mined-block ID sequence USAGE. It adds no schema
CREATE, TRUNCATE, unused legacy-table access, or administrative privilege.
CI preparation now adds only its existing test reset TRUNCATE privileges; all
runtime DML must come from the same versioned migrations used for deployment.

The new migrations-only regression creates a unique database, applies every SQL
migration in lexical order as the admin role, and exercises the real repository
as kaspa_pulse_app. It checks settings, wallet insert/update, UTXO state,
pending-reward insert/update, mined-block insertion, deduplication and forget-all.
It closes/drops its isolated DB before checking the result. It neither grants
runtime privileges itself nor relies on CI grants in the ordinary test DB.
RED: permission denied system_settings with 20 migrations. GREEN: the actual
workflows pass with 21 migrations. The unchanged merged ARM64 binary also passes
the runtime schema proof after adding only the proposed migration: settings,
commands, event persistence, queue delivery, invalid-value fail-closed and clean
idle shutdown. This is not a v1.2.7 production artifact qualification.
The isolated mock can return HTTP 200 with readiness body degraded (inactive
subscription / stale scan); the harness records it explicitly per the existing
readiness contract. It does not prove live integrations. Final local qualification
is commit-bound.

## Qualification and NEXT ACTION

```bash
git status --short --branch
git rev-parse HEAD
git notes --ref=refs/notes/runtime-contract-v127 show HEAD
```

If the note is missing or incomplete, inspect existing logs/processes and finish
only the missing gate. Do not repeat passed tests or an active artifact build.
If it reports LOCAL_COMPLETE, the correction is ready for a publication decision.
The task's explicit EXACTLY ONE real push allowance was already consumed by
v1.2.6. A second real branch push requires an explicit user exception/new release
cycle authorization. Prepare the correction fully before asking. Until then,
use only the existing local mirror and do not deploy.

After that authorization: one validated corrective branch push, protected PR/CI,
squash merge (required by main protection), then build/package the exact merged
SHA on kas, verify the ARM64 native artifact on Ubuntu 24.04, preserve production
rollback, deploy, and verify production health/integrations. Do not build on dns.

## Evidence and continuity

v1.2.6 publication and failed ARM qualification:
`/home/kas/kaspa-telegram-dev/evidence/KASPA_TELEGRAM_V126_PUBLICATION_20260912/`
`/home/kas/kaspa-telegram-artifacts/71fb0aaf3acd2e519acb90f11e990efae5ab373c/`
Current F-12 evidence:
`/home/kas/kaspa-telegram-dev/evidence/KASPA_TELEGRAM_RUNTIME_CONTRACT_V127_20260912/`
Final per-commit qualification belongs below that directory and in the Git note.
Report: `FUNCTIONAL_CLOSURE_REPORT.md` (includes the historical verification limits).

Original v1.2.6 qualification: 277 executions / 26 binaries / 218 unique names;
F-01 through F-10 10/10 plus F-11 passed. Its 64-file manifest is unchanged. These
are historical results, not proof that migrations-only bootstrap succeeded.

## Reuse constraints

- Preserve original worktrees, eleven checkpoints, history, remotes and notes.
- local: `/home/kas/kaspa-telegram-dev/local-git/kaspa-telegram-notify.git`
- origin fetch: `https://github.com/KaspaPulse/kaspa-telegram-notify.git`
- origin push remains `local-first-push-disabled://KaspaPulse/kaspa-telegram-notify.git`.
- Keep FIX → TEST → VERIFY → REVIEW → LOCAL COMMIT → CONTINUE.
- PostgreSQL clients run inside task containers; host psql is absent.
- Development test container: kp-f12-dev-pg, loopback 55436, kaspa_dev.
- Isolated native fixture: kp-fc-runtime-net (internal), kp-fc-resume-pg,
  kp-fc-resume-tg, kp-fc-resume-node, kp-fc-resume-app. Inspect before reusing.
- Fixture HTTP must run inside the internal network, not host-published ports.
- Reuse corrected Telegram TLS and Kaspa subscribe/unsubscribe mocks; synthetic
  environment files and private certificate keys must never enter Git.
- Never run ci-prepare-postgres.sh on production: it resets a synthetic test role
  password and grants CI-only TRUNCATE. Production applies versioned SQL as admin.
- Busy price refresh may exceed the documented three-second shutdown drain;
  retain bounded-shutdown evidence and verify clean shutdown separately at idle.
