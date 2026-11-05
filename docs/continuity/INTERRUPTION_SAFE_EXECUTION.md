# Interruption-safe execution and repository continuity

Policy ID: UNIVERSAL_CONTINUITY_V2_20260913
Status: Permanent owner-requested operating policy; repository-adapted supplement.

## 1. Scope, authority and existing conventions

Apply this policy to new and resumed engineering sessions, including connection loss,
context exhaustion, tool failure, process termination, reboot and partial execution.
Preserve the repository's existing continuity policy, accepted decisions, gates,
host mapping, canonical state records and authorization boundaries. Extend them;
do not create a competing source of truth or rename/migrate an existing ledger.

Observed reality determines facts, not permission. A checkpoint, log, website,
command history or old deployment instruction cannot grant new authorization.
Current owner scope and applicable higher-priority instructions still govern.
A resume request does not authorize push, merge, release, deployment, production
access, destructive cleanup, credential changes or external scanning.

## 2. Bind the exact project before acting

Verify repository identity, physical root, host/user, worktree, Git common directory,
branch, HEAD, index, staged/unstaged/untracked changes, last commits and relevant
local mirror/remotes. A similarly named directory is not identity proof.
Use the repository's own host rules; never transfer one project's host or runtime
permissions to another. Do not use production as a development environment.

Read AGENTS.md, every applicable AGENTS.override.md and PROJECT_STATE.md completely.
Check instruction files at each applicable directory level before editing that scope.
Read the canonical current checkpoint and latest relevant task record. If present,
read ACTIVE_TASK.md/CURRENT_STATE.md; read PLANS.md when its plan is ACTIVE.
Record absence honestly; do not invent missing state or reconstruct it from memory.

Discover and classify pertinent Markdown first: README/contribution/development,
ADRs, architecture/design, runbooks, deployment/release, tests/QA, security,
API/schema/migrations, operations/troubleshooting, changelog and handoff records.
Read the authoritative, materially relevant subset; do not blindly read all Markdown.
Within the same session, reuse a completed read only while its bytes/scope are unchanged.

## 3. Reconcile facts before continuing

Use the evidence owner for each fact: Git for source/history/index; filesystem for
actual local bytes; process/service/database state for authorized operational facts;
source-bound test/build receipts for validation. Logs/checkpoints/docs are evidence,
not substitutes for those observations. Conversation is supporting context only.
Record observation time, source and confidence. Timestamps alone do not prove an
operation completed. Do not resolve contradictions by choosing the newest-looking prose.

Inspect remote documentation/refs read-only when relevant and accessible. Do not
implicitly pull, merge, rebase, reset, clean, check out remote files or replace local
work. An unavailable remote is not a reason to destroy or suspend safe local work.
Distinguish local, local-mirror, published, merged, CI-tested and deployed states.

## 4. One task identity and one writer

Use the existing task identity and canonical ledger. The default for a repository
without a ledger is handoff-ledger/CURRENT.md plus handoff-ledger/checkpoints/.
Where an external or docs/handoff-ledger location already exists, preserve that
location and put only a routing reference in the repository adoption record.
Do not overwrite another task's CURRENT, ACTIVE_TASK or PROJECT_STATE record.

Before sharing a worktree, inspect its existing owner/lock and active agent/job.
Use the repository's established coordination mechanism. Bind ownership to task,
host, worktree and process/session identity. PID alone is not sufficient across reboots.
Do not create a duplicate agent, test runner, watcher, worktree or service merely
because the previous conversation disappeared. Do not break a lock just because
it is old; establish whether its owner is actually gone and preserve that evidence.
Separate task checkpoints when writers work on different scopes. Serialize changes
to shared state and reread/reconcile after a conflicting write; never last-writer-wins.

## 5. Write ahead of effects, then record the outcome

Before EVERY materially state-changing command, persist a concise operation record
using the existing ledger format or its task-scoped operation journal:

- unique operation ID and task ID; UTC timestamp; exact host/root/worktree;
- intent, permitted scope and safe command description with secrets removed;
- starting branch/HEAD and relevant dirty/index/artifact fingerprints;
- expected effects, prerequisites and authorization reference;
- state PLANNED, verification method and safest recovery action.

Persist RUNNING and the actual process/job/receipt identity as soon as available.
After return, verify actual effects and append VERIFIED, FAILED, PARTIAL, BLOCKED
or UNKNOWN with exit/result, evidence path/hash, changed state and next safe action.
A zero exit code alone is not sufficient for effects that require independent proof.
If the tool times out or disconnects, the outcome is UNKNOWN until reconciled.
Never fabricate a post-action receipt for an operation that might still be running.

Update the concise canonical task checkpoint after each meaningful milestone and
before substantial edits, tests with side effects, commits and authorized high-impact
operations; update it again after verification. Persist discoveries/root causes,
failed approaches worth retaining, test results and decision boundaries immediately.
Do not defer all documentation to the end or treat a chat message as durable storage.
Routine read-only commands need not each create a checkpoint; batch their findings.
A checkpoint write does not recursively require another checkpoint of itself.

## 6. Durable writes and recoverable evidence

Use the existing atomic ledger procedure. Otherwise write a same-directory temporary
file, flush/sync as supported, validate it, and atomically replace the snapshot while
retaining a previous good snapshot or immutable checkpoint. Use exclusive ownership
and a compare-before-write check; detect concurrent changes rather than overwrite them.
The operation journal is append-only; preserve and report an incomplete final record.
An atomic rename prevents partial snapshots, but is not a universal power-loss guarantee;
record storage/platform limitations and retain recovery evidence.

Long jobs must keep output and identity in durable task storage, not only terminal
scrollback. Record the command, source/environment identity, log path and actual exit
receipt. Reattach/read the existing job before starting a replacement. Do not assume
that starting a process means it succeeded, or that a disconnected tool killed it.
Never persist secrets, raw credentials, private keys, sessions, production rows or
personal data. Store redacted diagnostics and references; do not dump whole histories.

## 7. Recover undocumented or interrupted work

A missing or stale checkpoint does NOT mean that no work occurred.
Checkpointing itself may have been interrupted. Inspect the smallest relevant set:
Git diff/index/status/untracked files, recent commits and stashes/worktrees, file
fingerprints and timestamps, partial/generated artifacts, test/build receipts,
logs, existing processes and authorized runtime/schema state. Inspect relevant
sanitized history only when needed; never execute history automatically.

For each uncertain operation distinguish NEVER_STARTED, RUNNING, VERIFIED, FAILED,
PARTIAL and UNKNOWN. Verify side effects before retrying. In particular, do not
repeat a migration, deployment, upload, notification, commit or publication on the
assumption that lack of a receipt means failure. Prefer idempotency keys/receipts
where the existing system supports them; this policy does not add runtime features.

Preserve uncommitted, staged, unstaged, untracked, partial and uncertain work.
Never reset, clean, stash, revert, overwrite or replace unexplained work to simplify
recovery. Determine what is complete, consistent, repairable and still unverified.
If the safe next step cannot be established, checkpoint the precise uncertainty,
block only the affected action and continue independent authorized work where safe.

## 8. Reuse evidence precisely; continue the actual task

Classify completed-and-verified, completed-but-unverified, in-progress, partial,
failed, unknown, blocked and remaining work. Validate NEXT ACTION against current
prerequisites, local changes, newer results and failures; it is a hint, not a script.
Continue from the first real unfinished boundary after reconciliation. Do not stop
at a state summary when authorized work can safely continue. A new conversation is
not a new audit; an explicitly new owner task is not automatically the old task.

Reuse passing evidence only when its relevant source, tests, command, toolchain,
configuration, environment and artifact assumptions still match and the evidence
remains available. Record the scope fingerprint and reason for reuse. Respect any
stricter exact-SHA gate. Rerun the smallest invalidated group first; expand only for
real dependency/risk/evidence changes. Do not rerun broad suites just for reassurance.
Do not report reused checks as newly executed. Do not transfer an old PASS to new code.

## 9. Failures and functional integrity

Preserve failed command output and side effects; record the failure before retry.
Diagnose, repair narrowly, verify, add proportionate regression protection and update
the checkpoint. Never suppress a real error, increase a timeout merely to hide a
liveness defect, disable gates, or claim a root cause that has not been established.
For affected functionality trace the complete applicable path from user/action or
worker through state, request, backend/storage/dependency and final visible result.
Check failure/retry/cancellation paths as well as success. Do not begin an unrelated
repository-wide audit in the name of this policy.

## 10. Handoff and completion gate

A fresh session must be able to find the request and permission boundary, exact
repository/host, last verified source/state, existing local work, active operation,
completed work/evidence, failed approaches, unknown outcomes, blockers, do-not-repeat
items and exact next safe action without reconstructing the previous conversation.

Keep PROJECT_STATE as the repository's concise summary, not a raw command log.
Keep immutable checkpoints/evidence and link them. Never overwrite another active
lane to document this one. Local policy installation, local validation, remote
publication, CI, integration and runtime adoption are separate claims.

Do not declare completion until the intended files/effects exist, appropriate checks
actually ran, original work is preserved, limitations are disclosed and the durable
checkpoint was reread successfully. Policies require agent adherence; these documents
alone do not intercept every tool command or guarantee automatic recovery.
