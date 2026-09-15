# Repository adoption — interruption-safe continuity

Policy ID: UNIVERSAL_CONTINUITY_V2_20260913
Repository: KaspaPulse/kaspa-telegram-notify
Status: adopted; current repository state is bound to PROJECT_STATE.md and verified evidence.

## Binding and canonical state

Development/Git/build/test/evidence work runs on `kas` only. Production is `dns` and
remains subject to the mandatory AGENTS.md environment boundary.

Read `PROJECT_STATE.md` at the start of every resumed task and reconcile it with actual
Git/runtime state. Keep task evidence/checkpoints under `/home/kas/kaspa-telegram-dev/evidence/`.
Do not create a competing shared CURRENT ledger or overwrite another task's checkpoint.

Current verified Production reference after the v1.2.10 closeout:
- version: `1.2.10`
- source SHA: `631d64c5e11a8d05a072288aec6441e2308e4008`
- release: `v1.2.10`
- status: `VERIFIED_STABLE`
- schema compatibility remediation: `COMPLETE`
- v1.2.9 artifact: `RETIRED_NOT_REUSABLE`

Final closeout evidence:
`/home/kas/kaspa-telegram-dev/evidence/KASPA_TELEGRAM_V1_2_10_FINAL_PRODUCTION_CLOSEOUT_20260915`

## Authorization and resume boundary

Continuity records facts; they do not grant new push, release or Production authority.
The proven v1.2.3 shutdown-order exception was baseline-only for the completed one-way
upgrade and must never be generalized to v1.2.10 candidate shutdowns.

For the completed closeout task, `NEXT_ACTION=NONE_FOR_THIS_TASK` and
`PRODUCTION_ACTION_REQUIRED=NO`. Any future Production action requires a new material
regression or new owner authorization and must begin by reconciling actual state.

Canonical v1.2.10 closeout identity:
- deployment attempt: `20260915T164255.225210Z-1422640`
- deployment attempt SHA-256: `c64b6a79e310683421b95d0fbf5b3cd6f9f8d5ccb02996e41a8b4ef273066b58`
- final evidence manifest SHA-256: `9010b3403c0a26e628d14a433459e2a7615a502e17a0ecae111724bde8ac7ef2`
- Production action after this documentation publication: none.
