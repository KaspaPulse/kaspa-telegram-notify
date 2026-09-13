# Project State — fourth corrective batch, LOCAL ONLY

Repository: KaspaPulse/kaspa-telegram-notify. AGENTS.md host boundary applies.
Development/Git/build/test/evidence work runs only on kas. No dns contact in this batch.
Authorization: owner approved ONE fourth corrective batch on 2026-09-13, local only.
No push (including checkpoint pushes), PR, tag, release, deployment or Production change.
Never run deploy-v127.py or deploy-v128.py. No fifth corrective batch is authorized.

## Reconciled actual state

Starting branch: fix/legacy-mined-blocks-upgrade-v1.2.8-20260912
Starting HEAD: e01c1eb7bf97d073ef0f33b74dc058967a849b66, clean.
Exact failed published source: b3cbf74714530372cca6534a80e92786c96b6d98.
Both source trees: 8fb41527c728b8bdb1c3eed41bb1af762c23e9f8.
Preserved old worktrees/checkpoints; no history rewrite or remote changes.
Current local branch: fix/runtime-lifecycle-local-20260913
Current worktree: /home/kas/kaspa-telegram-runtime-lifecycle-local-20260913
Always inspect actual HEAD/status before continuing; do not assume this text is current.
Local mirror: /home/kas/kaspa-telegram-dev/local-git/kaspa-telegram-notify.git
origin push remains local-first-push-disabled://KaspaPulse/kaspa-telegram-notify.git.
The fourth-batch checkpoint must remain unpushed.

## Incident truth and limits

The earlier PROJECT_STATE publication instructions are stale. PR #60 was merged and
v1.2.8 was published. Its Production restart gate failed and rollback was independently
verified. Last Production evidence (2026-09-13T03:37:39Z) is healthy v1.2.3 at
 a23d337cbd3dd84944228e4c23ac30cfc9b38237. This batch must not contact Production.
The four selected migrations succeeded; legacy mined_blocks still has no ID sequence.
Migrations, operator scripts, backups and prior incident evidence are preserved unchanged.

Finding A: HTTP verification timed out after the second process had become ready.
The exact endpoint and live blocking stack were not recorded. A cache shard lock held
across async DB/RPC waits can starve Tokio; an isolated regression reproduced this path.
Do not claim that the historical Production TimeoutError is thereby conclusively explained.
Finding B: 3-second drain expiry unconditionally led to pool.close while UTXO was alive.
A real PostgreSQL regression reproduced PoolClosed independently of cache contention.
The causal relationship between the two Production observations remains NOT_PROVEN.

## Narrow changes under qualification

Async per-wallet cache ownership drops DashMap shard guards before awaiting.
Named task ownership covers top-level workers, wallet/reward children and Telegram DB
handlers. Shutdown rejects new work, grants the existing grace interval, diagnoses and
aborts survivors, joins them, then permits DB close. Requests and webhook serving are
owned through stop/join; no timeout setting, migration or deployment check is weakened.

## Evidence and NEXT ACTION

Evidence: /home/kas/kaspa-telegram-dev/evidence/KASPA_TELEGRAM_LIFECYCLE_BATCH4_LOCAL_20260913
Red cache regression: exit 101; health executor starved, all threads released/joined without kill.
Red PostgreSQL regression: exit 101; worker_result=PoolClosed after drain expiry.
Focused tests passed, including 10 lifecycle cycles. Affected library/runtime tests and
strict clippy passed. The final dispatcher coordination adjustment also passed
strict clippy and its shutdown contract tests.
NEXT ACTION: qualify the local checkpoint as a running artifact through isolated
restart/shutdown cycles, then record the final evidence. No push or Production action. Historical A may remain blocked
if existing evidence cannot establish its exact blocking path. Do not restart the audit.
