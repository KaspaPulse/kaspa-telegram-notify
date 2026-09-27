use super::*;
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
struct Scenario {
    id: String,
    feature: String,
    role: String,
    outcome: String,
    evidence: String,
}
#[derive(Clone, Debug)]
struct ResultRow {
    id: String,
    feature: String,
    outcome: String,
    executed: String,
    strategy: String,
    evidence: String,
    reason: String,
}

pub(super) fn execute(ctx: &ContextState) -> Result<()> {
    ctx.require_phase("restart")?;
    if ctx.state_is("phase.scenarios", "VERIFIED")? {
        ensure!(
            ctx.run_dir.join("scenario-results.csv").is_file()
                && ctx.run_dir.join("scenario-summary.json").is_file(),
            "scenario receipt exists but result artifacts are missing"
        );
        ctx.record(
            "RESUME:scenarios",
            "VERIFIED",
            "scenario receipt and artifacts agree; skipped replay",
        )?;
        return Ok(());
    }
    ctx.record(
        "SCENARIOS",
        "PLANNED",
        "execute BLOCKED matrix rows conservatively through exact candidate and Rust fixtures",
    )?;
    let matrix = ctx.repo.join("opqual/scenario-map.csv");
    let rows = read_matrix(&matrix)?;
    ensure!(
        rows.len() == 195,
        "scenario matrix size drift: {}",
        rows.len()
    );
    let mut results = BTreeMap::<String, ResultRow>::new();
    for row in &rows {
        if row.outcome != "BLOCKED" {
            results.insert(
                row.id.clone(),
                ResultRow {
                    id: row.id.clone(),
                    feature: row.feature.clone(),
                    outcome: row.outcome.clone(),
                    executed: "NO".into(),
                    strategy: "REUSED_QUALIFIED_EVIDENCE".into(),
                    evidence: row.evidence.clone(),
                    reason: "source-bound prior evidence retained".into(),
                },
            );
        }
    }
    let mut blocked = rows
        .iter()
        .filter(|r| r.outcome == "BLOCKED")
        .cloned()
        .collect::<Vec<_>>();
    blocked.sort_by_key(priority);
    let evidence_dir = ctx.run_dir.join("scenario-evidence");
    fs::create_dir_all(&evidence_dir)?;
    align_runtime_baseline(ctx)?;
    for row in blocked {
        let started = Instant::now();
        let r = match scenario(ctx, &row) {
            Ok(ev) => {
                let rel = format!("scenario-evidence/{}.json", row.id);
                atomic_json(
                    &ctx.run_dir.join(&rel),
                    &json!({"scenario":{"id":row.id,"feature_ids":row.feature},"runtime_evidence":ev,"duration_ms":started.elapsed().as_millis()}),
                )?;
                ResultRow {
                    id: row.id.clone(),
                    feature: row.feature.clone(),
                    outcome: "VERIFIED_PASS".into(),
                    executed: "YES".into(),
                    strategy: ev
                        .get("contract")
                        .and_then(Value::as_str)
                        .unwrap_or("runtime_contract")
                        .into(),
                    evidence: rel,
                    reason: String::new(),
                }
            }
            Err(e) => {
                let rel = format!("scenario-evidence/{}.failure.json", row.id);
                atomic_json(
                    &ctx.run_dir.join(&rel),
                    &json!({"scenario":{"id":row.id,"feature_ids":row.feature},"error":format!("{e:#}")}),
                )?;
                ResultRow {
                    id: row.id.clone(),
                    feature: row.feature.clone(),
                    outcome: "VERIFIED_FAIL".into(),
                    executed: "YES".into(),
                    strategy: "RUNTIME_CONTRACT_FAILED".into(),
                    evidence: rel,
                    reason: format!("{e:#}"),
                }
            }
        };
        println!("{}={} {}", r.id, r.outcome, r.reason);
        results.insert(row.id.clone(), r);
    }
    write_results(ctx, &rows, &results)?;
    let pass = results
        .values()
        .filter(|r| r.outcome == "VERIFIED_PASS")
        .count();
    let fail = results
        .values()
        .filter(|r| r.outcome == "VERIFIED_FAIL")
        .count();
    let blocked = results.values().filter(|r| r.outcome == "BLOCKED").count();
    let na = results
        .values()
        .filter(|r| r.outcome == "NOT_APPLICABLE")
        .count();
    let executed_pass = results
        .values()
        .filter(|r| r.executed == "YES" && r.outcome == "VERIFIED_PASS")
        .count();
    let edge001 = edge001(ctx)?;
    let journeys = executed_pass + usize::from(edge001);
    atomic_json(
        &ctx.run_dir.join("scenario-summary.json"),
        &json!({
            "total":rows.len(),"verified_pass":pass,"verified_fail":fail,"blocked":blocked,
            "not_applicable":na,"not_tested":0,"full_bot_journeys_completed":journeys,
            "supplemental_runs":if edge001 {vec!["EDGE-001"]} else {Vec::<&str>::new()}
        }),
    )?;
    ensure!(
        fail == 0,
        "scenario runner reported {fail} VERIFIED_FAIL rows"
    );
    ensure!(blocked == 0, "scenario runner left {blocked} BLOCKED rows");
    ensure!(
        journeys >= 150,
        "full bot journey threshold not met: {journeys}"
    );
    ctx.mark_phase("scenarios", "VERIFIED")?;
    ctx.record(
        "SCENARIOS",
        "VERIFIED",
        "scenario matrix has no BLOCKED/FAIL rows; full journey threshold satisfied",
    )?;
    Ok(())
}

fn scenario(ctx: &ContextState, row: &Scenario) -> Result<Value> {
    if row.id == "EDGE-002" {
        return edge002(ctx);
    }
    if row.id == "EDGE-003" {
        return edge003(ctx);
    }
    let (kind, name) = row
        .feature
        .split_once('-')
        .unwrap_or(("", row.feature.as_str()));
    match kind {
        "CMD" => command_contract(ctx, name),
        "CB" => callback_contract(ctx, name, &row.role),
        "HTTP" => http_contract(ctx, name),
        "INT" => integration_contract(ctx, name),
        "JOB" => job_contract(ctx, name),
        "TASK" => task_contract(ctx, name),
        "LIFE" => life_contract(ctx, name),
        _ => bail!("no execution strategy for {}", row.feature),
    }
}

fn command_contract(ctx: &ContextState, name: &str) -> Result<Value> {
    let admin = [
        "health",
        "stats",
        "sys",
        "pause",
        "resume",
        "mute_alerts",
        "unmute_alerts",
        "alerts_status",
        "restart_info",
        "logs",
        "events",
        "errors",
        "delivery",
        "subscribers",
        "wallet_events",
        "cleanup_events",
        "db_diag",
        "settings",
        "toggle",
    ]
    .contains(&name);
    let (uid, cid) = if admin {
        (SYNTH_ADMIN_ID, SYNTH_ADMIN_ID)
    } else {
        (SYNTH_USER_ID, SYNTH_CHAT_ID)
    };
    if name == "add" {
        delete_wallet(ctx, cid)?;
    }
    if name == "remove" {
        seed_wallet(ctx, cid)?;
    }
    let text = match name {
        "add" => format!("/add {SYNTH_WALLET}"),
        "remove" => format!("/remove {SYNTH_WALLET}"),
        "subscribers" | "wallet_events" => format!("/{name} {SYNTH_WALLET}"),
        "toggle" => "/toggle ENABLE_LIVE_SYNC".into(),
        _ => format!("/{name}"),
    };
    let start = event_count(&ctx.events_dir.join("telegram-events.jsonl"))?;
    inject_message(ctx, uid, cid, &text)?;
    let event = wait_output(ctx, start, cid, Duration::from_secs(15))?;
    if name == "add" {
        ensure!(wallet_count(ctx, cid)? == 1, "/add did not persist wallet")
    }
    if name == "remove" {
        ensure!(
            wallet_count(ctx, cid)? == 0,
            "/remove did not delete wallet"
        );
        seed_wallet(ctx, cid)?;
    }
    Ok(
        json!({"contract":if matches!(name,"add"|"remove"){"dispatcher_reply+db_state"}else{"dispatcher_reply"},"input":text,"telegram_event":event.get("sequence")}),
    )
}

fn callback_contract(ctx: &ContextState, name: &str, role: &str) -> Result<Value> {
    if let Some(command) = nonce_command(name) {
        return nonce_contract(ctx, name, command);
    }
    if let Some(key) = name.strip_prefix("btn_toggle_") {
        let before = setting(ctx, key)?;
        let first = toggle_via_confirmation(ctx, key, true)?;
        ensure!(setting(ctx, key)? != before, "{key} did not toggle");
        let second = toggle_via_confirmation(ctx, key, true)?;
        ensure!(setting(ctx, key)? == before, "{key} did not restore");
        return Ok(
            json!({"contract":"button_confirmation+db_toggle+confirmed_restore","confirmation":first,"restore":second}),
        );
    }
    let admin = name.starts_with("cmd_")
        && [
            "alerts_status",
            "cleanup_events",
            "db_diag",
            "delivery",
            "errors",
            "events",
            "health",
            "logs",
            "mute_alerts",
            "pause",
            "restart_info",
            "resume",
            "settings",
            "stats",
            "sys",
            "unmute_alerts",
        ]
        .contains(&name.trim_start_matches("cmd_"))
        || role.to_ascii_lowercase().contains("private admin");
    let (uid, cid) = if admin {
        (SYNTH_ADMIN_ID, SYNTH_ADMIN_ID)
    } else {
        (SYNTH_USER_ID, SYNTH_CHAT_ID)
    };
    let data = callback_data(name, cid);
    if ["wp", "wbal", "wblk", "wmin", "wrc", "wrd"].contains(&name) {
        seed_wallet(ctx, cid)?;
    }
    let before = event_count(&ctx.events_dir.join("telegram-events.jsonl"))?;
    let before_wallet = wallet_count(ctx, cid)?;
    let callback_update_id = inject_callback(ctx, uid, cid, &data, None)?;
    let ev = wait_output(ctx, before, cid, Duration::from_secs(15))?;
    if name == "wrd" {
        wait_wallet_count(
            ctx,
            cid,
            0,
            Duration::from_secs(15),
            "wrd did not remove wallet",
        )?;
        seed_wallet(ctx, cid)?;
        return Ok(json!({"contract":"wallet_token_callback+db_delete+restore","input":data}));
    }
    if [
        "do_forget_wallets",
        "do_forget_all",
        "do_pause",
        "do_resume",
        "do_cleanup_events",
        "do_mute_alerts",
        "do_unmute_alerts",
    ]
    .contains(&name)
    {
        wait_callback_answer(
            ctx,
            &format!("opqual-cb-{callback_update_id}"),
            "Confirmation expired",
            Duration::from_secs(15),
        )?;
        if name.contains("forget") {
            ensure!(
                wallet_count(ctx, cid)? == before_wallet,
                "expired callback mutated wallet state"
            )
        }
        return Ok(json!({"contract":"expired_sensitive_callback_fail_closed","input":data}));
    }
    Ok(
        json!({"contract":"dispatcher_callback_reply","input":data,"telegram_event":ev.get("sequence")}),
    )
}

fn nonce_command(name: &str) -> Option<&'static str> {
    match name {
        "confirm-pause" => Some("/pause"),
        "confirm-resume" => Some("/resume"),
        "confirm-cleanup_events" => Some("/cleanup_events"),
        "confirm-mute_alerts" => Some("/mute_alerts"),
        "confirm-unmute_alerts" => Some("/unmute_alerts"),
        "confirm-clear_wallets" => Some("/forget_wallets"),
        "confirm-forget_all" => Some("/forget_all"),
        "confirm-toggle_memory" => Some("/toggle ENABLE_MEMORY_CLEANER"),
        "confirm-toggle_live_sync" => Some("/toggle ENABLE_LIVE_SYNC"),
        "confirm-toggle_maintenance" => Some("/toggle MAINTENANCE_MODE"),
        _ => None,
    }
}
fn nonce_contract(ctx: &ContextState, name: &str, command: &str) -> Result<Value> {
    if matches!(name, "confirm-clear_wallets" | "confirm-forget_all") {
        seed_wallet(ctx, SYNTH_ADMIN_ID)?;
    }
    if name == "confirm-cleanup_events" {
        pg_admin_input(
            ctx,
            &format!(
                "INSERT INTO bot_event_log(event_type,severity,chat_id,status,metadata,created_at) VALUES ('OPQUAL_OLD','info',{SYNTH_ADMIN_ID},'old','{{}}'::jsonb,NOW()-INTERVAL '100 days');"
            ),
        )?;
    }
    let start = event_count(&ctx.events_dir.join("telegram-events.jsonl"))?;
    inject_message(ctx, SYNTH_ADMIN_ID, SYNTH_ADMIN_ID, command)?;
    let (row, payload) = wait_keyboard(ctx, start, "admin_do:", Duration::from_secs(15))?;
    let mid = row
        .get("response_message_id")
        .or_else(|| row.get("message_id"))
        .and_then(Value::as_i64)
        .context("confirmation message id missing")?;
    let cb = event_count(&ctx.events_dir.join("telegram-events.jsonl"))?;
    inject_callback(ctx, SYNTH_ADMIN_ID, SYNTH_ADMIN_ID, &payload, Some(mid))?;
    wait_any_text(
        ctx,
        cb,
        &[
            "Action completed",
            "Confirmed.",
            "deleted and verified",
            "Alerts resumed",
            "Alerts muted",
            "Cleanup",
            "Setting updated.",
        ],
        Duration::from_secs(15),
    )?;
    if matches!(name, "confirm-clear_wallets" | "confirm-forget_all") {
        ensure!(
            wallet_count(ctx, SYNTH_ADMIN_ID)? == 0,
            "confirmed privacy delete failed"
        );
        seed_wallet(ctx, SYNTH_ADMIN_ID)?;
    }
    if name == "confirm-cleanup_events" {
        ensure!(
            pg_u64(
                ctx,
                "SELECT count(*) FROM bot_event_log WHERE event_type='OPQUAL_OLD';"
            )? == 0,
            "cleanup did not delete old event"
        );
    }
    align_runtime_baseline(ctx)?;
    Ok(
        json!({"contract":"bound_nonce_confirmation+baseline_restored","input":payload,"confirmation_message_id":mid}),
    )
}

fn edge002(ctx: &ContextState) -> Result<Value> {
    delete_wallet(ctx, SYNTH_CHAT_ID)?;
    let s = event_count(&ctx.events_dir.join("telegram-events.jsonl"))?;
    inject_callback(ctx, SYNTH_USER_ID, SYNTH_CHAT_ID, "cmd_add_wallet", None)?;
    let _ = wait_output(ctx, s, SYNTH_CHAT_ID, Duration::from_secs(15))?;
    for i in 0..2 {
        let s = event_count(&ctx.events_dir.join("telegram-events.jsonl"))?;
        inject_message(
            ctx,
            SYNTH_USER_ID,
            SYNTH_CHAT_ID,
            &format!("not-a-wallet-{i}"),
        )?;
        wait_contains(ctx, s, "No Kaspa wallet found", Duration::from_secs(10))?;
        ensure!(
            wallet_count(ctx, SYNTH_CHAT_ID)? == 0,
            "invalid add created state"
        );
    }
    let s = event_count(&ctx.events_dir.join("telegram-events.jsonl"))?;
    inject_message(ctx, SYNTH_USER_ID, SYNTH_CHAT_ID, SYNTH_WALLET)?;
    let _ = wait_output(ctx, s, SYNTH_CHAT_ID, Duration::from_secs(15))?;
    ensure!(
        wallet_count(ctx, SYNTH_CHAT_ID)? == 1,
        "pending Add Wallet did not survive invalid inputs"
    );
    Ok(json!({"contract":"pending_add_survives_two_invalid_inputs_then_valid_add","attempts":2}))
}
fn edge003(ctx: &ContextState) -> Result<Value> {
    seed_wallet(ctx, SYNTH_CHAT_ID)?;
    align_setting(ctx, "MAINTENANCE_MODE", "false")?;
    toggle_via_confirmation(ctx, "MAINTENANCE_MODE", true)?;
    ensure!(
        setting(ctx, "MAINTENANCE_MODE")? == "true",
        "maintenance toggle-on failed"
    );
    let data = format!("wrd:{}", wallet_token(SYNTH_CHAT_ID));
    let s = event_count(&ctx.events_dir.join("telegram-events.jsonl"))?;
    inject_callback(ctx, SYNTH_USER_ID, SYNTH_CHAT_ID, &data, None)?;
    let _ = wait_output(ctx, s, SYNTH_CHAT_ID, Duration::from_secs(15))?;
    wait_wallet_count(
        ctx,
        SYNTH_CHAT_ID,
        0,
        Duration::from_secs(15),
        "maintenance-safe wrd did not remove wallet",
    )?;
    seed_wallet(ctx, SYNTH_CHAT_ID)?;
    if setting(ctx, "MAINTENANCE_MODE")? != "false" {
        toggle_via_confirmation(ctx, "MAINTENANCE_MODE", true)?;
    }
    ensure!(
        setting(ctx, "MAINTENANCE_MODE")? == "false",
        "maintenance state not restored"
    );
    Ok(
        json!({"contract":"maintenance_allows_existing_privacy_delete_callback+db_delete+confirmed_restore"}),
    )
}

fn http_contract(ctx: &ContextState, name: &str) -> Result<Value> {
    if name == "webhook" {
        return webhook_cycle(ctx);
    }
    let out = runtime::probe(
        ctx,
        &format!("http://{APP_CONTAINER}:{HEALTH_PORT}/{name}"),
        false,
    )?;
    ensure!(out.starts_with("STATUS=200\n"), "/{name} not HTTP 200");
    Ok(json!({"contract":"actual_internal_http_200","body":out}))
}
fn integration_contract(ctx: &ContextState, name: &str) -> Result<Value> {
    if name == "coingecko-history" {
        return coingecko_history_contract(ctx);
    }
    let (file, needle) = match name {
        "telegram" => ("telegram-events.jsonl", "telegram_request"),
        "kaspa-wrpc" => ("kaspa-events.jsonl", "kaspa_request"),
        "coingecko-current" => ("http-provider-events.jsonl", "api.coingecko.com"),
        "kaspa-price" => ("http-provider-events.jsonl", "/info/price"),
        "kaspa-marketcap" => ("http-provider-events.jsonl", "/info/marketcap"),
        "kaspa-fees" => ("http-provider-events.jsonl", "/info/fee-estimate"),
        _ => bail!("unknown integration {name}"),
    };
    let text = fs::read_to_string(ctx.events_dir.join(file)).unwrap_or_default();
    ensure!(text.contains(needle), "integration not exercised: {name}");
    Ok(json!({"contract":"actual_synthetic_provider_interaction","integration":name}))
}
fn job_contract(ctx: &ContextState, name: &str) -> Result<Value> {
    if name == "telegram_webhook_server" {
        return webhook_cycle(ctx);
    }
    let logs = app_logs()?;
    ensure!(
        logs.contains(&format!("[TASK START] {name}")),
        "task did not execute: {name}"
    );
    Ok(json!({"contract":"owned_task_started","marker":format!("[TASK START] {name}")}))
}
fn task_contract(ctx: &ContextState, name: &str) -> Result<Value> {
    let mut logs = app_logs()?;
    if name == "utxo_reward_analysis" && !logs.contains("[TASK START] utxo_reward_analysis") {
        reward_task_cycle(ctx)?;
        logs = app_logs()?;
    }
    ensure!(
        logs.contains(&format!("[TASK START] {name}")),
        "owned child task not observed: {name}"
    );
    ensure!(
        logs.contains(&format!("[TASK MONITOR] {name} joined cleanly")),
        "owned child task did not join cleanly: {name}"
    );
    Ok(
        json!({"contract":"owned_child_task_started_and_joined","marker":format!("[TASK START] {name}")}),
    )
}
fn life_contract(ctx: &ContextState, name: &str) -> Result<Value> {
    match name {
        "startup" => {
            let logs = app_logs()?;
            ensure!(
                logs.contains("Kaspa Pulse starting."),
                "startup marker missing"
            );
            ensure!(
                pg_u64(
                    ctx,
                    "SELECT count(*) FROM bot_event_log WHERE event_type='SYSTEM_START';"
                )? > 0,
                "SYSTEM_START missing"
            );
            Ok(json!({"contract":"main_startup_log+database_event"}))
        }
        "settings" => {
            for k in [
                "ENABLE_MEMORY_CLEANER",
                "ENABLE_LIVE_SYNC",
                "MAINTENANCE_MODE",
            ] {
                ensure!(!setting(ctx, k)?.is_empty(), "setting missing {k}")
            }
            Ok(json!({"contract":"persisted_settings_available_after_startup"}))
        }
        "shutdown" | "pool-close" => {
            let v: Value = serde_json::from_str(&fs::read_to_string(
                ctx.run_dir.join("shutdown-verification.json"),
            )?)?;
            for k in [
                "application_long_sql_lock_shutdown",
                "zero_app_db_sessions_after_shutdown",
                "zero_app_locks_after_shutdown",
                "clean_exit",
            ] {
                ensure!(
                    v.get(k).and_then(Value::as_str) == Some("PASS"),
                    "shutdown receipt failed {k}"
                )
            }
            if name == "pool-close" {
                ensure!(
                    app_logs()?.contains("Database connections closed safely"),
                    "pool close marker missing"
                )
            }
            Ok(json!({"contract":"exec01_shutdown_receipt","receipt":v}))
        }
        _ => bail!("unknown lifecycle {name}"),
    }
}

fn reward_task_cycle(ctx: &ContextState) -> Result<()> {
    let receipt = ctx.run_dir.join("reward-task-cycle.json");
    if receipt.exists() {
        let v: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
        if v.get("result").and_then(Value::as_str) == Some("PASS") {
            return Ok(());
        }
    }
    assert_owned_container(APP_CONTAINER)?;
    ensure!(
        docker_inspect(APP_CONTAINER, "{{.State.Running}}")? == "true",
        "primary app not running"
    );
    if container_exists(NODE_CONTAINER)? {
        assert_owned_container(NODE_CONTAINER)?;
        if docker_inspect(NODE_CONTAINER, "{{.State.Running}}")? == "true" {
            let _ = run_status(
                "sudo",
                ["-n", "docker", "kill", "--signal=TERM", NODE_CONTAINER],
            )?;
            wait_until(
                || Ok(docker_inspect(NODE_CONTAINER, "{{.State.Running}}")? == "false"),
                40,
                Duration::from_millis(250),
                "Kaspa fixture did not stop",
            )?;
        }
        run("sudo", ["-n", "docker", "rm", NODE_CONTAINER])?;
    }
    let txid = "1111111111111111111111111111111111111111111111111111111111111111";
    let outpoint = format!("{txid}:0");
    let baseline = "opqual-reward-baseline:0";
    pg_admin_input(
        ctx,
        &format!(
            "INSERT INTO user_wallets(wallet,chat_id) VALUES ('{}',{SYNTH_CHAT_ID}) ON CONFLICT(wallet,chat_id) DO NOTHING; DELETE FROM telegram_delivery_queue WHERE event_key='{outpoint}'; DELETE FROM wallet_alert_dedup WHERE wallet='{}' AND alert_key='{txid}'; DELETE FROM pending_rewards WHERE wallet='{}' AND outpoint='{outpoint}'; DELETE FROM wallet_seen_utxos WHERE wallet='{}'; INSERT INTO wallet_seen_utxos(wallet,outpoint) VALUES ('{}','{baseline}') ON CONFLICT(wallet,outpoint) DO UPDATE SET last_seen_at=NOW();",
            sql_literal(SYNTH_WALLET),
            sql_literal(SYNTH_WALLET),
            sql_literal(SYNTH_WALLET),
            sql_literal(SYNTH_WALLET),
            sql_literal(SYNTH_WALLET)
        ),
    )?;
    atomic_json(
        &ctx.control_dir.join("kaspa.json"),
        &json!({"utxos":[{
            "address":SYNTH_WALLET,"outpoint":{"transactionId":txid,"index":0},
            "utxoEntry":{"amount":100000000u64,"scriptPublicKey":"0000","blockDaaScore":1,"isCoinbase":false,"covenantId":Value::Null}
        }]}),
    )?;
    runtime::start_fixture(
        ctx,
        NODE_CONTAINER,
        "kaspa-wrpc",
        "kaspa",
        &["--network-alias", NODE_CONTAINER],
    )?;
    wait_until(
        || {
            Ok(app_logs()
                .unwrap_or_default()
                .contains("[TASK START] utxo_reward_analysis"))
        },
        160,
        Duration::from_millis(250),
        "utxo_reward_analysis did not start",
    )?;
    wait_until(
        || {
            Ok(app_logs()
                .unwrap_or_default()
                .contains("[TASK MONITOR] utxo_reward_analysis joined cleanly"))
        },
        80,
        Duration::from_millis(250),
        "utxo_reward_analysis did not join cleanly",
    )?;
    let logs = app_logs()?;
    ensure!(
        logs.contains("[TASK STOP] utxo_reward_analysis finished normally"),
        "reward task normal stop marker missing"
    );
    atomic_json(&ctx.control_dir.join("kaspa.json"), &json!({}))?;
    std::thread::sleep(Duration::from_secs(2));
    pg_admin_input(
        ctx,
        &format!(
            "DELETE FROM telegram_delivery_queue WHERE event_key='{outpoint}'; DELETE FROM wallet_alert_dedup WHERE wallet='{}' AND alert_key='{txid}'; DELETE FROM pending_rewards WHERE wallet='{}' AND outpoint='{outpoint}'; DELETE FROM wallet_seen_utxos WHERE wallet='{}' AND outpoint IN ('{baseline}','{outpoint}');",
            sql_literal(SYNTH_WALLET),
            sql_literal(SYNTH_WALLET),
            sql_literal(SYNTH_WALLET)
        ),
    )?;
    atomic_json(
        &receipt,
        &json!({"result":"PASS","synthetic_utxo":{"outpoint":outpoint,"is_coinbase":false},"task_start":true,"task_stop":true,"task_join":true,"provider":"local Rust Kaspa wRPC fixture","real_user_data":false}),
    )?;
    Ok(())
}

fn coingecko_history_contract(ctx: &ContextState) -> Result<Value> {
    let day = (Utc::now().date_naive() - chrono::Duration::days(1)).to_string();
    let outpoint = "opqual-history-synthetic:0";
    pg_admin_input(
        ctx,
        &format!(
            "DELETE FROM kas_price_history WHERE day='{day}'::date; DELETE FROM mined_blocks WHERE outpoint='{outpoint}'; INSERT INTO mined_blocks(wallet,outpoint,amount,daa_score,timestamp) VALUES ('{}','{outpoint}',100000000,1,'{day} 12:00:00+00'::timestamptz);",
            sql_literal(SYNTH_WALLET)
        ),
    )?;
    let file = ctx.events_dir.join("http-provider-events.jsonl");
    let start = event_count(&file)?;
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut observed = false;
    while Instant::now() < deadline {
        let text = events_since_file(&file, start)?;
        if text.contains("api.coingecko.com")
            && text.contains("/api/v3/coins/kaspa/market_chart/range")
        {
            observed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    ensure!(
        observed,
        "periodic KAS price sync did not exercise CoinGecko history within 20s"
    );
    ensure!(
        pg_u64(
            ctx,
            &format!(
                "SELECT count(*) FROM kas_price_history WHERE day='{day}'::date AND source='coingecko_history';"
            )
        )? > 0,
        "CoinGecko history response not persisted"
    );
    pg_admin_input(
        ctx,
        &format!(
            "DELETE FROM mined_blocks WHERE outpoint='{outpoint}'; DELETE FROM kas_price_history WHERE day='{day}'::date;"
        ),
    )?;
    Ok(json!({"contract":"periodic_system_task+actual_history_provider+db_backfill","day":day}))
}

fn edge001(ctx: &ContextState) -> Result<bool> {
    let control = ctx.control_dir.join("http.json");
    let original = fs::read(&control).unwrap_or_else(|_| b"{}\n".to_vec());
    let result = (|| -> Result<()> {
        for (label, mode) in [("missing", "missing"), ("negative", "invalid")] {
            atomic_json(
                &control,
                &json!({"api.kaspa.org/info/fee-estimate":{"mode":mode}}),
            )?;
            let hs = event_count(&ctx.events_dir.join("http-provider-events.jsonl"))?;
            let ts = event_count(&ctx.events_dir.join("telegram-events.jsonl"))?;
            inject_message(ctx, SYNTH_USER_ID, SYNTH_CHAT_ID, "/fees")?;
            wait_contains(
                ctx,
                ts,
                "Fee estimates are temporarily unavailable",
                Duration::from_secs(15),
            )?;
            let h = events_since_file(&ctx.events_dir.join("http-provider-events.jsonl"), hs)?;
            ensure!(
                h.contains("/info/fee-estimate"),
                "fee provider not observed {label}"
            );
            let t = events_since_file(&ctx.events_dir.join("telegram-events.jsonl"), ts)?;
            ensure!(
                !t.contains("<b>Priority:</b>") && !t.contains("sompi/gram"),
                "invalid fee rendered live"
            );
        }
        Ok(())
    })();
    fs::write(&control, original)?;
    result?;
    atomic_json(
        &ctx.run_dir.join("EDGE-001-full-dispatcher.json"),
        &json!({"scenario":"EDGE-001","execution_level":"FULL_MAIN_DISPATCHER","source_head":ctx.source_head,"binary_sha256":ctx.binary_sha256,"missing_case":"PASS","negative_case":"PASS","real_provider_traffic":false,"synthetic_only":true}),
    )?;
    Ok(true)
}

fn webhook_cycle(ctx: &ContextState) -> Result<Value> {
    let receipt = ctx.run_dir.join("webhook-cycle.json");
    if receipt.exists() {
        let v: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
        if v.get("result").and_then(Value::as_str) == Some("PASS") {
            return Ok(json!({"contract":"exact_candidate_webhook_cycle","receipt":v}));
        }
    }
    // Webhook is tested with exact binary and Rust HTTP client fixture; this intentionally creates only task-labelled internal resources.
    ensure!(
        !container_exists(WEBHOOK_CONTAINER)?,
        "unexpected existing webhook container"
    );
    let binary = ctx.binary.to_str().context("binary path")?;
    let mut args = vec![
        "-n",
        "docker",
        "run",
        "-d",
        "--name",
        WEBHOOK_CONTAINER,
        "--network",
        DOCKER_NETWORK,
        "--network-alias",
        WEBHOOK_CONTAINER,
        "--label",
        &format!("task={TASK_ID}"),
        "--label",
        "purpose=webhook-qualification",
        "--read-only",
        "--security-opt",
        "no-new-privileges:true",
        "--tmpfs",
        "/tmp:rw,noexec,nosuid,size=16m",
        "--mount",
        &format!("type=bind,src={binary},dst=/task/kaspa-pulse,readonly"),
        "--mount",
        &format!(
            "type=bind,src={},dst=/task/certs,readonly",
            ctx.certs_dir.display()
        ),
        "--entrypoint",
        "/task/kaspa-pulse",
        POSTGRES_IMAGE,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    let secret = "OPQUAL_WEBHOOK_SECRET_20260914_0123456789";
    let envs = [
        ("APP_ENV", "development".into()),
        ("RUST_LOG", "info".into()),
        ("ENABLE_VERBOSE_LOGS", "true".into()),
        (
            "DATABASE_URL",
            format!(
                "postgresql://{RUNTIME_ROLE}@{POSTGRES_CONTAINER}:5432/{DATABASE}?sslmode=disable&application_name={WEBHOOK_APPLICATION_NAME}"
            ),
        ),
        ("DB_MAX_CONNECTIONS", "4".into()),
        ("ALLOW_RUNTIME_SCHEMA_ENSURE", "false".into()),
        ("BOT_TOKEN", "1234567890:TEST_TOKEN".into()),
        ("ADMIN_ID", SYNTH_ADMIN_ID.to_string()),
        ("ADMIN_USER_ID", SYNTH_ADMIN_ID.to_string()),
        ("ADMIN_CHAT_ID", SYNTH_ADMIN_ID.to_string()),
        ("USE_WEBHOOK", "true".into()),
        ("WEBHOOK_DOMAIN", WEBHOOK_CONTAINER.into()),
        ("WEBHOOK_PORT", WEBHOOK_PORT.to_string()),
        ("WEBHOOK_BIND", "0.0.0.0".into()),
        ("WEBHOOK_ALLOW_PUBLIC_BIND", "true".into()),
        ("WEBHOOK_MAX_CONNECTIONS", "5".into()),
        ("WEBHOOK_SECRET_TOKEN", secret.into()),
        ("NODE_URL_01", format!("ws://{NODE_CONTAINER}:{NODE_PORT}")),
        ("KASPA_MONITOR_MODE", "polling_only".into()),
        ("READINESS_REQUIRE_NODE", "true".into()),
        ("READINESS_REQUIRE_SUBSCRIPTION", "false".into()),
        ("HEALTH_ENDPOINT_ENABLED", "true".into()),
        ("HEALTH_BIND", "0.0.0.0".into()),
        ("HEALTH_PORT", WEBHOOK_HEALTH_PORT.to_string()),
        ("HEALTH_ALLOW_PUBLIC_BIND", "true".into()),
        ("ENABLE_TELEGRAM_DELIVERY_QUEUE", "false".into()),
        ("KAS_PRICE_HISTORY_ENABLED", "false".into()),
        ("SSL_CERT_FILE", "/task/certs/ca.crt".into()),
    ];
    let insert = args.len() - 1;
    let mut n = 0;
    for (k, v) in envs {
        args.insert(insert + n, "-e".into());
        n += 1;
        args.insert(insert + n, format!("{k}={v}"));
        n += 1;
    }
    run("sudo", args)?;
    assert_owned_container(WEBHOOK_CONTAINER)?;
    assert_no_published_ports(WEBHOOK_CONTAINER)?;
    wait_until(
        || {
            Ok(
                run_capture("sudo", ["-n", "docker", "logs", WEBHOOK_CONTAINER])
                    .unwrap_or_default()
                    .contains("[WEBHOOK] Listening"),
            )
        },
        60,
        Duration::from_millis(250),
        "webhook server not ready",
    )?;
    let before = event_count(&ctx.events_dir.join("telegram-events.jsonl"))?;
    webhook_post(ctx, secret)?;
    wait_contains(ctx, before, "Kaspa Pulse Help", Duration::from_secs(15))?;
    let logs = run_capture("sudo", ["-n", "docker", "logs", WEBHOOK_CONTAINER])?;
    ensure!(
        logs.contains("[TASK START] telegram_webhook_server"),
        "webhook task marker missing"
    );
    run(
        "sudo",
        ["-n", "docker", "kill", "--signal=TERM", WEBHOOK_CONTAINER],
    )?;
    wait_until(
        || Ok(docker_inspect(WEBHOOK_CONTAINER, "{{.State.Running}}")? == "false"),
        120,
        Duration::from_millis(250),
        "webhook did not stop",
    )?;
    ensure!(
        docker_inspect(WEBHOOK_CONTAINER, "{{.State.ExitCode}}")? == "0",
        "webhook exit nonzero"
    );
    run("sudo", ["-n", "docker", "rm", WEBHOOK_CONTAINER])?;
    let out = json!({"result":"PASS","container":WEBHOOK_CONTAINER,"published_host_ports":0,"webhook_http":"PASS","dispatcher_help_response":"PASS","telegram_webhook_server_task":"PASS","exit_code":0,"synthetic_only":true,"production_contact":false});
    atomic_json(&receipt, &out)?;
    Ok(json!({"contract":"exact_candidate_webhook_cycle","receipt":out}))
}

fn webhook_post(ctx: &ContextState, secret: &str) -> Result<()> {
    let fixture = ctx.fixture_binary.to_str().context("fixture path")?;
    let body=json!({"update_id":990001,"message":{"message_id":990001,"date":Utc::now().timestamp(),"chat":{"id":SYNTH_CHAT_ID,"type":"private","first_name":"OpQualUser"},"from":{"id":SYNTH_USER_ID,"is_bot":false,"first_name":"OpQualUser","username":"opqual_user"},"text":"/help"}}).to_string();
    let out = run_capture(
        "sudo",
        vec![
            "-n".into(),
            "docker".into(),
            "run".into(),
            "--rm".into(),
            "--network".into(),
            DOCKER_NETWORK.into(),
            "--mount".into(),
            format!("type=bind,src={fixture},dst=/task/opqual-fixture,readonly"),
            "--entrypoint".into(),
            "/task/opqual-fixture".into(),
            POSTGRES_IMAGE.into(),
            "--mode".into(),
            "probe".into(),
            "--url".into(),
            format!("http://{WEBHOOK_CONTAINER}:{WEBHOOK_PORT}/webhook"),
            "--method".into(),
            "POST".into(),
            "--body".into(),
            body,
            "--header".into(),
            format!("X-Telegram-Bot-Api-Secret-Token: {secret}"),
            "--header".into(),
            "Content-Type: application/json".into(),
        ],
    )?;
    ensure!(
        out.starts_with("STATUS=200\n"),
        "webhook POST failed: {out}"
    );
    Ok(())
}

fn align_runtime_baseline(ctx: &ContextState) -> Result<()> {
    pg_admin_input(
        ctx,
        "INSERT INTO system_settings(key_name,value_data) VALUES ('ENABLE_ALERT_DELIVERY','true') ON CONFLICT(key_name) DO UPDATE SET value_data=EXCLUDED.value_data;",
    )?;
    seed_wallet(ctx, SYNTH_CHAT_ID)?;
    for (k, v) in [
        ("ENABLE_MEMORY_CLEANER", "false"),
        ("ENABLE_LIVE_SYNC", "true"),
        ("MAINTENANCE_MODE", "false"),
    ] {
        align_setting(ctx, k, v)?;
    }
    Ok(())
}
fn align_setting(ctx: &ContextState, key: &str, target: &str) -> Result<()> {
    let _ = toggle_via_confirmation(ctx, key, false)?;
    if setting(ctx, key)? != target {
        let _ = toggle_via_confirmation(ctx, key, false)?;
    }
    ensure!(
        setting(ctx, key)? == target,
        "could not align {key}={target}"
    );
    Ok(())
}
fn toggle_via_confirmation(ctx: &ContextState, key: &str, require_delta: bool) -> Result<Value> {
    let before = setting(ctx, key)?;
    let start = event_count(&ctx.events_dir.join("telegram-events.jsonl"))?;
    inject_callback(
        ctx,
        SYNTH_ADMIN_ID,
        SYNTH_ADMIN_ID,
        &format!("btn_toggle_{key}"),
        None,
    )?;
    let prefix = match key {
        "ENABLE_MEMORY_CLEANER" => "admin_do:toggle_memory:",
        "ENABLE_LIVE_SYNC" => "admin_do:toggle_live_sync:",
        "MAINTENANCE_MODE" => "admin_do:toggle_maintenance:",
        _ => bail!("unknown toggle"),
    };
    let (row, payload) = wait_keyboard(ctx, start, prefix, Duration::from_secs(15))?;
    let mid = row
        .get("response_message_id")
        .or_else(|| row.get("message_id"))
        .and_then(Value::as_i64)
        .context("toggle message id missing")?;
    let s = event_count(&ctx.events_dir.join("telegram-events.jsonl"))?;
    inject_callback(ctx, SYNTH_ADMIN_ID, SYNTH_ADMIN_ID, &payload, Some(mid))?;
    wait_contains(ctx, s, "Setting updated.", Duration::from_secs(15))?;
    let after = setting(ctx, key)?;
    if require_delta {
        ensure!(after != before, "{key} did not change")
    }
    Ok(json!({"before":before,"after":after,"confirmation":payload,"message_id":mid}))
}

fn callback_data(name: &str, cid: i64) -> String {
    let token = wallet_token(cid);
    match name {
        "wp" => format!("wp:{token}"),
        "wbal" => format!("wbal:{token}"),
        "wblk" => format!("wblk:{token}:0"),
        "wmin" => format!("wmin:{token}"),
        "wrc" => format!("wrc:{token}"),
        "wrd" => format!("wrd:{token}"),
        x if x.starts_with("legacy-") => {
            format!("{}opqual-stale-payload", x.trim_start_matches("legacy-"))
        }
        x => x.to_owned(),
    }
}
fn wallet_token(cid: i64) -> String {
    let mut h = Sha256::new();
    h.update(format!("wallet-callback-v1:{cid}:{SYNTH_WALLET}"));
    h.finalize()
        .iter()
        .take(16)
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn seed_wallet(ctx: &ContextState, cid: i64) -> Result<()> {
    pg_admin_input(
        ctx,
        &format!(
            "INSERT INTO user_wallets(wallet,chat_id) VALUES ('{}',{cid}) ON CONFLICT(wallet,chat_id) DO NOTHING;",
            sql_literal(SYNTH_WALLET)
        ),
    )
}
fn delete_wallet(ctx: &ContextState, cid: i64) -> Result<()> {
    pg_admin_input(
        ctx,
        &format!(
            "DELETE FROM user_wallets WHERE wallet='{}' AND chat_id={cid};",
            sql_literal(SYNTH_WALLET)
        ),
    )
}
fn wallet_count(ctx: &ContextState, cid: i64) -> Result<u64> {
    pg_u64(
        ctx,
        &format!(
            "SELECT count(*) FROM user_wallets WHERE wallet='{}' AND chat_id={cid};",
            sql_literal(SYNTH_WALLET)
        ),
    )
}
fn wait_wallet_count(
    ctx: &ContextState,
    cid: i64,
    expected: u64,
    timeout: Duration,
    context: &str,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        let actual = wallet_count(ctx, cid)?;
        if actual == expected {
            return Ok(());
        }
        ensure!(
            Instant::now() < deadline,
            "{context}: expected wallet count {expected}, observed {actual}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
fn setting(ctx: &ContextState, key: &str) -> Result<String> {
    Ok(pg_admin(
        ctx,
        &format!(
            "SELECT value_data FROM system_settings WHERE key_name='{}';",
            sql_literal(key)
        ),
    )?
    .trim()
    .to_owned())
}
fn pg_u64(ctx: &ContextState, sql: &str) -> Result<u64> {
    Ok(pg_admin(ctx, sql)?.trim().parse().unwrap_or(0))
}
fn app_logs() -> Result<String> {
    Ok(run_capture("sudo", ["-n", "docker", "logs", APP_CONTAINER]).unwrap_or_default())
}

fn inject_message(ctx: &ContextState, uid: i64, cid: i64, text: &str) -> Result<u64> {
    let p = ctx.control_dir.join("telegram-updates.jsonl");
    wait_update_queue_drained(ctx, &p)?;
    let id = next_update(&p)?;
    append_jsonl(
        &p,
        &json!({"update_id":id,"message":{"message_id":id,"date":Utc::now().timestamp(),"chat":{"id":cid,"type":"private","first_name":"OpQualUser"},"from":{"id":uid,"is_bot":false,"first_name":"OpQualUser","username":"opqual_user"},"text":text}}),
    )?;
    Ok(id)
}
fn inject_callback(
    ctx: &ContextState,
    uid: i64,
    cid: i64,
    data: &str,
    mid: Option<i64>,
) -> Result<u64> {
    let p = ctx.control_dir.join("telegram-updates.jsonl");
    wait_update_queue_drained(ctx, &p)?;
    let id = next_update(&p)?;
    let m = mid.unwrap_or(id as i64);
    append_jsonl(
        &p,
        &json!({"update_id":id,"callback_query":{"id":format!("opqual-cb-{id}"),"from":{"id":uid,"is_bot":false,"first_name":"OpQualUser","username":"opqual_user"},"message":{"message_id":m,"date":Utc::now().timestamp(),"chat":{"id":cid,"type":"private","first_name":"OpQualUser"},"from":{"id":1234567890i64,"is_bot":true,"first_name":"OpQualBot","username":"opqual_bot"},"text":"synthetic callback origin"},"chat_instance":"opqual-chat-instance","data":data}}),
    )?;
    Ok(id)
}
fn max_update_id(p: &Path) -> Result<u64> {
    let mut max_id = 0;
    if p.exists() {
        for line in fs::read_to_string(p)?.lines() {
            if let Ok(v) = serde_json::from_str::<Value>(line) {
                max_id = max_id.max(v.get("update_id").and_then(Value::as_u64).unwrap_or(0));
            }
        }
    }
    Ok(max_id)
}

fn output_event_count(ctx: &ContextState) -> Result<usize> {
    Ok(events(ctx)?
        .into_iter()
        .filter(|v| {
            matches!(
                v.get("method")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .as_str(),
                "sendmessage"
                    | "editmessagetext"
                    | "editmessagereplymarkup"
                    | "answercallbackquery"
            )
        })
        .count())
}

fn wait_update_queue_drained(ctx: &ContextState, p: &Path) -> Result<()> {
    let latest = max_update_id(p)?;
    if latest == 0 {
        return Ok(());
    }
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut stable_count = None;
    let mut stable_since = Instant::now();
    loop {
        let drained = events(ctx)?.into_iter().rev().any(|v| {
            if v.get("method").and_then(Value::as_str) != Some("getupdates") {
                return false;
            }
            let offset = v
                .get("offset")
                .and_then(|x| {
                    x.as_u64()
                        .or_else(|| x.as_str().and_then(|s| s.parse().ok()))
                })
                .unwrap_or(0);
            offset > latest
        });
        if drained {
            let count = output_event_count(ctx)?;
            match stable_count {
                Some(previous) if previous == count => {
                    if stable_since.elapsed() >= Duration::from_millis(350) {
                        return Ok(());
                    }
                }
                _ => {
                    stable_count = Some(count);
                    stable_since = Instant::now();
                }
            }
        } else {
            stable_count = None;
            stable_since = Instant::now();
        }
        ensure!(
            Instant::now() < deadline,
            "Telegram update queue did not drain through update {latest}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn next_update(p: &Path) -> Result<u64> {
    let mut m = 10000;
    if p.exists() {
        for l in fs::read_to_string(p)?.lines() {
            if let Ok(v) = serde_json::from_str::<Value>(l) {
                m = m.max(v.get("update_id").and_then(Value::as_u64).unwrap_or(0))
            }
        }
    }
    Ok(m + 1)
}
fn event_count(p: &Path) -> Result<usize> {
    Ok(if p.exists() {
        fs::read_to_string(p)?
            .lines()
            .filter(|x| !x.trim().is_empty())
            .count()
    } else {
        0
    })
}
fn events(ctx: &ContextState) -> Result<Vec<Value>> {
    Ok(
        fs::read_to_string(ctx.events_dir.join("telegram-events.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|x| serde_json::from_str(x).ok())
            .collect(),
    )
}
fn events_since_file(p: &Path, start: usize) -> Result<String> {
    Ok(fs::read_to_string(p)
        .unwrap_or_default()
        .lines()
        .skip(start)
        .collect::<Vec<_>>()
        .join("\n"))
}
fn wait_callback_answer(
    ctx: &ContextState,
    callback_query_id: &str,
    needle: &str,
    timeout: Duration,
) -> Result<Value> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(event) = events(ctx)?.into_iter().rev().find(|value| {
            value.get("method").and_then(Value::as_str) == Some("answercallbackquery")
                && value.get("callback_query_id").and_then(Value::as_str) == Some(callback_query_id)
                && value
                    .get("text")
                    .and_then(Value::as_str)
                    .is_some_and(|text| text.contains(needle))
        }) {
            return Ok(event);
        }
        ensure!(
            Instant::now() < deadline,
            "timeout waiting for callback answer {callback_query_id} containing {needle:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn wait_output(ctx: &ContextState, start: usize, cid: i64, timeout: Duration) -> Result<Value> {
    let d = Instant::now() + timeout;
    loop {
        for v in events(ctx)?.into_iter().skip(start) {
            let method = v
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_lowercase();
            if [
                "sendmessage",
                "editmessagetext",
                "editmessagereplymarkup",
                "answercallbackquery",
            ]
            .contains(&method.as_str())
                && (method == "answercallbackquery"
                    || v.get("chat_id").and_then(Value::as_i64) == Some(cid))
            {
                return Ok(v);
            }
        }
        ensure!(Instant::now() < d, "no Telegram output for chat {cid}");
        std::thread::sleep(Duration::from_millis(100))
    }
}
fn wait_contains(
    ctx: &ContextState,
    start: usize,
    needle: &str,
    timeout: Duration,
) -> Result<Value> {
    let d = Instant::now() + timeout;
    loop {
        for v in events(ctx)?.into_iter().skip(start) {
            if v.to_string().contains(needle) {
                return Ok(v);
            }
        }
        ensure!(Instant::now() < d, "Telegram output missing {needle:?}");
        std::thread::sleep(Duration::from_millis(100))
    }
}
fn wait_any_text(
    ctx: &ContextState,
    start: usize,
    needles: &[&str],
    timeout: Duration,
) -> Result<Value> {
    let d = Instant::now() + timeout;
    loop {
        for v in events(ctx)?.into_iter().skip(start) {
            let s = v.to_string();
            if needles.iter().any(|n| s.contains(n)) {
                return Ok(v);
            }
            if s.contains("Confirmation failed") || s.contains("Confirmation expired") {
                bail!("nonce confirmation failed")
            }
        }
        ensure!(Instant::now() < d, "no confirmed effect output");
        std::thread::sleep(Duration::from_millis(100))
    }
}
fn wait_keyboard(
    ctx: &ContextState,
    start: usize,
    prefix: &str,
    timeout: Duration,
) -> Result<(Value, String)> {
    let d = Instant::now() + timeout;
    loop {
        for v in events(ctx)?.into_iter().skip(start) {
            if let Some(x) = find_callback(&v, prefix) {
                return Ok((v, x));
            }
        }
        ensure!(
            Instant::now() < d,
            "callback payload {prefix:?} not observed"
        );
        std::thread::sleep(Duration::from_millis(100))
    }
}
fn find_callback(v: &Value, prefix: &str) -> Option<String> {
    match v {
        Value::Object(m) => {
            if let Some(x) = m
                .get("callback_data")
                .and_then(Value::as_str)
                .filter(|x| x.starts_with(prefix))
            {
                return Some(x.into());
            }
            m.values().find_map(|x| find_callback(x, prefix))
        }
        Value::Array(a) => a.iter().find_map(|x| find_callback(x, prefix)),
        Value::String(s) => serde_json::from_str::<Value>(s)
            .ok()
            .and_then(|x| find_callback(&x, prefix)),
        _ => None,
    }
}

fn read_matrix(path: &Path) -> Result<Vec<Scenario>> {
    let s = fs::read_to_string(path)?;
    let mut lines = s.lines();
    let header = parse_csv(
        lines
            .next()
            .context("matrix header missing")?
            .trim_start_matches('\u{feff}'),
    );
    let idx = |name: &str| {
        header
            .iter()
            .position(|x| x == name)
            .with_context(|| format!("matrix column missing {name}"))
    };
    let (iid, ifeat, irole, iout, iev) = (
        idx("id")?,
        idx("feature_ids")?,
        idx("role_preconditions")?,
        idx("outcome")?,
        idx("evidence")?,
    );
    let mut out = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let c = parse_csv(line);
        ensure!(
            c.len() == header.len(),
            "scenario csv column mismatch for {}",
            c.get(iid).unwrap_or(&String::new())
        );
        out.push(Scenario {
            id: c[iid].clone(),
            feature: c[ifeat].clone(),
            role: c[irole].clone(),
            outcome: c[iout].clone(),
            evidence: c[iev].clone(),
        })
    }
    Ok(out)
}
fn parse_csv(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut q = false;
    let mut it = line.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '"' => {
                if q && it.peek() == Some(&'"') {
                    cur.push('"');
                    it.next();
                } else {
                    q = !q
                }
            }
            ',' if !q => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}
fn priority(r: &Scenario) -> u8 {
    if r.id.starts_with("EDGE-") {
        30
    } else {
        match r.feature.split_once('-').map(|x| x.0).unwrap_or("") {
            "CMD" => 10,
            "CB" => 20,
            "HTTP" => 40,
            "INT" => 50,
            "JOB" => 60,
            "TASK" => 70,
            "LIFE" => 80,
            _ => 90,
        }
    }
}
fn csv_escape(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.into()
    }
}
fn write_results(
    ctx: &ContextState,
    rows: &[Scenario],
    results: &BTreeMap<String, ResultRow>,
) -> Result<()> {
    let mut s = "id,feature_ids,outcome,executed_this_run,strategy,evidence,reason\n".to_owned();
    for row in rows {
        let r = results.get(&row.id).context("scenario result missing")?;
        s.push_str(
            &[
                &r.id,
                &r.feature,
                &r.outcome,
                &r.executed,
                &r.strategy,
                &r.evidence,
                &r.reason,
            ]
            .into_iter()
            .map(|x| csv_escape(x))
            .collect::<Vec<_>>()
            .join(","),
        );
        s.push('\n')
    }
    fs::write(ctx.run_dir.join("scenario-results.csv"), s)?;
    Ok(())
}
