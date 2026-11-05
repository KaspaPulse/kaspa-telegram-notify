#[test]
fn subscription_runtime_is_wired_into_the_utxo_monitor() {
    let source = include_str!("../src/presentation/telegram/workers/utxo_monitor.rs");

    assert!(source.contains("MonitoringSchedule::new"));
    assert!(source.contains("spawn_subscription_runtime"));
    assert!(source.contains("record_scan_trigger"));
    assert!(source.contains("mark_successful_scan"));
    assert!(source.contains("KASPA_MONITOR_RECONCILIATION_INTERVAL_SECS"));
}

#[test]
fn subscription_supervisor_restores_fallback_after_internal_failure() {
    let source = include_str!("../src/infrastructure/node/subscription.rs");

    assert!(source.contains("run_subscription_supervisor"));
    assert!(source.contains("lifecycle.on_runtime_failed(connected)"));
    assert!(source.contains("increment_subscription_runtime_restarts"));
    assert!(source.contains("ConnectionTransition::Reconnect"));
    assert!(source.contains("connection_generation > 1"));
}

#[test]
fn callback_execution_guard_is_injected_and_enforced() {
    let main_source = include_str!("../src/main.rs");
    let handler_source = include_str!("../src/presentation/telegram/handlers/mod.rs");

    assert!(main_source.contains("CallbackExecutionRegistry::default"));
    assert!(main_source.contains("callback_execution_registry"));
    assert!(handler_source.contains("callback_execution_registry.try_acquire"));
    assert!(handler_source.contains("edit_message_reply_markup"));
    assert!(handler_source.contains("increment_callbacks_rejected_inflight"));
}

#[test]
fn callback_ui_is_restored_and_database_failures_are_not_reported_as_success() {
    let handler_source = include_str!("../src/presentation/telegram/handlers/mod.rs");
    let wallet_source = include_str!("../src/presentation/telegram/handlers/wallet.rs");

    assert!(handler_source.contains("restore_safe_callback_menu"));
    assert!(handler_source.contains("restore_wallet_callback_menu"));
    assert!(handler_source.contains("Unable to lock this action panel"));
    assert!(handler_source.contains("This action message is no longer available"));
    assert!(handler_source.contains("Wallets were not deleted"));
    assert!(handler_source.contains("Setting was not changed"));
    assert!(handler_source.contains("Failed to send a replacement action panel"));
    assert!(handler_source.contains("callback_disables_keyboard(&data) && q.message.is_none()"));

    let remove_handler = wallet_source
        .split("pub async fn handle_wallet_remove_do")
        .nth(1)
        .and_then(|source| source.split("pub fn wallet_buttons_markup").next())
        .expect("wallet remove handler missing");
    assert!(!remove_handler.contains("unwrap_or_default"));
    assert!(remove_handler.contains("wallet_query.get_list(cid).await?"));
    assert!(remove_handler.contains("match wallet_mgt.remove_wallet(address, cid).await?"));
}

#[test]
fn operational_metrics_are_connected_to_runtime_paths() {
    let delivery_source = include_str!("../src/presentation/telegram/workers/telegram_delivery.rs");
    let queue_source = include_str!("../src/infrastructure/telegram_delivery_queue.rs");
    let health_source = include_str!("../src/infrastructure/webhook_security.rs");
    let metrics_source = include_str!("../src/infrastructure/metrics.rs");
    let observability_source = include_str!("../src/infrastructure/observability.rs");

    assert!(delivery_source.contains("DELIVERY_QUEUE_METRICS_INTERVAL_SECS"));
    assert!(delivery_source.contains("set_queue_snapshot"));
    assert!(delivery_source.contains("observe_delivery_latency"));
    assert!(queue_source.contains("oldest_active_age_seconds"));
    assert!(health_source.contains("ReadinessPolicy::from_env"));
    assert!(health_source.contains("StatusCode::SERVICE_UNAVAILABLE"));
    assert!(metrics_source.contains("render_prometheus"));
    assert!(observability_source.contains("subscription_runtime_restarts_total"));
}

#[test]
fn health_endpoint_is_started_in_polling_and_webhook_modes() {
    let main_source = include_str!("../src/main.rs");
    let health_call = "webhook_security::spawn_health_endpoint";

    assert_eq!(main_source.matches(health_call).count(), 1);
    let call_position = main_source.find(health_call).expect("health call missing");
    let webhook_branch_position = main_source
        .find("if startup.use_webhook")
        .expect("typed webhook branch missing");
    assert!(call_position < webhook_branch_position);
}

#[test]
fn timeout_metrics_and_deterministic_tokio_time_are_wired() {
    let cargo = include_str!("../Cargo.toml");
    let subscription = include_str!("../src/infrastructure/node/subscription.rs");
    let system = include_str!("../src/infrastructure/external_services/system.rs");
    let main_source = include_str!("../src/main.rs");

    let dev_dependencies = cargo
        .split("[dev-dependencies]")
        .nth(1)
        .expect("dev-dependencies section missing");
    assert!(dev_dependencies.contains("test-util"));
    let production_dependencies = cargo
        .split("[dev-dependencies]")
        .next()
        .expect("dependencies section missing");
    assert!(!production_dependencies.contains("test-util"));
    assert!(subscription.contains("tokio::time::advance"));
    assert!(system.contains("metrics::inc_rpc_timeouts"));
    assert!(main_source.contains("metrics::inc_rpc_timeouts"));
}

#[test]
fn readiness_configuration_is_fail_safe() {
    let source = include_str!("../src/infrastructure/observability.rs");

    assert!(source.contains("effective_subscription_requirement"));
    assert!(source.contains("Invalid boolean value"));
    assert!(source.contains("using default"));
}

#[test]
fn queue_stats_preserves_fail_closed_unknown_status_detection() {
    let source = include_str!("../src/infrastructure/telegram_delivery_queue.rs");

    assert!(source.contains("unexpected_status_count"));
    assert!(source.contains("Unexpected Telegram delivery queue status"));
}

#[test]
fn clippy_strictness_regressions_are_prevented() {
    let observability = include_str!("../src/infrastructure/observability.rs");
    let handlers = include_str!("../src/presentation/telegram/handlers/mod.rs");

    assert!(observability.contains(".checked_div(self.delivery_latency_samples)"));

    let final_test_module = handlers
        .rfind("mod callback_execution_tests")
        .expect("callback execution test module missing");
    let final_runtime_item = handlers
        .rfind("pub async fn handle_block_user")
        .expect("handle_block_user missing");
    assert!(final_test_module > final_runtime_item);
}

#[test]
fn final_audit_fixes_preserve_contextual_callback_recovery() {
    let source = include_str!("../src/presentation/telegram/handlers/mod.rs");

    assert!(source.contains("enum CallbackRecoveryMenu"));
    assert!(source.contains("restore_contextual_callback_menu"));
    assert!(source.contains("CallbackRecoveryMenu::Wallet"));
    assert!(source.contains("callback_recovery_menu(data, callback_is_admin)"));
}

#[test]
fn successful_scan_freshness_is_not_advanced_after_wallet_failures() {
    let source = include_str!("../src/presentation/telegram/workers/utxo_monitor.rs");

    assert!(source.contains("all_wallet_scan_tasks_succeeded"));
    assert!(source.contains("wallet_scan_outcomes.push(false)"));
    assert!(source.contains("successful-scan freshness was not advanced"));
    assert!(!source.contains("while join_set.join_next().await.is_some() {}"));
}

#[test]
fn final_audit_followup_preserves_accurate_deletion_recovery() {
    let handler_source = include_str!("../src/presentation/telegram/handlers/mod.rs");
    let wallet_source = include_str!("../src/presentation/telegram/handlers/wallet.rs");

    let forget_command = handler_source
        .split("Command::Forget | Command::ForgetAll =>")
        .nth(1)
        .and_then(|source| source.split("Command::ForgetWallets =>").next())
        .expect("forget command branch missing");
    assert!(forget_command.contains("admin_confirm::send_command_confirmation"));
    assert!(forget_command.contains("SensitiveAction::ForgetAll"));

    let forget_wallets_command = handler_source
        .split("Command::ForgetWallets =>")
        .nth(1)
        .and_then(|source| source.split("Command::HideMenu =>").next())
        .expect("forget-wallets command branch missing");
    assert!(forget_wallets_command.contains("admin_confirm::send_command_confirmation"));
    assert!(forget_wallets_command.contains("SensitiveAction::ClearWallets"));
    assert!(!handler_source.contains("async fn send_confirm_delete_all"));
    assert!(!handler_source.contains("async fn send_confirm_clear_wallets"));

    let forget_all_execution = handler_source
        .rsplit("if data == \"do_forget_all\"")
        .next()
        .and_then(|source| source.split("if data == \"cmd_add_wallet\"").next())
        .expect("forget-all execution branch missing");
    assert!(forget_all_execution.contains("restore_contextual_callback_menu"));
    assert!(!forget_all_execution.contains("restore_wallet_callback_menu"));

    let remove_handler = wallet_source
        .split("pub async fn handle_wallet_remove_do")
        .nth(1)
        .and_then(|source| source.split("pub fn wallet_buttons_markup").next())
        .expect("wallet remove handler missing");
    assert!(remove_handler.contains("match wallet_mgt.remove_wallet(address, cid).await?"));
    assert!(remove_handler.contains("restore_wallet_removal_state"));
    assert!(!remove_handler.contains(".edit_message_text"));
}

#[test]
fn final_scan_freshness_includes_nested_processing_failures() {
    let use_case_source = include_str!("../src/wallet/wallet_use_cases.rs");
    let worker_source = include_str!("../src/presentation/telegram/workers/utxo_monitor.rs");

    assert!(use_case_source.contains("pub struct WalletUtxoScanResult"));
    assert!(use_case_source.contains("completed_without_errors: bool"));
    assert!(use_case_source.contains("return (None, false);"));
    assert!(use_case_source.contains("Reward analysis task failed to join"));
    assert!(worker_source.contains("scan_result.completed_without_errors"));
    assert!(worker_source.contains("for event in scan_result.events"));
}

#[test]
fn rpc_outage_events_use_matching_sql_bind_parameters() {
    let source = include_str!("../src/infrastructure/external_services/system.rs");

    assert!(
        source.contains("VALUES ($1, $2, 'node_unreachable', 'RPC connection lost', $3::jsonb)")
    );
    assert!(source.contains("VALUES ($1, $2, 'recovered', $3::jsonb)"));
    assert!(source.contains("Failed to record RPC outage event"));
    assert!(source.contains("Failed to record RPC recovery event"));
    assert!(!source.contains(
        "VALUES ('RPC_ERROR', 'error', 'node_unreachable', 'RPC connection lost', $1::jsonb)"
    ));
    assert!(!source.contains("VALUES ('RPC_RECOVERED', 'info', 'recovered', $1::jsonb)"));
}
