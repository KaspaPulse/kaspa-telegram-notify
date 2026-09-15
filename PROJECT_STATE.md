# Project State — v1.2.10 Production verified stable

Repository: KaspaPulse/kaspa-telegram-notify
Development host: kas
Production host: dns
AGENTS.md host boundary remains authoritative.

## Reference release state

PRODUCTION_VERSION=1.2.10
PRODUCTION_SHA=631d64c5e11a8d05a072288aec6441e2308e4008
PRODUCTION_BINARY_SHA256=053cf6920285a6e6b8822f6f4d0869be296213e3f8872ab9e9f2b6be550066fd
PRODUCTION_STATUS=VERIFIED_STABLE
CURRENT_RELEASE=v1.2.10
SCHEMA_COMPATIBILITY_REMEDIATION=COMPLETE
V1_2_9_ARTIFACT=RETIRED_NOT_REUSABLE

The canonical successful Production attempt is
`20260915T164255.225210Z-1422640`, started at
`2026-09-15T16:42:56.083990+00:00`, with status `DEPLOYED_VERIFIED`.
Rollback was not required and was not performed for the successful attempt.

## Final Production contracts

Service is active/running with health `ok`, readiness `ready`, node connected,
subscription active and Telegram `PASS_GETME`. Final read-only verification found
zero fatal, DB, timeout, panic, worker and missing-created_at runtime errors.

`public.user_wallets.created_at` exists as `timestamp with time zone`, is NOT NULL,
has default `now()`, has zero NULL rows and preserves all five legacy rows.
`legacy_last_active_mismatch_rows=0`. The delivery-claim guard DB path passes.

## Closed incident facts

The v1.2.9 Production failure `column "created_at" does not exist` is fixed in v1.2.10.
`CREATED_AT_CONTRACT=PASS`, `DELIVERY_CLAIM_GUARD=PASS`, `NEW_DB_ERRORS=0`.

The known v1.2.3 shutdown-order defect was accepted only as a tightly fingerprinted,
PID/time-scoped baseline exception for the one-way upgrade. It is not a general policy
and MUST NOT be applied to v1.2.10 or later candidate shutdowns. The v1.2.10 candidate
shutdown contract passed cleanly with all owned tasks joined before DB close and no
forced kill.

A rendered/output view attributed `created_at_exists=false` to `migration.after`.
Canonical deployment JSON proves that false belongs only to pre-migration snapshots;
`migration.after`, `schema_after_migration_before_candidate` and `schema_final` are true.
This is a non-Production reporting/field-attribution artifact only. Do not redeploy for it.

## Durable closeout evidence

Evidence directory:
`/home/kas/kaspa-telegram-dev/evidence/KASPA_TELEGRAM_V1_2_10_FINAL_PRODUCTION_CLOSEOUT_20260915`

The evidence manifest records the canonical deploy-attempt path/hash, DB backup hash,
migration/operator/deployed-binary hashes, final schema/runtime verification,
observation receipts and rollback identity.

NEXT_ACTION=NONE_FOR_THIS_TASK
PRODUCTION_ACTION_REQUIRED=NO
Do not restart, redeploy, roll back, rerun the migration, mutate SQL/schema or replace
the binary unless a new material Production regression is independently established.

## Published closeout identity and verification matrix

CANONICAL_DEPLOY_ATTEMPT_ID=20260915T164255.225210Z-1422640
CANONICAL_DEPLOY_ATTEMPT_SHA256=c64b6a79e310683421b95d0fbf5b3cd6f9f8d5ccb02996e41a8b4ef273066b58
FINAL_EVIDENCE_MANIFEST_SHA256=9010b3403c0a26e628d14a433459e2a7615a502e17a0ecae111724bde8ac7ef2

V1_2_9_SCHEMA_DEFECT=FIXED_IN_V1_2_10
CREATED_AT_CONTRACT=PASS
DELIVERY_CLAIM_GUARD=PASS
HEALTH=PASS
READINESS=PASS
DB_CONNECTIVITY=PASS
NODE_CONNECTIVITY=PASS
SUBSCRIPTION_ACTIVE=YES
TELEGRAM_CONNECTIVITY=PASS
NEW_DB_ERRORS=0
RECENT_FATAL_ERRORS=0
RECENT_TIMEOUT_ERRORS=0
RECENT_PANICS=0
RECENT_WORKER_ERRORS=0
GRACEFUL_SHUTDOWN=PASS_CLEAN
FORCED_KILL_USED=NO
POST_DEPLOY_OBSERVATION=PASS
ROLLBACK_READY=YES
ROLLBACK_USED=NO

MIGRATION_BEFORE_CREATED_AT_EXISTS=false
MIGRATION_AFTER_CREATED_AT_EXISTS=true
SCHEMA_FINAL_CREATED_AT_EXISTS=true
MIGRATION_AFTER_FIELD_STALE_REPORTING_ARTIFACT=YES
EVIDENCE_REPORTING_DEFECT_FOUND=YES_NON_PRODUCTION_FIELD_ATTRIBUTION_ONLY
CANONICAL_JSON_SERIALIZATION_DEFECT=NO
PRODUCTION_SCHEMA_DEFECT=NO
