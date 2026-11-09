use std::fs;

fn read_source(path: &str) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("failed to read {}: {}", path, e))
}

fn extract_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start_index = source
        .find(start)
        .unwrap_or_else(|| panic!("start marker not found: {}", start));

    let after_start = &source[start_index..];

    let end_index = after_start
        .find(end)
        .unwrap_or_else(|| panic!("end marker not found: {}", end));

    &after_start[..end_index]
}

#[test]
fn dag_candidate_missing_block_must_not_abort_search() {
    let source = read_source("src/network/analyze_dag.rs");
    assert!(source.contains("const MAX_CHAIN_BLOCKS: usize = 512"));
    assert!(source.contains("let mut visited = HashSet::new()"));
    assert!(source.contains("rpc_cl.get_block(current_hash, true).await.map_err("));
    assert!(source.contains("Selected-parent DAG block fetch failed for tx"));
    assert!(source.contains("no fabricated mined-block details"));
    assert!(!source.contains("for hash in &current_hashes"));
    // Current selected-parent-only design must fail closed if a required
    // chain block is unavailable. Old fanout-and-skip behavior was retired.
}

#[test]
fn dag_tip_lookup_must_not_silently_fallback_to_empty_tips() {
    let source = read_source("src/network/analyze_dag.rs");
    assert!(source.contains("get_block_dag_info().await.map_err("));
    assert!(source.contains("DAG tip lookup failed for tx"));
    assert!(!source.contains("Err(_) => vec![]"));
    assert!(!source.contains("get_block_dag_info().await.unwrap_or_default()"));
}

#[test]
fn dag_execute_must_not_use_silent_rpc_ok_or_unwrap_fallbacks() {
    let source = read_source("src/network/analyze_dag.rs");

    let execute_body = extract_between(
        &source,
        "pub async fn execute",
        "// Dependency Injection: Connection is shared",
    );

    assert!(
        !execute_body.contains(".await.ok()"),
        "execute() must not hide RPC errors with .ok()"
    );

    assert!(
        !execute_body.contains("unwrap_or_default()"),
        "execute() must not hide sensitive DAG data with unwrap_or_default()"
    );

    assert!(
        !execute_body.contains("unwrap_or(0)"),
        "execute() must not hide sensitive DAG data with unwrap_or(0)"
    );

    assert!(
        !execute_body.contains("if let Ok(block) = rpc_cl.get_block(*hash, true).await"),
        "candidate block fetch must not use silent if-let Ok"
    );
}

#[test]
fn blue_block_fetch_errors_are_not_silent_when_no_actual_block_is_found() {
    let source = read_source("src/network/analyze_dag.rs");

    assert!(
        source.contains("blue_block_fetch_errors"),
        "blue block fetch failures must be counted"
    );

    assert!(
        source.contains("Blue block fetch failed during mined block extraction"),
        "blue block fetch failures must become explicit when no actual mined block is found"
    );
}

#[test]
fn wallet_utxo_seen_state_must_not_silently_fallback_to_empty_db_state() {
    let source = read_source("src/wallet/wallet_use_cases.rs");

    assert!(
        !source.contains(
            ".get_seen_utxos(wallet_address)\n            .await\n            .unwrap_or_default()"
        ),
        "seen UTXO DB load must not silently fallback to empty state"
    );

    assert!(
        source.contains("seen_utxo_load_failed"),
        "seen UTXO load failures must be logged as DB errors"
    );

    assert!(
        source.contains(r#""action":"abort_wallet_scan""#),
        "seen UTXO load failure must abort wallet scan rather than continue with incomplete state"
    );
}

#[test]
fn live_balance_fallback_must_be_logged() {
    let source = read_source("src/wallet/wallet_use_cases.rs");
    assert!(
        source.contains("checked_snapshot_balance_sompi(utxos.iter().map(|utxo| utxo.amount))")
    );
    assert!(source.contains("UTXO snapshot balance overflow"));
    assert!(source.contains("let live_balance = snapshot_balance_sompi;"));
    assert!(source.contains("live_balance as f64 / 1e8"));
    assert!(!source.contains("node.get_balance(&wallet).await.map(|(b, _)| b).unwrap_or(0)"));
    assert!(source.contains("wallet_balance_read_failed"));
    // Snapshot overflow fails closed and is not silently rendered as zero.
}

#[test]
fn kaspa_adapter_must_not_unwrap_user_supplied_addresses() {
    let source = read_source("src/infrastructure/node/kaspa_adapter.rs");

    let function_body = extract_between(
        &source,
        "pub async fn get_utxos_by_addresses",
        "pub async fn connect",
    );

    assert!(
        !function_body.contains(".unwrap()"),
        "get_utxos_by_addresses must not unwrap address parsing"
    );

    assert!(
        function_body.contains("Invalid Kaspa address passed to get_utxos_by_addresses"),
        "invalid address parsing must return an explicit AppError"
    );
}

#[test]
fn reward_confirmation_gate_must_run_before_dag_analysis() {
    let source = read_source("src/wallet/wallet_use_cases.rs");

    assert!(
        source.contains("MIN_REWARD_CONFIRMATIONS"),
        "reward confirmation threshold must be configurable"
    );

    assert!(
        source.contains("get_virtual_daa_score"),
        "reward confirmation gate must use virtual DAA score"
    );

    assert!(
        source.contains("reward_confirmation_status"),
        "wallet flow must use the reward confirmation behavior helper"
    );

    let behavior_source = read_source("tests/reward_confirmation_behavior_tests.rs");

    assert!(
        behavior_source.contains("virtual_daa_behind_reward_daa_saturates_to_zero"),
        "behavior tests must verify saturating DAA confirmation behavior"
    );

    assert!(
        source.contains("reward_processing_decision")
            && source.contains("RewardProcessingDecision::ProcessNow"),
        "wallet flow must use reward_processing_decision before DAG analysis"
    );

    assert!(
        behavior_source.contains("coinbase_reward_at_required_confirmations_is_confirmed"),
        "behavior tests must verify rewards become confirmed at the configured threshold"
    );

    let before_join_set = extract_between(
        &source,
        "let utxos = self.node.get_utxos(wallet_address).await?",
        "let mut join_set =",
    );

    assert!(
        before_join_set.contains("continue;"),
        "unconfirmed rewards must stay unprocessed until they reach the confirmation threshold"
    );

    assert!(
        before_join_set.contains("new_rewards.push(utxo.clone())"),
        "confirmed rewards must still enter the DAG analysis path"
    );
}

#[test]
fn unconfirmed_rewards_must_not_be_marked_seen_before_processing() {
    let source = read_source("src/wallet/wallet_use_cases.rs");

    let loop_body = extract_between(&source, "for utxo in utxos", "known_mem.retain");

    assert!(
        loop_body.contains("if !reward_is_confirmed"),
        "the monitor must explicitly handle unconfirmed rewards"
    );

    assert!(
        loop_body.contains("continue;"),
        "unconfirmed rewards must not fall through into seen UTXO persistence"
    );

    assert!(
        loop_body.contains("current_outpoints_vec.push(utxo.outpoint.clone())"),
        "confirmed or already-seen UTXOs must still be persisted"
    );
}

#[test]
fn help_guide_must_include_current_commands_buttons_and_safety_policy() {
    let source = read_source("src/presentation/telegram/handlers/mod.rs");
    let readme = read_source("README.md");
    assert!(source.contains("🔔 Mining alerts arrive after confirmation."));
    assert!(source.contains("Wallets: send <code>kaspa:...</code>"));
    assert!(source.contains("🌐 Network: public PNN and BlockDAG health."));
    assert!(source.contains(r"/start — Home\n/help — Help"));
    assert!(source.contains(r"/start — الرئيسية\n/help — المساعدة"));
    assert!(readme.contains("only **`/start`** and **`/help`**"));
    // Admin-only operations are available through protected UI, never
    // advertised as additional slash commands in public /help.
    assert!(readme.contains("authorization-checked"));
}

#[test]
fn dag_candidate_skips_must_not_log_warn_per_block() {
    let source = read_source("src/network/analyze_dag.rs");
    assert!(source.contains("const MAX_CHAIN_BLOCKS: usize = 512"));
    assert!(source.contains("dag_block_in_reward_window"));
    assert!(source.contains("current_hash = verbose.selected_parent_hash;"));
    assert!(source.contains("Selected-parent DAG block fetch failed for tx"));
    assert!(!source.contains("for hash in &current_hashes"));
    assert!(!source.contains("Skipped unavailable DAG candidate blocks summary"));
    // Current architecture visits a bounded selected-parent chain; there
    // are no historical fanout candidates to skip/log.
}

#[test]
fn pending_rewards_must_be_persisted_for_unconfirmed_rewards() {
    let wallet = read_source("src/wallet/wallet_use_cases.rs");
    let repo = read_source("src/infrastructure/database/pending_rewards_repo.rs");
    assert!(wallet.contains("upsert_pending_reward"));
    assert!(wallet.contains("pending_reward_upsert_failed"));
    assert!(wallet.contains("upsert_awaiting_block_proof("));
    assert!(wallet.contains("complete_mining_proof("));
    assert!(wallet.contains("get_awaiting_block_proof("));
    assert!(wallet.contains("claim_awaiting_block_proof("));
    assert!(repo.contains("PRIMARY KEY (wallet, outpoint)"));
    assert!(repo.contains("AND first_seen_at >= TIMESTAMPTZ '2026-11-05 17:30:32+00'"));
    assert!(wallet.contains("Incomplete verified proof, no Telegram alert enqueued"));
    // Old 'pending_reward_ready_for_processing' logging marker was removed
    // when strict Coinbase proof entered durable outbox flow.
}

#[test]
fn pending_rewards_table_must_be_created_at_startup() {
    let main_source = read_source("src/main.rs");
    let repo_source = read_source("src/infrastructure/database/pending_rewards_repo.rs");
    let mod_source = read_source("src/infrastructure/database/mod.rs");

    assert!(
        main_source.contains("ensure_pending_rewards_table"),
        "pending_rewards table must be ensured at startup"
    );

    assert!(
        repo_source.contains("CREATE TABLE IF NOT EXISTS pending_rewards"),
        "pending_rewards repository must create the table"
    );

    assert!(
        repo_source.contains("PRIMARY KEY (wallet, outpoint)"),
        "pending_rewards must be idempotent per wallet/outpoint"
    );

    assert!(
        mod_source.contains("pending_rewards_repo"),
        "pending_rewards repository module must be registered"
    );
}

#[test]
fn reward_confirmation_behavior_tests_must_exist() {
    let source = read_source("tests/reward_confirmation_behavior_tests.rs");

    assert!(
        source.contains("coinbase_reward_below_required_confirmations_waits"),
        "behavior tests must verify unconfirmed coinbase rewards wait"
    );

    assert!(
        source.contains("coinbase_reward_at_required_confirmations_is_confirmed"),
        "behavior tests must verify rewards become confirmed at threshold"
    );

    assert!(
        source.contains("virtual_daa_behind_reward_daa_saturates_to_zero"),
        "behavior tests must verify saturating DAA confirmation behavior"
    );
}

#[test]
fn alert_dedup_behavior_tests_must_exist() {
    let behavior_source = read_source("tests/alert_dedup_behavior_tests.rs");
    let outbox_tests = read_source("tests/transactional_alert_outbox_tests.rs");
    let wallet_source = read_source("src/wallet/wallet_use_cases.rs");
    let queue_source = read_source("src/infrastructure/telegram_delivery_queue.rs");
    let monitor_source = read_source("src/presentation/telegram/workers/utxo_monitor.rs");

    assert!(
        behavior_source.contains("alert_key_prefers_mined_block_hash_when_available"),
        "behavior tests must verify mined block hash is preferred"
    );

    assert!(
        behavior_source.contains(
            "different_wallet_and_same_alert_key_is_not_duplicate_for_wallet_scoped_dedup"
        ),
        "behavior tests must verify dedup is wallet scoped"
    );

    assert!(
        outbox_tests.contains("repeated_commit_does_not_duplicate_queue_rows")
            && outbox_tests.contains("existing_dedup_without_queue_is_reconciled"),
        "transactional outbox tests must verify idempotency and reconciliation"
    );

    assert!(
        wallet_source.contains("build_alert_key"),
        "wallet flow must create a stable alert key"
    );

    assert!(
        queue_source.contains("INSERT INTO wallet_alert_dedup")
            && queue_source.contains("ON CONFLICT (wallet, alert_key) DO NOTHING")
            && monitor_source.contains("commit_alert_outbox"),
        "wallet-scoped dedup must be committed by the transactional outbox"
    );
}

#[test]
fn alert_delivery_behavior_tests_must_exist() {
    let behavior_source = read_source("tests/alert_delivery_behavior_tests.rs");
    let monitor_source = read_source("src/presentation/telegram/workers/utxo_monitor.rs");
    let worker_source = read_source("src/presentation/telegram/workers/telegram_delivery.rs");

    assert!(
        behavior_source.contains("successful_send_records_delivered"),
        "behavior tests must verify successful sends become delivered"
    );

    assert!(
        behavior_source.contains("failed_send_records_failed"),
        "behavior tests must verify failed sends become delivery failures"
    );

    assert!(
        monitor_source.contains("commit_alert_outbox")
            && !monitor_source.contains(".send_message("),
        "UTXO monitor must persist alerts without sending directly"
    );

    assert!(
        worker_source.contains(".send_message(")
            && worker_source.contains("mark_sent")
            && worker_source.contains("QUEUED ALERT DELIVERED"),
        "delivery worker must send queued messages and persist successful delivery"
    );

    assert!(
        worker_source.contains("mark_failed") && worker_source.contains("Queued alert send failed"),
        "delivery worker must persist delivery failures for retry"
    );
}

#[test]
fn blocks_history_must_be_full_paginated_and_env_configurable() {
    let repo = read_source("src/infrastructure/database/mined_blocks_repo.rs");
    let mining = read_source("src/presentation/telegram/handlers/mining.rs");
    let handlers = read_source("src/presentation/telegram/handlers/mod.rs");
    let wallet = read_source("src/wallet/wallet_use_cases.rs");
    assert!(repo.contains("get_all_daily_blocks"));
    assert!(
        !repo
            .split("get_all_daily_blocks")
            .nth(1)
            .unwrap()
            .contains("LIMIT 7")
    );
    assert!(wallet.contains("get_all_daily_blocks"));
    assert!(mining.contains("DEFAULT_BLOCKS_HISTORY_PAGE_SIZE: usize = 15"));
    assert!(mining.contains(r#"std::env::var("BLOCKS_HISTORY_PAGE_SIZE")"#));
    assert!(mining.contains("value.clamp(5, 50)"));
    assert!(mining.contains(".unwrap_or(DEFAULT_BLOCKS_HISTORY_PAGE_SIZE)"));
    assert!(mining.contains("history_page: usize"));
    assert!(mining.contains("div_ceil(page_size)"));
    assert!(mining.contains("blocks_history_markup"));
    assert!(mining.contains("Previous") && mining.contains("Next"));
    assert!(handlers.contains(r#"data.strip_prefix("wblk:")"#));
    assert!(handlers.contains("let token = parts.next().unwrap_or_default();"));
    // Production intentionally does not bundle a plaintext .env.example.
    // Code itself is authoritative for bounded environment configuration.
}

#[test]
fn delivery_queue_processing_status_constraint_fix_must_drop_legacy_constraint() {
    let migration_0006 =
        std::fs::read_to_string("migrations/0006_delivery_queue_processing_locks.sql")
            .expect("migration 0006 must be readable");
    let migration_0007 =
        std::fs::read_to_string("migrations/0007_delivery_queue_status_constraint_fix.sql")
            .expect("migration 0007 must be readable");

    assert!(
        migration_0006.contains("'processing'"),
        "migration 0006 must introduce processing status usage"
    );

    assert!(
        migration_0007.contains("DROP CONSTRAINT IF EXISTS ck_telegram_delivery_queue_status"),
        "migration 0007 must drop the legacy status constraint that blocks processing"
    );

    assert!(
        migration_0007.contains("ck_telegram_delivery_queue_status_v2"),
        "migration 0007 must ensure the v2 delivery queue status constraint exists"
    );

    assert!(
        migration_0007.contains("'processing'"),
        "migration 0007 must allow the processing status"
    );

    assert!(
        migration_0007.contains("VALIDATE CONSTRAINT ck_telegram_delivery_queue_status_v2"),
        "migration 0007 must validate the v2 constraint"
    );
}

#[test]
fn blocks_display_must_show_avg_kas_and_no_broken_daily_tree() {
    let repo = read_source("src/infrastructure/database/mined_blocks_repo.rs");
    let mining_handler = read_source("src/presentation/telegram/handlers/mining.rs");
    let wallet_use_cases = read_source("src/wallet/wallet_use_cases.rs");

    assert!(
        repo.contains("COALESCE(SUM(amount), 0)::BIGINT"),
        "mined block repository must sum mined KAS from amount"
    );

    assert!(
        wallet_use_cases.contains("blocks_1h_sompi")
            && wallet_use_cases.contains("blocks_24h_sompi")
            && wallet_use_cases.contains("blocks_7d_sompi")
            && wallet_use_cases.contains("lifetime_sompi"),
        "wallet block details must carry mined KAS totals"
    );

    assert!(
        mining_handler.contains("format_avg_per_hour")
            && mining_handler.contains("format_kas_from_sompi")
            && mining_handler.contains("Rates & KAS"),
        "/blocks display must show hourly averages and mined KAS"
    );

    assert!(
        !mining_handler.contains("â”œ <code>{}</code>: {} blocks"),
        "/blocks daily history must not use the broken mojibake tree prefix"
    );

    assert!(
        mining_handler.contains("format_daily_blocks_history_row"),
        "/blocks daily history rows must use the safe formatter"
    );
}

#[test]
fn blocks_kas_display_must_be_rounded_and_grouped_for_readability() {
    let mining_handler = read_source("src/presentation/telegram/handlers/mining.rs");

    assert!(
        mining_handler.contains("fn format_with_thousands"),
        "/blocks KAS display must use a thousands separator helper"
    );

    assert!(
        mining_handler.contains("format!(\"{:.2}\""),
        "/blocks KAS display must be rounded to two decimals"
    );

    assert!(
        mining_handler.contains("grouped_rev.push(',')"),
        "/blocks KAS display must include comma thousands separators"
    );

    assert!(
        !mining_handler.contains("while rendered.contains('.') && rendered.ends_with('0')"),
        "/blocks KAS display must not show long trimmed precision strings"
    );
}

#[test]
fn blocks_usd_value_must_use_stored_prices_only() {
    let mining_handler = read_source("src/presentation/telegram/handlers/mining.rs");
    let wallet_use_cases = read_source("src/wallet/wallet_use_cases.rs");
    let price_repo = read_source("src/infrastructure/database/kas_price_repo.rs");
    let background_jobs = read_source("src/application/background_jobs.rs");

    assert!(
        mining_handler.contains("format_kas_with_optional_usd"),
        "/blocks must format KAS with optional stored USD values"
    );

    assert!(
        mining_handler.contains("price_usd: Option<f64>"),
        "daily row formatter must accept optional stored daily price"
    );

    assert!(
        !mining_handler.contains("$n/a"),
        "/blocks must not display $n/a"
    );

    assert!(
        !mining_handler.contains("get_kaspa_market_data")
            && !mining_handler.contains("get_kaspa_usd_history")
            && !mining_handler.contains("CoinGecko"),
        "/blocks must not call live market APIs"
    );

    assert!(
        wallet_use_cases.contains("get_latest_kas_price_usd")
            && wallet_use_cases.contains("get_kas_price_usd_map_for_days"),
        "wallet block details must read stored KAS/USD prices from DB"
    );

    assert!(
        price_repo.contains("kas_price_history")
            && price_repo.contains("upsert_kas_price_usd")
            && price_repo.contains("get_missing_kas_price_days_for_mined_blocks"),
        "KAS price repository must persist and read stored daily prices"
    );

    assert!(
        background_jobs.contains("execute_kas_price_sync")
            && background_jobs.contains("upsert_kas_price_usd")
            && background_jobs.contains("get_kaspa_usd_history"),
        "background job must fetch and store prices outside /blocks"
    );
}

#[test]
fn kas_price_history_migration_must_exist() {
    let migration = read_source("migrations/0008_kas_price_history.sql");

    assert!(
        migration.contains("CREATE TABLE IF NOT EXISTS kas_price_history"),
        "kas_price_history migration must create the storage table"
    );

    assert!(
        migration.contains("price_usd NUMERIC")
            && migration.contains("CHECK (price_usd > 0)")
            && migration.contains("day DATE PRIMARY KEY"),
        "kas_price_history must enforce day primary key and positive USD price"
    );
}
#[test]
fn kas_price_history_migration_must_grant_application_roles_safely() {
    let migration = read_source("migrations/0008_kas_price_history.sql");

    assert!(
        migration.contains("kas_price_grants")
            && migration.contains("GRANT SELECT, INSERT, UPDATE ON TABLE kas_price_history")
            && migration.contains("pg_roles"),
        "kas_price_history migration must include safe grants for application DB roles"
    );
}

#[test]
fn build_provenance_must_be_embedded_and_propagated() {
    let build = read_source("src/build_info.rs");
    let main = read_source("src/main.rs");
    let metrics = read_source("src/infrastructure/metrics.rs");
    let docker = read_source("Dockerfile");
    let qualified = read_source(".github/workflows/qualified-source-build.yml");
    let ci = read_source(".github/workflows/ci.yml");
    assert!(build.contains("KASPA_PULSE_SOURCE_REVISION"));
    assert!(build.contains("kaspa_pulse_build_info"));
    assert!(main.contains("source_revision = build_info::source_revision()"));
    assert!(metrics.contains("crate::build_info::render_prometheus()"));
    assert!(docker.contains("ARG SOURCE_REVISION=unknown"));
    assert!(docker.contains("KASPA_PULSE_SOURCE_REVISION=${SOURCE_REVISION}"));
    assert!(docker.contains("org.opencontainers.image.revision=\"$SOURCE_REVISION\""));
    assert!(qualified.contains("KASPA_PULSE_SOURCE_REVISION: ${{ github.sha }}"));
    assert!(qualified.contains("test \"$(git rev-parse HEAD)\" = \"$GITHUB_SHA\""));
    assert!(qualified.contains("grep -a -F \"$GITHUB_SHA\" package/kaspa-block-alert-bot"));
    assert!(qualified.contains("cargo build --locked --release --bin kaspa-block-alert-bot"));
    assert!(ci.contains("cargo clippy --locked --all-targets -- -D warnings"));
}

#[test]
fn production_http_clients_must_be_fallible_and_versioned() {
    let runtime = read_source("src/infrastructure/resilience/runtime.rs");
    let system = read_source("src/infrastructure/external_services/system.rs");
    let coingecko = read_source("src/infrastructure/market/coingecko_adapter.rs");
    let main = read_source("src/main.rs");

    assert!(runtime.contains("pub fn build_http_client() -> Result<reqwest::Client, AppError>"));
    assert!(runtime.contains(r#"format!("KaspaPulse/{}", env!("CARGO_PKG_VERSION"))"#));
    assert!(!system.contains(r#"expect("failed to build HTTP client")"#));
    assert!(!coingecko.contains(r#"expect("failed to build HTTP client")"#));
    assert!(!system.contains("KaspaPulse/1.2"));
    assert!(!coingecko.contains("KaspaPulse/1.2"));
    assert!(coingecko.contains("pub fn new() -> Result<Self, AppError>"));
    assert!(main.contains("Arc::new(CoinGeckoAdapter::new()?)"));
    assert!(main.contains("spawn_price_monitor(") && main.contains(")?;"));
}

#[test]
fn docker_build_cache_is_arch_scoped_and_preserves_final_binary() {
    let docker = read_source("Dockerfile");
    assert!(docker.contains("ARG TARGETARCH"));
    assert!(docker.contains("ARG SOURCE_REVISION=unknown"));
    assert!(docker.contains("id=kaspa-pulse-registry-${TARGETARCH}"));
    assert!(docker.contains("id=kaspa-pulse-git-${TARGETARCH}"));
    assert!(docker.contains("id=kaspa-pulse-target-${TARGETARCH}"));
    assert!(docker.matches("sharing=locked").count() >= 3);
    assert!(docker.contains("cargo build --locked --release --bin kaspa-block-alert-bot"));
    assert!(docker.contains(
        "install -D -m 0755 target/release/kaspa-block-alert-bot /out/kaspa-block-alert-bot"
    ));
    assert!(docker.contains("COPY --from=builder --chown=kaspa:kaspa /out/kaspa-block-alert-bot /usr/local/bin/kaspa-block-alert-bot"));
    assert!(docker.contains("USER 10001:10001"));
}

#[test]
fn wallet_ui_must_fail_closed_when_wallet_database_reads_fail() {
    let wallet = read_source("src/presentation/telegram/handlers/wallet.rs");
    let handlers = read_source("src/presentation/telegram/handlers/mod.rs");

    assert!(!wallet.contains("get_list(cid).await.unwrap_or_default()"));
    assert!(!handlers.contains("get_list(cid).await.unwrap_or_default()"));
    assert!(
        !wallet.contains(".get_wallet_balances(cid)\n        .await\n        .unwrap_or_default()")
    );
    assert!(wallet.contains("Wallet data is temporarily unavailable."));
    assert!(wallet.contains("sanitize_for_log(&error.to_string())"));

    let list_handler = extract_between(
        &wallet,
        "pub async fn handle_list",
        "pub async fn handle_balance",
    );
    assert!(list_handler.contains("WALLET_DATA_UNAVAILABLE_MESSAGE"));
    assert!(!list_handler.contains("format!(\"❌ {}\", e)"));

    let balance_handler = extract_between(
        &wallet,
        "pub async fn handle_balance",
        "pub async fn handle_wallet_panel",
    );
    assert!(balance_handler.contains("WALLET_DATA_UNAVAILABLE_MESSAGE"));
    assert!(!balance_handler.contains("format!(\"❌ Error: {}\", e)"));

    assert!(handlers.contains("wallet::wallet_data_unavailable_message()"));
    assert!(handlers.contains("wallet::log_wallet_data_error(\"render_wallet_panel\", &error)"));
    assert!(
        handlers.contains("wallet::log_wallet_data_error(\"render_remove_wallet_panel\", &error)")
    );
}

#[test]
fn telegram_http_requests_use_central_policy_or_shared_price_cache() {
    let wallet = read_source("src/presentation/telegram/handlers/wallet.rs");
    let network = read_source("src/presentation/telegram/handlers/network.rs");
    let use_cases = read_source("src/wallet/wallet_use_cases.rs");
    assert!(!wallet.contains("reqwest::get("));
    assert!(!network.contains("reqwest::get("));
    assert!(wallet.contains("get_wallet_balances(cid).await"));
    assert!(wallet.contains("events_formatter::formatted_usd("));
    assert!(use_cases.contains("get_latest_kas_price_usd()"));
    assert!(use_cases.contains("get_kas_price_usd_map_for_days"));
    assert!(network.contains("build_http_client()"));
    assert!(network.contains("with_timeout_result("));
    assert!(network.contains("error_for_status()"));
    assert!(network.contains("sanitize_for_log(&error)"));
    // Values are read from the stored DB price service; no ad-hoc API
    // calls or obsolete price_cache.read().await.0 in wallet UI.
}

#[test]
fn initial_price_refresh_must_share_runtime_failure_state() {
    let system = read_source("src/infrastructure/external_services/system.rs");

    assert!(!system.contains("let _ = update_price_cache(&client, &ctx).await"));
    assert!(system.contains("let Some(initial_result)"));
    assert!(system.contains("run_until_cancelled("));
    assert!(system.contains("update_price_cache(&client, &ctx),"));
    assert!(system.contains("apply_price_refresh_result(\n            initial_result,"));
    assert!(system.matches("apply_price_refresh_result(").count() >= 3);
}

#[test]
fn mining_statistics_must_fail_closed_on_required_db_and_rpc_reads() {
    let mining = read_source("src/presentation/telegram/handlers/mining.rs");
    let stats = read_source("src/network/stats_use_cases.rs");
    let wallet = read_source("src/wallet/wallet_use_cases.rs");

    assert!(!mining.contains("sqlx::query_scalar(\"SELECT wallet FROM user_wallets"));
    assert!(
        !mining.contains(
            "get_wallet_blocks_details(cid)\n        .await\n        .unwrap_or_default()"
        )
    );
    assert!(mining.contains("Mining statistics are temporarily unavailable."));
    assert!(mining.contains("sanitize_for_log(&error.to_string())"));

    assert!(!stats.contains(
        "get_blocks_count_1h(wallet_address)\n            .await\n            .unwrap_or(0)"
    ));
    assert!(!stats.contains(
        "get_utxos(wallet_address)\n            .await\n            .unwrap_or_default()"
    ));

    assert!(wallet.contains("require_wallet_stats("));
    assert!(!wallet.contains("stats_1h_fallback"));
    assert!(!wallet.contains("lifetime_stats_fallback"));
    assert!(!wallet.contains("daily_blocks_fallback"));
    assert!(!wallet.contains("get_blocks_sum_sompi_1h(&wallet).await.unwrap_or(0)"));
}

#[test]
fn wallet_balance_ui_must_not_aggregate_rpc_failures_as_zero() {
    let source = read_source("src/wallet/wallet_use_cases.rs");

    assert!(!source.contains(
        "balance_sompi: 0,\n                        utxos: 0,\n                        is_online: false"
    ));
    assert!(source.contains("wallet_balance_read_failed"));
    assert!(source.contains(r#"{"operation":"get_balance","fallback":"none"}"#));
    assert!(source.contains("return Err(error);"));
}

#[test]
fn network_market_ui_must_not_render_failed_reads_as_zero() {
    let stats = read_source("src/network/stats_use_cases.rs");
    let node = read_source("src/infrastructure/node/kaspa_adapter.rs");
    let network = read_source("src/presentation/telegram/handlers/network.rs");
    assert!(!stats.contains(
        "get_kaspa_market_data()\n            .await\n            .unwrap_or((0.0, 0.0))"
    ));
    assert!(!stats.contains("get_network_hashrate().await.unwrap_or(0.0)"));
    assert!(!node.contains("get_connected_peer_info()\n            .await\n            .map(|p| p.peer_info.len())\n            .unwrap_or(0)"));
    assert!(!network.contains("live_bps.unwrap_or(0.0)"));
    assert!(network.contains("bps.map(|v| format!(\"{v:.2}\"))"));
    assert!(network.contains(r#"lang.label("Collecting", "قيد القياس")"#));
    assert!(network.contains("⚠️ Market data temporarily unavailable"));
    assert!(network.contains("sanitize_for_log(&error.to_string())"));
}
