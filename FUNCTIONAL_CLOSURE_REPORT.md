# Functional remediation closure — v1.2.6

This report continues the existing F-01…F-10 remediation. It records the
completed source changes and the boundaries of their verification. It is
not a new whole-application audit and does not claim every external workflow
has been exercised against live services.

The current candidate is the Git HEAD containing this report. Read its
qualification with:
`git notes --ref=refs/notes/functional-closure-v126 show HEAD`.
That note provides final counts, image ID, source revision, evidence hashes,
runtime assertions and remaining publication steps. A missing note means
final qualification is still incomplete.

## Remediation matrix

| ID | Function / location | Expected behavior and original failure | Mechanism corrected / components | Regression evidence |
|---|---|---|---|---|
| F-01 | Forget all user data; wallet confirmation flow | Remove allowed user linkage atomically, retain anonymized security history and other subscribers' wallet state. Original cleanup left data or assumed privileges/schema. | Transactional deletion and audit anonymization in wallets_repo.rs; final chronological migration grants its complete read/delete contract. Shared wallet locks serialize additions with orphan cleanup. | Existing PostgreSQL privacy/orphan regression; deterministic concurrent-subscriber regression; migration privilege checks; final suite. |
| F-02 | Wallet detail/remove and block pagination callbacks | An old button must stay bound to its original wallet. Numeric list indices could select another wallet after list changes. | Stable chat/wallet-bound callback tokens, current-ownership lookup and stale-token rejection in wallet/mining handlers. | Stale-list regression and updated pagination contract. |
| F-03 | Owner database diagnostics | Query failures must appear unavailable/degraded. A live connection previously masked failed queries as zero/healthy data. | Separate connection and query results in admin_diagnostics.rs and owner output. | PostgreSQL private-schema fault injection verifies DEGRADED and UNAVAILABLE. |
| F-04 | Owner Broadcast command/help | Advertised controls must perform their stated action. Broadcast had a partial/non-delivery implementation. | Broadcast command, handler and advertising removed under the established remediation scope. | No misleading command/handler/help contract regression. |
| F-05 | Owner restart command/button | Never claim a restart that did not occur. | Renamed to restart_info / Restart Info; information-only output; removed from sensitive-action confirmation. | Command/menu/authorization contracts; artifact reply and unchanged start identity. |
| F-06 | Owner /logs | Return actual recent service logs within Telegram limits. Old paths were absent; the initial buffer fix could still exceed the message limit and log its own output recursively. | Bounded sanitized tracing buffer; 4000 UTF-16 response budget including escaped HTML; newest events retained; explicit omission; direct reply avoids reinserting its body. | Buffer test; three long/Unicode/empty response regressions; strict mock artifact assertions for repeated /logs. |
| F-07 | Remove wallet | Distinguish actual removal from an absent/already-removed wallet. | Explicit Removed/NotFound repository outcome and corresponding user response. | PostgreSQL remove-twice regression. |
| F-08 | /help and registered owner menus | Help must match actual commands/buttons. | Aligned registered commands, help wording and owner menu entries. | Registered-command and actual-button contracts. |
| F-09 | Startup persisted settings | Missing rows get documented defaults; DB/schema/permission/invalid boolean errors stop startup. | Persisted runtime loader propagates errors and parses booleans strictly; main fails closed. | PostgreSQL missing-row/schema fault tests; final artifact invalid-boolean/restart assertion. |
| F-10 | Unknown commands and maintenance raw messages | Give explicit feedback instead of silence. | Unknown-command /help response and Maintenance Mode response in raw_message.rs. | Regression contracts and actual artifact/mock replies. |

## Additional evidence found during final review

F-01 concurrency — high severity: while one chat forgot a wallet, a second
chat could insert a subscription before committing. The orphan check could
not see that subscription and deleted shared state. A private-schema trigger
gate reproduced a successful new subscription with a missing shared row.
The corrected code uses transaction-scoped wallet advisory locks shared with
addition. It orders the actual PostgreSQL lock keys to avoid inverted lock
order for overlapping multiwallet operations. The same test verifies both
shared-state preservation and unrelated orphan deletion.

F-01 migration privileges — high severity for fresh installations: applying
the repository migrations and ordinary CI preparation left the runtime role
without DELETE on bot_event_log and other cleanup tables. The existing F-01
test failed with permission denied. The final F-01 migration now establishes
SELECT/DELETE on deletion tables plus SELECT and identity-column UPDATE for
audit anonymization. No new INSERT/TRUNCATE or schema-wide grant was added;
pre-existing audit grants are preserved.

F-06 long responses — medium severity: the initial log response could reach
25065 UTF-16 units. A permissive mock did not reject it. The corrected response
is bounded at 4000 units including HTML entities and supplementary Unicode.
Repeated log views no longer copy their response bodies into the buffer.

## Qualification and coverage limits

Final source checks: formatting, environment boundary, locked all-target/all-feature
check, strict clippy, complete Rust/PostgreSQL 18 suite and repository secret scan.
Final artifact checks: non-root UID/executable/writable state, build revision,
health/readiness/metrics, webhook secret rejection/acceptance, user and owner
replies, queue delivery with SQL state, long/repeated logs, maintenance restart,
invalid persisted boolean fail-closed behavior and graceful shutdown.
Only a PASS qualification note confirms completion of these gates.

The runtime environment uses a separate synthetic database and Telegram/Kaspa
mocks on an internal Docker network. It validates application request/response
behavior; it does not prove Telegram's live infrastructure, real blockchain
mining notifications, or live market-provider behavior. Real production
verification has not been performed or authorized in this continuation.
Non-applicable website/browser/account/email surfaces were not invented for
this Telegram bot.

The historical Rust run reports a future-compatibility notice for the existing
proc-macro-error2 2.0.1 dependency. Dependency versions were not changed by this
remediation; the notice is not a strict-clippy/test failure.

## Git outcome

All checkpoints stay on the existing branch and local mirror. No real GitHub
push, PR, merge, tag/release or deployment is part of this continuation.
PROJECT_STATE.md contains the exact recovery commands and points to the
commit-bound qualification note. Follow its NEXT ACTION.
