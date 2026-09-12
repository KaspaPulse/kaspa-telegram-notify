# Project State — legacy upgrade correction v1.2.8

Repository: KaspaPulse/kaspa-telegram-notify only. AGENTS.md remains authoritative.
All development, Git, builds and tests run on kas; dns is production only.
Worktree: /home/kas/kaspa-telegram-legacy-upgrade-v128-20260912
Branch: fix/legacy-mined-blocks-upgrade-v1.2.8-20260912
Base: c2f271bbb229facd84b64e2a14dd72f0c4338272 (protected main, PR #59).
AGENTS.override.md was absent. No PLANS.md was needed/read.
Always verify actual HEAD, working tree and local mirror before resuming.

## Reconciled state

PR #58 published v1.2.6; PR #59 published v1.2.7 after the user authorized
the corrective push and deployment. The v1.2.7 merged source and ARM64 artifact
passed their recorded gates on a fresh database. They did not prove the legacy
production upgrade.

The user ran the prepared administrative deployment at 2026-09-12T17:49:06Z.
PostgreSQL exited 3 in the atomic migration transaction, before service stop or
executable replacement. The original script did not preserve psql output.
Read-only verification found the same PID 2790969, zero restarts, health ok,
readiness ready, connected node/subscription, and zero application DB errors.
Production remained v1.2.3 at a23d337cbd3dd84944228e4c23ac30cfc9b38237.
The absent legacy tables were still absent after the failed transaction.

Both production backups are preserved. The administrative backup has 36249837
bytes and SHA-256 b03ed56354bbc1295197afbe06f09e757b252089de91db2cdad85c6e77c79ae8.
Do not rerun the original deploy-v127.py: its migration is incompatible and its
fixed backup filename already exists.

## F-13 correction and proof

Production mined_blocks uses (wallet, outpoint) and has no ID column or sequence.
The v1.2.7 migration granted USAGE on mined_blocks_id_seq unconditionally.
A PostgreSQL 18 regression reproduced SQLSTATE 42P01 at that exact GRANT.
The correction grants USAGE only when the sequence exists. Fresh schemas still
receive the grant; legacy schemas need no sequence or primary-key change.
The migration is corrected forward in a new commit; published commits, tags and
packages remain unchanged. Application Rust logic and dependency versions do not
change.

Fresh and legacy migrations-only regressions both pass. The legacy case builds
the published old schema and its old migrations before seeding a preservation
canary, removes the two absent legacy tables, then atomically applies only the
published init and three September contracts. It exercises the actual application
role and checks canary retention, restored chat_history and no unnecessary ID
sequence. Never replay historical destructive migrations on production.

The revised operational template preserves complete command output in private
attempt-specific records and creates a fresh backup filename for every attempt.
Five checks passed against isolated PostgreSQL 18: backup retention, valid
archives, verbose SQLSTATE/error capture, rollback/data retention and refusal of
an unqualified template. The template requires the qualified merged SHA and
artifact hashes before deployment.

## NEXT ACTION

Read the actual HEAD note: git notes --ref=refs/notes/legacy-upgrade-v128 show HEAD.
Finish only incomplete local gates; never repeat passed work or active builds.
A LOCAL_COMPLETE note records the exact candidate, results and artifact.

No third real push has occurred. Preserve the two earlier published releases and
their history. Any publication must follow the user's authorization and the
established one-final-push workflow: fully qualify locally, push from kas, protected
PR/CI, squash merge, then build and qualify the exact merged SHA on kas.
Deploy only that approved artifact with a fresh backup and the four selected SQL
files; retain or restore the healthy v1.2.3 baseline on failure.

RDC on dns blocks sudo and unprivileged systemd management is not authorized.
Do not bypass that control. Administrative deployment requires an authorized
operator session. Deployment approval already exists; connector access is a
separate limitation, not a reason to ask again for the same approval.

## Evidence and preservation

Current evidence:
/home/kas/kaspa-telegram-dev/evidence/KASPA_TELEGRAM_LEGACY_UPGRADE_V128_20260912/
Contains production-failure.json, red/green logs, operator-tests.json and
deploy-v128.template.py. Preserve operational script text/results in the local
Git note. No secrets or database dumps enter Git.

Prior publication-state.json is reconciled under:
/home/kas/kaspa-telegram-dev/evidence/KASPA_TELEGRAM_V127_PUBLICATION_20260912/
The original v1.2.7 worktree stays at 972859fa8c8ccedefc4c4d6eb06e3386ce283165;
the merged detached worktree stays at c2f271bbb229facd84b64e2a14dd72f0c4338272.
The v1.2.6 worktree and eleven checkpoints remain preserved.

Local mirror: /home/kas/kaspa-telegram-dev/local-git/kaspa-telegram-notify.git
origin fetch: https://github.com/KaspaPulse/kaspa-telegram-notify.git
origin push: local-first-push-disabled://KaspaPulse/kaspa-telegram-notify.git
Use FIX → TEST → VERIFY → REVIEW → LOCAL COMMIT → CONTINUE.
Development PG: kp-f12-dev-pg, loopback 55436; never use production data.
Reuse the existing internal Telegram TLS and Kaspa fixtures for artifact checks.
