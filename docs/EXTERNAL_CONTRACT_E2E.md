# External provider contract E2E

This layer complements, but does not replace, the required hermetic full-application E2E gate.

## Test layers

1. Unit/component tests run locally and in normal Rust CI.
2. Hermetic full-application E2E remains required for pull requests and protected merges.
3. External provider contracts run only from trusted `main`:
   - live Kaspa wRPC: scheduled and manual;
   - Telegram Test Environment: manual only, with dedicated test credentials.
4. Production acceptance remains read-only health/readiness/build-identity/provider-smoke validation.

External provider jobs are intentionally **not** triggered by `pull_request` or
`pull_request_target`. Untrusted pull-request code never receives external test
credentials.

## Live Kaspa contract

The Rust test is:

```bash
cargo test --locked --test kaspa_live_compatibility_tests \
  live_latest_kaspa_wrpc_contract -- --ignored --exact --nocapture
```

Provider selection:

- `KASPA_LIVE_WRPC_URL=resolver` (or an unset value) uses the Rusty Kaspa public
  resolver with Borsh wRPC.
- A non-empty explicit `KASPA_LIVE_WRPC_URL` uses that endpoint with JSON wRPC.
- `KASPA_EXPECTED_SERVER_VERSION` is optional. When set, the reported server
  version must contain the configured value.

The contract is read-only. It verifies server information, synchronization,
UTXO index availability, BlockDAG information, coin supply, network hashrate,
empty-address UTXO query compatibility, and a live
`VirtualDaaScoreChanged` subscription. Resolver, connection, RPC, and whole-test
timeouts are bounded.

Machine-readable evidence is written when
`EXTERNAL_CONTRACT_EVIDENCE_PATH` is set.

## Telegram Test Environment contract

Telegram's Test Environment requires a **dedicated test user and test bot**.
Never use the production bot token or a production/private user session for this
gate.

The workflow expects a GitHub Environment named:

```text
external-contract-e2e
```

with these environment secrets:

```text
TELEGRAM_TEST_BOT_TOKEN
TELEGRAM_TEST_CHAT_ID
```

`TELEGRAM_TEST_CHAT_ID` must identify the dedicated private Test Environment
user chat after that user has started the dedicated test bot.

The test sends only read-only Bot API methods to the Telegram Test Environment:

- `getMe`
- `getMyCommands`
- `getWebhookInfo`
- `getChat`

It does not send messages, change commands, configure a webhook, mutate chat
state, or touch production.

Telegram Test Environment requests use the official path form:

```text
https://api.telegram.org/bot<TOKEN>/test/METHOD_NAME
```

Transport failures, HTTP 5xx responses, and Telegram `429` responses are
retried with bounded backoff; Telegram `retry_after` is honored when present.
Errors never intentionally include the configured bot token, and evidence does
not record the configured chat ID.

Run locally only with dedicated test credentials:

```bash
EXTERNAL_CONTRACT_EVIDENCE_PATH=target/external-contract-evidence/telegram-test.json \
TELEGRAM_TEST_BOT_TOKEN='...' \
TELEGRAM_TEST_CHAT_ID='...' \
cargo test --locked --test telegram_test_environment_contract_tests \
  telegram_test_environment_contract -- --ignored --exact --nocapture
```

## GitHub Actions

`.github/workflows/external-contract-e2e.yml` has two jobs.

### Scheduled/manual Kaspa

The live Kaspa contract runs on the default branch on the schedule and on manual
dispatch. A repository variable can optionally override provider selection:

```text
KASPA_LIVE_WRPC_URL
KASPA_EXPECTED_SERVER_VERSION
```

No secret is required for public-resolver mode.

### Manual Telegram

The Telegram job runs only when all of the following are true:

- event is `workflow_dispatch`;
- `run_telegram=true`;
- ref is `refs/heads/main`;
- the `external-contract-e2e` environment releases its dedicated test secrets.

A missing token/chat ID fails closed without printing either value.

## Evidence and failure semantics

Successful jobs upload only provider-contract evidence. Evidence contains source
identity and non-secret provider results; credentials and configured chat ID are
excluded.

External provider failures do not weaken or replace the required hermetic E2E
merge gate. They indicate that the real provider contract needs investigation,
while normal pull-request testing remains deterministic and secret-free.
