# Project State — functional closure v1.2.6

Repository: KaspaPulse/kaspa-telegram-notify only.
Development host: kas. Worktree:
`/home/kas/kaspa-telegram-functional-closure-v126-20260912`
Branch: `fix/functional-closure-f01-f10-v1.2.6-20260912`
Base: `02ad378c1525f8841c8deadab7180ea96a51a612`
Resolve current HEAD and working-tree status with Git; do not substitute a historical SHA.

## Current task and source state

The existing F-01…F-10 remediation is implemented, including the final review
corrections to F-01 concurrent subscription safety, F-01 migration privileges,
and F-06 response length/non-recursive logging. The scoped report is
`FUNCTIONAL_CLOSURE_REPORT.md`.

The latest user instruction forbids any push to real GitHub. Work and
qualification remain local. Production deployment is outside this instruction.

## Commit-bound qualification and NEXT ACTION

Final validation is stored as a local Git note so recording the result does not
change the tested/built source SHA:

```bash
git status --short --branch
git rev-parse HEAD
git notes --ref=refs/notes/functional-closure-v126 show HEAD
```

Read that note and verify its `source_revision` equals actual HEAD. It records
the final test results, image identity, evidence directory, limitations and
`next_action`. The note is checkpointed to the existing local remote.

- If the note says `LOCAL_COMPLETE` and the worktree is clean, local remediation
  is complete. NEXT ACTION: retain the qualified checkpoint and await an explicit
  publication instruction. Do not rerun the audit or push to GitHub.
- If the note is missing or a gate is incomplete, NEXT ACTION: inspect the
  commit-specific evidence below and complete only the missing/failed gate.
  Do not infer qualification from source changes or this document alone.

Final evidence directory:
`/home/kas/kaspa-telegram-dev/evidence/KASPA_TELEGRAM_FUNCTIONAL_CLOSURE_V126_FINAL_20260912/<HEAD>/`

## Historical evidence and what must not be repeated

Historical candidate `5edc43cae00a33d00cfc703f95af1da5988743ef` had 269 passing
test executions (24 binaries; 213 unique test names), including F-01…F-10 10/10,
and an image/mock smoke. Its smoke continued past the quoted timeout.
These results are historical and do not qualify later source edits.

Evidence and continuation scripts:
`/home/kas/kaspa-telegram-dev/evidence/KASPA_TELEGRAM_FUNCTIONAL_CLOSURE_V126_20260912_5edc43c/resume/`

Confirmed recurring failure knowledge:

- Host PostgreSQL CLI tools are absent; use the task container's psql through
  `docker exec -i`. The Rust tests connect to loopback.
- Run every migration in lexical order. The F-01 migration must follow legacy
  schema creation and establish its complete runtime SELECT/DELETE contract.
  Do not replace missing migration grants with unexplained fixture grants.
- Telegram Bot::new uses its normal API hostname. The runtime fixture resolves
  api.telegram.org to a TLS mock and trusts the existing test CA bundle.
- The old permissive Telegram mock accepted oversized messages; the final
  mock enforces 4096 UTF-16 units. /logs must stay within 4000 including HTML.
- Do not count a filtered-out test as executed. Preserve commands, SHA and
  exit codes with the final test output.

Task-owned fixtures, if retained: PostgreSQL `kp-fc-resume-pg`, test DB
`kaspa_dev` on `127.0.0.1:55436`, separate runtime DB `kaspa_smoke`;
internal Docker network `kp-fc-runtime-net`; mock containers
`kp-fc-resume-tg` and `kp-fc-resume-node`. Inspect their state before reuse.
Synthetic environment files belong to evidence directories, never Git.

## Git and execution constraints

local fetch/push:
`/home/kas/kaspa-telegram-dev/local-git/kaspa-telegram-notify.git`
origin fetch:
`https://github.com/KaspaPulse/kaspa-telegram-notify.git`
origin push:
`local-first-push-disabled://KaspaPulse/kaspa-telegram-notify.git`

Preserve the branch, local mirror, history and existing checkpoints.
Use FIX → TEST → VERIFY → REVIEW → LOCAL COMMIT → CONTINUE.
Do not reset, discard, rebase, squash or force-push this work.
Do not alter any other repository's policies or worktrees.

AGENTS.md was read fully and governs the kas/dns boundary.
AGENTS.override.md was absent at recovery; check for newly introduced
instructions before future work. PROJECT_STATE.md was absent at recovery
and was created during this continuation. No PLANS.md was needed/read.

A later publication instruction must still follow AGENTS.md: protected CI
and merge, then build/deploy the exact merged SHA outside production.
Never deploy this local candidate merely because its local gates passed.
