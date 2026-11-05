use crate::domain::models::AppContext;
use crate::infrastructure::database::postgres_adapter::PostgresRepository;
use chrono::Utc;
use kaspa_rpc_core::api::rpc::RpcApi;
use sqlx::Row;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use sysinfo::System;
use teloxide::prelude::*;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, ParseMode};

/// Centralized wording compatibility for existing admin diagnostics.
fn admin_report_language(
    lang: crate::presentation::telegram::menus::Language,
    mut message: String,
) -> String {
    if lang == crate::presentation::telegram::menus::Language::Ar {
        for (en, ar) in [
            ("Kaspa Pulse Health", "صحة بوت كاسبا"),
            ("Community Mining Alerts", "تنبيهات التعدين"),
            ("System Stats", "إحصاءات البوت"),
            ("Alert Delivery Summary", "ملخص التنبيهات"),
            ("Database Diagnostics", "تشخيص قاعدة البيانات"),
            ("Settings Panel", "إعدادات الإدارة"),
            ("Tracked wallets:", "المحافظ المراقبة:"),
            ("Process uptime:", "مدة التشغيل:"),
            ("Mined Records:", "سجلات التعدين:"),
            ("Live Monitoring:", "المراقبة المباشرة:"),
            ("Memory Cleaner:", "منظف الذاكرة:"),
            ("Required Queries:", "الاستعلامات المطلوبة:"),
            ("DB Queries:", "استعلامات قاعدة البيانات:"),
            ("Settings Rows:", "سجلات الإعدادات:"),
            ("Wallet Rows:", "سجلات المحافظ:"),
            ("Mined Rows:", "سجلات التعدين:"),
            ("Last alert:", "آخر تنبيه:"),
            ("Last 24 hours", "آخر 24 ساعة"),
            ("Detected:", "المكتشفة:"),
            ("Delivered:", "المرسلة:"),
            ("Failed:", "الفاشلة:"),
            ("Connection:", "الاتصال:"),
            ("Webhook:", "الويب هوك:"),
            ("Window:", "الفترة:"),
            ("Users:", "المشتركون:"),
            ("Wallets:", "المحافظ:"),
            ("Maintenance:", "الصيانة:"),
            ("Bot:", "البوت:"),
            ("Node:", "العقدة:"),
            ("DB:", "قاعدة البيانات:"),
            ("Enabled", "مفعّل"),
            ("Disabled", "معطّل"),
            ("Online", "متصل"),
            ("Offline", "غير متصل"),
            ("CONNECTED", "متصل"),
            ("FAILED", "فشل"),
        ] {
            message = message.replace(en, ar);
        }
    }
    message
}

pub async fn handle_pause(
    bot: Bot,
    msg: Message,
    app_context: Arc<AppContext>,
) -> anyhow::Result<()> {
    app_context
        .live_sync_enabled
        .store(false, Ordering::Relaxed);
    crate::send_logged!(bot, msg, "⏸️ <b>Live monitoring paused.</b>");
    Ok(())
}

pub async fn handle_resume(
    bot: Bot,
    msg: Message,
    app_context: Arc<AppContext>,
) -> anyhow::Result<()> {
    app_context.live_sync_enabled.store(true, Ordering::Relaxed);
    crate::send_logged!(bot, msg, "▶️ <b>Live monitoring active.</b>");
    Ok(())
}

pub async fn handle_restart_info(bot: Bot, msg: Message) -> anyhow::Result<()> {
    crate::send_logged!(
        bot,
        msg,
        "ℹ️ <b>Restart Information</b>\nKaspa Pulse does not restart its own process. Service restarts are intentionally controlled by the external production supervisor/deployment procedure."
    );
    Ok(())
}

fn format_uptime(total_seconds: u64) -> String {
    let days = total_seconds / 86_400;
    let hours = (total_seconds % 86_400) / 3_600;
    let minutes = (total_seconds % 3_600) / 60;
    let seconds = total_seconds % 60;

    if days > 0 {
        format!("{}d {}h {}m", days, hours, minutes)
    } else if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, seconds)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, seconds)
    } else {
        format!("{}s", seconds)
    }
}
pub async fn handle_health(
    bot: Bot,
    msg: Message,
    app_context: Arc<AppContext>,
) -> anyhow::Result<()> {
    let db = crate::infrastructure::admin_diagnostics::collect(&app_context.pool).await;
    let node_ok = app_context.rpc.get_server_info().await.is_ok();
    let webhook_enabled = std::env::var("USE_WEBHOOK")
        .unwrap_or_else(|_| "false".to_string())
        .eq_ignore_ascii_case("true");
    let uptime_seconds = current_process_uptime_seconds().unwrap_or_else(System::uptime);
    let uptime = format_uptime(uptime_seconds);

    let text = format!(
        "🩺 <b>Kaspa Pulse Health</b>\n\
         Community Mining Alerts\n\
         ━━━━━━━━━━━━━━━━━━\n\
         🤖 <b>Bot:</b> <code>Online</code>\n\
         🌐 <b>Node:</b> <code>{}</code>\n\
         🗄️ <b>DB:</b> <code>{}</code>\n\
         🔎 <b>DB Queries:</b> <code>{}</code>\n\
         🔗 <b>Webhook:</b> <code>{}</code>\n\
         👛 <b>Tracked wallets:</b> <code>{}</code>\n\
         ⛏️ <b>Last alert:</b> <code>{}</code>\n\
         ⏱️ <b>Process uptime:</b> <code>{}</code>",
        if node_ok { "Online" } else { "Offline" },
        if db.connection_ok() {
            "CONNECTED"
        } else {
            "FAILED"
        },
        db.database_status(),
        if webhook_enabled {
            "Enabled"
        } else {
            "Disabled"
        },
        crate::infrastructure::admin_diagnostics::display_count(&db.wallets_count),
        crate::infrastructure::admin_diagnostics::display_last_alert(&db.last_alert),
        uptime
    );
    let lang =
        crate::presentation::telegram::menus::user_language(&app_context.pool, msg.chat.id.0).await;
    let text = admin_report_language(lang, text);
    crate::send_logged!(bot, msg, text);
    Ok(())
}

pub async fn handle_stats(
    bot: Bot,
    msg: Message,
    app_context: Arc<AppContext>,
) -> anyhow::Result<()> {
    let db = crate::infrastructure::admin_diagnostics::collect(&app_context.pool).await;
    let text = format!(
        "📊 <b>System Stats</b>\n\
         ━━━━━━━━━━━━━━━━━━\n\
         🗄️ DB Queries: <code>{}</code>\n\
         👥 Users: <code>{}</code>\n\
         👛 Wallets: <code>{}</code>\n\
         ⛏️ Mined Records: <code>{}</code>\n\
         🔄 Live Monitoring: <code>{}</code>\n\
         🧹 Memory Cleaner: <code>{}</code>\n\
         🚧 Maintenance: <code>{}</code>",
        db.database_status(),
        crate::infrastructure::admin_diagnostics::display_count(&db.users_count),
        crate::infrastructure::admin_diagnostics::display_count(&db.wallets_count),
        crate::infrastructure::admin_diagnostics::display_count(&db.mined_count),
        app_context.live_sync_enabled.load(Ordering::Relaxed),
        app_context.memory_cleaner_enabled.load(Ordering::Relaxed),
        app_context.maintenance_mode.load(Ordering::Relaxed),
    );
    let lang =
        crate::presentation::telegram::menus::user_language(&app_context.pool, msg.chat.id.0).await;
    let text = admin_report_language(lang, text);
    crate::send_logged!(bot, msg, text);
    Ok(())
}

pub async fn handle_toggle(
    bot: Bot,
    msg: Message,
    flag: String,
    app_context: Arc<AppContext>,
) -> anyhow::Result<()> {
    let key = flag.trim().to_uppercase();
    let db = PostgresRepository::new(app_context.pool.clone());

    let new_state = match key.as_str() {
        "ENABLE_MEMORY_CLEANER" | "MEMORY" | "MEM" => {
            let current = app_context.memory_cleaner_enabled.load(Ordering::Relaxed);
            let next = !current;
            app_context
                .memory_cleaner_enabled
                .store(next, Ordering::Relaxed);
            db.update_setting("ENABLE_MEMORY_CLEANER", if next { "true" } else { "false" })
                .await?;
            Some(("ENABLE_MEMORY_CLEANER", next))
        }
        "ENABLE_LIVE_SYNC" | "LIVE" | "SYNC" => {
            let current = app_context.live_sync_enabled.load(Ordering::Relaxed);
            let next = !current;
            app_context.live_sync_enabled.store(next, Ordering::Relaxed);
            db.update_setting("ENABLE_LIVE_SYNC", if next { "true" } else { "false" })
                .await?;
            Some(("ENABLE_LIVE_SYNC", next))
        }
        "MAINTENANCE_MODE" | "MAINTENANCE" => {
            let current = app_context.maintenance_mode.load(Ordering::Relaxed);
            let next = !current;
            app_context.maintenance_mode.store(next, Ordering::Relaxed);
            db.update_setting("MAINTENANCE_MODE", if next { "true" } else { "false" })
                .await?;
            Some(("MAINTENANCE_MODE", next))
        }
        _ => None,
    };

    match new_state {
        Some((name, state)) => {
            crate::send_logged!(
                bot,
                msg,
                format!(
                    "✅ <b>Setting Updated</b>\n<code>{}</code> = <code>{}</code>",
                    name, state
                )
            );
        }
        None => {
            crate::send_logged!(
                bot,
                msg,
                "⚠️ <b>Unknown setting.</b>\nAvailable: MEMORY, SYNC, MAINTENANCE"
            );
        }
    }

    Ok(())
}

pub async fn handle_sys(bot: Bot, msg: Message, monitoring_status: bool) -> anyhow::Result<()> {
    let mut sys = System::new_all();
    sys.refresh_all();

    let total_memory_mb = sys.total_memory() / 1024 / 1024;
    let used_memory_mb = sys.used_memory() / 1024 / 1024;
    let total_swap_mb = sys.total_swap() / 1024 / 1024;
    let used_swap_mb = sys.used_swap() / 1024 / 1024;

    let text = format!(
        "🖥️ <b>System Diagnostics</b>\n\
         ━━━━━━━━━━━━━━━━━━\n\
         🔄 Monitoring: <code>{}</code>\n\
         🧠 RAM: <code>{} / {} MB</code>\n\
         💾 Swap: <code>{} / {} MB</code>\n\
         🕒 Time: <code>{}</code>",
        monitoring_status,
        used_memory_mb,
        total_memory_mb,
        used_swap_mb,
        total_swap_mb,
        Utc::now().to_rfc3339(),
    );

    crate::send_logged!(bot, msg, text);
    Ok(())
}

pub async fn handle_logs(bot: Bot, msg: Message) -> anyhow::Result<()> {
    let lines = crate::infrastructure::recent_logs::recent_lines(25);
    let text = crate::infrastructure::recent_logs::response_html(&lines);
    // Do not feed the log response back into the tracing buffer it displays.
    bot.send_message(msg.chat.id, text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
    Ok(())
}

pub async fn handle_db_diag(
    bot: Bot,
    msg: Message,
    app_context: Arc<AppContext>,
) -> anyhow::Result<()> {
    let db = crate::infrastructure::admin_diagnostics::collect(&app_context.pool).await;
    let text = format!(
        "🧪 <b>Database Diagnostics</b>\n\
         ━━━━━━━━━━━━━━━━━━\n\
         Connection: <code>{}</code>\n\
         Required Queries: <code>{}</code>\n\
         Settings Rows: <code>{}</code>\n\
         Wallet Rows: <code>{}</code>\n\
         Mined Rows: <code>{}</code>",
        if db.connection_ok() { "OK" } else { "FAILED" },
        db.database_status(),
        crate::infrastructure::admin_diagnostics::display_count(&db.settings_count),
        crate::infrastructure::admin_diagnostics::display_count(&db.wallets_count),
        crate::infrastructure::admin_diagnostics::display_count(&db.mined_count),
    );
    let lang =
        crate::presentation::telegram::menus::user_language(&app_context.pool, msg.chat.id.0).await;
    let text = admin_report_language(lang, text);
    crate::send_logged!(bot, msg, text);
    Ok(())
}

pub async fn handle_interactive_settings(
    bot: Bot,
    chat_id: teloxide::types::ChatId,
    msg_id: Option<teloxide::types::MessageId>,
    app_context: Arc<AppContext>,
) -> anyhow::Result<()> {
    let lang =
        crate::presentation::telegram::menus::user_language(&app_context.pool, chat_id.0).await;
    let mem = app_context.memory_cleaner_enabled.load(Ordering::Relaxed);
    let sync = app_context.live_sync_enabled.load(Ordering::Relaxed);
    let maint = app_context.maintenance_mode.load(Ordering::Relaxed);

    let text = format!(
        "⚙️ <b>Settings Panel</b>\n\
         ━━━━━━━━━━━━━━━━━━\n\
         🧹 Memory Cleaner: <code>{}</code>\n\
         🔄 Live Monitoring: <code>{}</code>\n\
         🚧 Maintenance: <code>{}</code>",
        mem, sync, maint
    );

    let text = admin_report_language(lang, text);
    let markup = InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback(
                lang.label("Toggle Memory", "تبديل تنظيف الذاكرة"),
                "btn_toggle_ENABLE_MEMORY_CLEANER",
            ),
            InlineKeyboardButton::callback(
                lang.label("Toggle Monitoring", "تبديل المراقبة"),
                "btn_toggle_ENABLE_LIVE_SYNC",
            ),
        ],
        vec![InlineKeyboardButton::callback(
            "Toggle Maintenance",
            "btn_toggle_MAINTENANCE_MODE",
        )],
        vec![
            InlineKeyboardButton::callback(lang.label("↩️ Admin", "↩️ الإدارة"), "ui:admin"),
            InlineKeyboardButton::callback(lang.label("🏠 Home", "🏠 الرئيسية"), "ui:home"),
        ],
    ]);

    if let Some(id) = msg_id {
        let _ = bot
            .edit_message_text(chat_id, id, text)
            .parse_mode(ParseMode::Html)
            .reply_markup(markup)
            .await?;
    } else {
        let _ = bot
            .send_message(chat_id, text)
            .parse_mode(ParseMode::Html)
            .reply_markup(markup)
            .await?;
    }

    Ok(())
}

fn current_process_uptime_seconds() -> Option<u64> {
    let mut sys = System::new_all();
    sys.refresh_all();

    let pid = sysinfo::get_current_pid().ok()?;
    let process = sys.process(pid)?;
    let process_start = process.start_time();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();

    now.checked_sub(process_start)
}

pub async fn handle_events(
    bot: Bot,
    msg: Message,
    app_context: Arc<AppContext>,
) -> anyhow::Result<()> {
    fn html_escape(value: &str) -> String {
        let mut escaped = String::with_capacity(value.len());

        for ch in value.chars() {
            match ch {
                '&' => escaped.push_str("&amp;"),
                '<' => escaped.push_str("&lt;"),
                '>' => escaped.push_str("&gt;"),
                '"' => escaped.push_str("&quot;"),
                '\'' => escaped.push_str("&#39;"),
                _ => escaped.push(ch),
            }
        }

        escaped
    }

    fn compact_text(value: &str, max_chars: usize) -> String {
        let cleaned = value
            .replace(['\r', '\n', '\t'], " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");

        if cleaned.chars().count() <= max_chars {
            return cleaned;
        }

        let mut short = cleaned.chars().take(max_chars).collect::<String>();
        short.push('…');
        short
    }

    fn event_icon(event_type: &str) -> &'static str {
        match event_type {
            "SYSTEM_START" => "🚀",
            "SYSTEM_SHUTDOWN" => "🛑",
            "WEBHOOK_START" => "🌐",
            "PANIC_EVENT" => "💥",
            "ALERT_DETECTED" => "🔎",
            "ALERT_DELIVERED" => "✅",
            "ALERT_DELIVERY_FAILED" => "❌",
            "ALERT_DUPLICATE_SKIPPED" => "♻️",
            "DB_ERROR" => "🗄️",
            "RPC_ERROR" => "🌐",
            "RPC_RECOVERED" => "✅",
            "TELEGRAM_ERROR" => "📨",
            "EVENT_LOG_PURGED" => "🧹",
            "ADMIN_ACTION" => "🛡️",
            "RATE_LIMITED" => "⏳",
            _ => "•",
        }
    }

    fn severity_icon(severity: &str) -> &'static str {
        if severity.eq_ignore_ascii_case("error") {
            "🔴"
        } else if severity.eq_ignore_ascii_case("warn") {
            "🟡"
        } else {
            "🟢"
        }
    }

    let rows = sqlx::query(
        r#"
        SELECT
            created_at,
            event_type,
            severity,
            chat_id,
            wallet_masked,
            status,
            error_message
        FROM bot_event_log
        ORDER BY created_at DESC
        LIMIT 10
        "#,
    )
    .fetch_all(&app_context.pool)
    .await;

    let rows = match rows {
        Ok(rows) => rows,
        Err(e) => {
            crate::send_logged!(
                bot,
                msg,
                format!(
                    "❌ <b>Events unavailable.</b>\n<code>{}</code>",
                    html_escape(&e.to_string())
                )
            );
            return Ok(());
        }
    };

    if rows.is_empty() {
        crate::send_logged!(
            bot,
            msg,
            "📜 <b>Recent Bot Events</b>\n━━━━━━━━━━━━━━━━━━\nNo events found."
        );
        return Ok(());
    }

    let mut text = String::from(
        "📜 <b>Recent Bot Events</b>\n━━━━━━━━━━━━━━━━━━\n<code>Latest 10 compact events</code>\n",
    );

    for (index, row) in rows.iter().enumerate() {
        let created_at: chrono::DateTime<chrono::Utc> = row.try_get("created_at")?;
        let event_type: String = row.try_get("event_type")?;
        let severity: String = row.try_get("severity")?;
        let chat_id: Option<i64> = row.try_get("chat_id")?;
        let wallet_masked: Option<String> = row.try_get("wallet_masked")?;
        let status: Option<String> = row.try_get("status")?;
        let error_message: Option<String> = row.try_get("error_message")?;

        let status_text = status
            .as_deref()
            .map(|value| compact_text(value, 28))
            .unwrap_or_else(|| "-".to_string());

        text.push_str(&format!(
            "\n<code>{:02}</code> {} {} <b>{}</b>\n",
            index + 1,
            severity_icon(&severity),
            event_icon(&event_type),
            html_escape(&event_type)
        ));

        text.push_str(&format!(
            "⏱ <code>{}</code> | <code>{}</code>\n",
            created_at.format("%Y-%m-%d %H:%M:%S UTC"),
            html_escape(&status_text)
        ));

        let mut details = Vec::new();

        if let Some(chat_id) = chat_id {
            details.push(format!("Chat: <code>{}</code>", chat_id));
        }

        if let Some(wallet) = wallet_masked {
            details.push(format!(
                "Wallet: <code>{}</code>",
                html_escape(&compact_text(&wallet, 32))
            ));
        }

        if !details.is_empty() {
            text.push_str(&details.join(" | "));
            text.push('\n');
        }

        if let Some(error) = error_message {
            let error = compact_text(&error, 90);
            if !error.is_empty() {
                text.push_str(&format!("⚠️ <code>{}</code>\n", html_escape(&error)));
            }
        }
    }

    if text.chars().count() > 3900 {
        text = text.chars().take(3900).collect::<String>();
        text.push_str("\n… truncated");
    }

    crate::send_logged!(bot, msg, text);
    Ok(())
}

pub async fn handle_errors(
    bot: Bot,
    msg: Message,
    app_context: Arc<AppContext>,
) -> anyhow::Result<()> {
    let rows = sqlx::query(
        r#"
        SELECT created_at, event_type, severity, chat_id, wallet_masked, status, error_message
        FROM bot_event_log
        WHERE severity = 'error'
        ORDER BY created_at DESC
        LIMIT 10
        "#,
    )
    .fetch_all(&app_context.pool)
    .await?;

    let mut text = String::from("🚨 <b>Recent Error Events</b>\n━━━━━━━━━━━━━━━━━━\n");

    if rows.is_empty() {
        text.push_str("No recent errors.");
    }

    for row in rows {
        let created_at: chrono::DateTime<chrono::Utc> = row.try_get("created_at")?;
        let event_type: String = row.try_get("event_type")?;
        let chat_id: Option<i64> = row.try_get("chat_id")?;
        let wallet: Option<String> = row.try_get("wallet_masked")?;
        let status: Option<String> = row.try_get("status")?;
        let err: Option<String> = row.try_get("error_message")?;

        text.push_str(&format!(
            "\n🔴 <b>{}</b>\n⏱️ <code>{}</code>\n",
            event_type,
            created_at.format("%Y-%m-%d %H:%M:%S UTC")
        ));

        if let Some(status) = status {
            text.push_str(&format!("Status: <code>{}</code>\n", status));
        }
        if let Some(chat_id) = chat_id {
            text.push_str(&format!("Chat: <code>{}</code>\n", chat_id));
        }
        if let Some(wallet) = wallet
            && !wallet.is_empty()
        {
            text.push_str(&format!("Wallet: <code>{}</code>\n", wallet));
        }
        if let Some(err) = err {
            let short = if err.chars().count() > 120 {
                format!("{}...", err.chars().take(120).collect::<String>())
            } else {
                err
            };
            text.push_str(&format!("Error: <code>{}</code>\n", short));
        }
    }

    if text.chars().count() > 3400 {
        text = text.chars().take(3400).collect::<String>();
        text.push_str("\n… truncated");
    }

    crate::send_logged!(bot, msg, text);
    Ok(())
}

pub async fn handle_delivery(
    bot: Bot,
    msg: Message,
    app_context: Arc<AppContext>,
) -> anyhow::Result<()> {
    let detected: i64 = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM bot_event_log WHERE event_type = 'ALERT_DETECTED' AND created_at >= NOW() - INTERVAL '24 hours'",
    )
    .fetch_one(&app_context.pool)
    .await?;

    let delivered: i64 = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM bot_event_log WHERE event_type = 'ALERT_DELIVERED' AND created_at >= NOW() - INTERVAL '24 hours'",
    )
    .fetch_one(&app_context.pool)
    .await?;

    let failed: i64 = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM bot_event_log WHERE event_type = 'ALERT_DELIVERY_FAILED' AND created_at >= NOW() - INTERVAL '24 hours'",
    )
    .fetch_one(&app_context.pool)
    .await?;

    let unique_wallets: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT wallet_masked) FROM bot_event_log WHERE event_type LIKE 'ALERT_%' AND created_at >= NOW() - INTERVAL '24 hours'",
    )
    .fetch_one(&app_context.pool)
    .await?;

    let text = format!(
        "📬 <b>Alert Delivery Summary</b>\n━━━━━━━━━━━━━━━━━━\n⏱️ Window: <code>Last 24 hours</code>\n🔎 Detected: <code>{}</code>\n✅ Delivered: <code>{}</code>\n❌ Failed: <code>{}</code>\n👛 Wallets: <code>{}</code>",
        detected, delivered, failed, unique_wallets
    );

    let lang =
        crate::presentation::telegram::menus::user_language(&app_context.pool, msg.chat.id.0).await;
    let text = admin_report_language(lang, text);
    crate::send_logged!(bot, msg, text);
    Ok(())
}

pub async fn handle_subscribers(
    bot: Bot,
    msg: Message,
    wallet: String,
    app_context: Arc<AppContext>,
) -> anyhow::Result<()> {
    let clean_wallet = wallet.trim();

    if clean_wallet.is_empty() {
        crate::send_logged!(
            bot,
            msg,
            "⚠️ <b>Usage:</b> /subscribers <code>kaspa:wallet</code>"
        );
        return Ok(());
    }

    let rows: Vec<(i64,)> =
        sqlx::query_as("SELECT chat_id FROM user_wallets WHERE wallet = $1 ORDER BY chat_id")
            .bind(clean_wallet)
            .fetch_all(&app_context.pool)
            .await?;

    let mut text = format!(
        "👥 <b>Wallet Subscribers</b>\n━━━━━━━━━━━━━━━━━━\nWallet: <code>{}</code>\nSubscribers: <code>{}</code>\n",
        crate::utils::format_short_wallet(clean_wallet),
        rows.len()
    );

    for (idx, row) in rows.iter().enumerate() {
        text.push_str(&format!("\n{}. <code>{}</code>", idx + 1, row.0));
    }

    crate::send_logged!(bot, msg, text);
    Ok(())
}

pub async fn handle_wallet_events(
    bot: Bot,
    msg: Message,
    wallet: String,
    app_context: Arc<AppContext>,
) -> anyhow::Result<()> {
    let clean_wallet = wallet.trim();

    if clean_wallet.is_empty() {
        crate::send_logged!(
            bot,
            msg,
            "⚠️ <b>Usage:</b> /wallet_events <code>kaspa:wallet</code>"
        );
        return Ok(());
    }

    let wallet_masked = crate::utils::format_short_wallet(clean_wallet);

    let rows = sqlx::query(
        r#"
        SELECT created_at, event_type, severity, chat_id, status
        FROM bot_event_log
        WHERE wallet_masked = $1
        ORDER BY created_at DESC
        LIMIT 10
        "#,
    )
    .bind(&wallet_masked)
    .fetch_all(&app_context.pool)
    .await?;

    let mut text = format!(
        "👛 <b>Wallet Events</b>\n━━━━━━━━━━━━━━━━━━\nShowing latest <code>10</code> events\nWallet: <code>{}</code>\n",
        wallet_masked
    );

    if rows.is_empty() {
        text.push_str("\nNo events found.");
    }

    for row in rows {
        let created_at: chrono::DateTime<chrono::Utc> = row.try_get("created_at")?;
        let event_type: String = row.try_get("event_type")?;
        let severity: String = row.try_get("severity")?;
        let chat_id: Option<i64> = row.try_get("chat_id")?;
        let status: Option<String> = row.try_get("status")?;

        text.push_str(&format!(
            "\n{} <b>{}</b>\n⏱️ <code>{}</code>\n",
            if severity == "error" { "🔴" } else { "🟢" },
            event_type,
            created_at.format("%Y-%m-%d %H:%M:%S UTC")
        ));

        if let Some(status) = status {
            text.push_str(&format!("Status: <code>{}</code>\n", status));
        }
        if let Some(chat_id) = chat_id {
            text.push_str(&format!("Chat: <code>{}</code>\n", chat_id));
        }
    }

    if text.chars().count() > 3400 {
        text = text.chars().take(3400).collect::<String>();
        text.push_str("\n… truncated");
    }

    crate::send_logged!(bot, msg, text);
    Ok(())
}

#[allow(dead_code)]
pub async fn handle_cleanup_events(
    bot: Bot,
    msg: Message,
    app_context: Arc<AppContext>,
) -> anyhow::Result<()> {
    let retention_days: i64 = std::env::var("BOT_EVENT_LOG_RETENTION_DAYS")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(60)
        .clamp(1, 365);

    let result = sqlx::query(
        "DELETE FROM bot_event_log
         WHERE created_at < NOW() - ($1::text || ' days')::interval",
    )
    .bind(retention_days.to_string())
    .execute(&app_context.pool)
    .await?;

    let text = format!(
        "🧹 <b>Events Cleanup Complete</b>\n━━━━━━━━━━━━━━━━━━\nRetention: <code>{} days</code>\nDeleted rows: <code>{}</code>",
        retention_days,
        result.rows_affected()
    );

    crate::send_logged!(bot, msg, text);
    Ok(())
}

#[allow(dead_code)]
pub async fn handle_mute_alerts(
    bot: Bot,
    msg: Message,
    app_context: Arc<AppContext>,
) -> anyhow::Result<()> {
    crate::wallet::alert_delivery_gate::set_alert_delivery_enabled(&app_context.pool, false)
        .await?;

    let text = "🔕 <b>Alert Delivery Muted</b>\n━━━━━━━━━━━━━━━━━━\nTelegram mining alerts are now <code>DISABLED</code>.\n\nThe bot will continue detecting blocks, analyzing DAG data, updating dedup state, and recording events in the database.";

    crate::send_logged!(bot, msg, text);
    Ok(())
}

#[allow(dead_code)]
pub async fn handle_unmute_alerts(
    bot: Bot,
    msg: Message,
    app_context: Arc<AppContext>,
) -> anyhow::Result<()> {
    crate::wallet::alert_delivery_gate::set_alert_delivery_enabled(&app_context.pool, true).await?;

    let text = "🔔 <b>Alert Delivery Resumed</b>\n━━━━━━━━━━━━━━━━━━\nTelegram mining alerts are now <code>ENABLED</code>.\n\nOnly new alerts after this point will be sent.";

    crate::send_logged!(bot, msg, text);
    Ok(())
}

pub async fn handle_alerts_status(
    bot: Bot,
    msg: Message,
    app_context: Arc<AppContext>,
) -> anyhow::Result<()> {
    let text =
        crate::wallet::alert_delivery_gate::alert_delivery_status_text(&app_context.pool).await;

    crate::send_logged!(bot, msg, text);
    Ok(())
}

/* Admin-only subscriber navigation. Caller must verify private admin identity
 * before entering either renderer. Queries are read-only and bounded. */
pub async fn render_subscribers_panel(
    bot: &Bot,
    chat: teloxide::types::ChatId,
    message: teloxide::types::MessageId,
    page: i64,
    lang: crate::presentation::telegram::menus::Language,
    pool: &sqlx::PgPool,
) -> anyhow::Result<()> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(DISTINCT chat_id) FROM public.user_wallets")
        .fetch_one(pool)
        .await?;
    const PAGE_SIZE: i64 = 8;
    let max_page = if count == 0 {
        0
    } else {
        (count - 1) / PAGE_SIZE
    };
    let page = page.min(max_page).max(0);
    // Only people actually tracking a wallet with THIS bot are listed.
    let rows=sqlx::query(
        "SELECT uw.chat_id, COUNT(*)::bigint AS wallet_count,          MAX(uw.last_active) AS last_active,          MAX(b.telegram_username) AS username, MAX(b.telegram_first_name) AS first_name, MAX(b.telegram_last_name) AS last_name FROM public.user_wallets uw          LEFT JOIN public.bot_users b ON b.telegram_user_id=uw.chat_id          GROUP BY uw.chat_id          ORDER BY MAX(uw.last_active) DESC NULLS LAST, uw.chat_id          LIMIT $1 OFFSET $2"
    )
    .bind(PAGE_SIZE).bind(page*PAGE_SIZE)
    .fetch_all(pool).await?;
    let mut text = format!(
        "{}\n{}: <code>{}</code>\n{}: <code>{}/{}</code>",
        lang.label("👥 <b>Subscribers</b>", "👥 <b>المشتركون</b>"),
        lang.label("Tracked users", "المشتركون المسجلون"),
        count,
        lang.label("Page", "الصفحة"),
        page + 1,
        max_page + 1
    );
    let mut buttons = Vec::new();
    for row in rows {
        let chat_id: i64 = row.try_get("chat_id")?;
        let n: i64 = row.try_get("wallet_count")?;
        let mut nickname: Option<String> = row.try_get("username")?;
        let mut first_name: Option<String> = row.try_get("first_name")?;
        let mut last_name: Option<String> = row.try_get("last_name")?;
        if nickname.as_deref().is_none_or(|s| s.trim().is_empty()) {
            // Telegram is authoritative for identities, but getChat may fail for blocked users.
            if let Ok(Ok(info)) = tokio::time::timeout(
                std::time::Duration::from_secs(3),
                bot.get_chat(teloxide::types::ChatId(chat_id)),
            )
            .await
            {
                nickname = info.username().map(str::to_owned);
                first_name = info.first_name().map(str::to_owned).or(first_name);
                last_name = info.last_name().map(str::to_owned).or(last_name);
                let _=sqlx::query("UPDATE public.bot_users SET telegram_username=$1,telegram_first_name=$2,telegram_last_name=$3 WHERE telegram_user_id=$4")
                    .bind(nickname.as_deref()).bind(first_name.as_deref()).bind(last_name.as_deref())
                    .bind(chat_id).execute(pool).await;
            }
        }
        let name = subscriber_name(
            nickname.as_deref(),
            first_name.as_deref(),
            last_name.as_deref(),
            chat_id,
        );
        let identity = crate::utils::html_escape(&name);
        text.push_str(&format!(
            "\n• {} · {} {}",
            identity,
            n,
            lang.label("wallets", "محافظ")
        ));
        buttons.push(vec![InlineKeyboardButton::callback(
            format!("👤 {}", name.chars().take(45).collect::<String>()),
            format!("ui:admin_user:{chat_id}"),
        )]);
    }
    if count == 0 {
        text.push_str(lang.label("\nNo tracked subscribers.", "\nلا يوجد مشتركون مسجلون."));
    }
    let mut pagination = Vec::new();
    if page > 0 {
        pagination.push(InlineKeyboardButton::callback(
            lang.label("◀ Previous", "◀ السابق"),
            format!("ui:admin_subs:{}", page - 1),
        ));
    }
    if page < max_page {
        pagination.push(InlineKeyboardButton::callback(
            lang.label("Next ▶", "التالي ▶"),
            format!("ui:admin_subs:{}", page + 1),
        ));
    }
    if !pagination.is_empty() {
        buttons.push(pagination);
    }
    buttons.push(vec![
        InlineKeyboardButton::callback(lang.label("↩️ Admin", "↩️ الإدارة"), "ui:admin"),
        InlineKeyboardButton::callback(lang.label("🏠 Home", "🏠 الرئيسية"), "ui:home"),
    ]);
    bot.edit_message_text(chat, message, text)
        .parse_mode(ParseMode::Html)
        .reply_markup(InlineKeyboardMarkup::new(buttons))
        .await?;
    Ok(())
}

/// Do not misrepresent a Telegram ID as a username.
fn subscriber_name(
    username: Option<&str>,
    first: Option<&str>,
    last: Option<&str>,
    id: i64,
) -> String {
    if let Some(u) = username.filter(|v| !v.trim().is_empty()) {
        return format!("@{}", u.trim().trim_start_matches('@'));
    }
    let full = [first.unwrap_or("").trim(), last.unwrap_or("").trim()]
        .into_iter()
        .filter(|v| !v.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if full.is_empty() {
        format!("ID {id}")
    } else {
        full
    }
}

fn kaspa_stream_url(wallet: &str) -> Option<reqwest::Url> {
    let suffix = wallet.strip_prefix("kaspa:")?;
    if !(50..=100).contains(&wallet.len())
        || !suffix
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    {
        return None;
    }
    reqwest::Url::parse(&format!("https://kaspa.stream/addresses/kaspa%3A{suffix}")).ok()
}

fn exact_kas(sompi: u64) -> String {
    format!("{}.{:08}", sompi / 100_000_000, sompi % 100_000_000)
}

pub async fn render_subscriber_detail(
    bot: &Bot,
    chat: teloxide::types::ChatId,
    message: teloxide::types::MessageId,
    user_id: i64,
    lang: crate::presentation::telegram::menus::Language,
    context: &AppContext,
) -> anyhow::Result<()> {
    let pool = &context.pool;
    let rows=sqlx::query(
        "SELECT u.wallet, u.last_active, u.is_active, u.alerts_enabled,          b.telegram_username, b.telegram_first_name, b.telegram_last_name, b.status, COALESCE(ul.language, 'en') AS language, b.first_seen_at, b.last_seen_at, b.monitoring_enabled          FROM public.user_wallets u          LEFT JOIN public.bot_users b ON b.telegram_user_id=u.chat_id          LEFT JOIN public.telegram_user_language ul ON ul.user_id=u.chat_id          WHERE u.chat_id=$1 ORDER BY u.last_active DESC NULLS LAST LIMIT 12"
    )
    .bind(user_id).fetch_all(pool).await?;
    let mut wallet_buttons: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    let mut text = format!(
        "{}\n{}: <code>{user_id}</code>",
        lang.label("👤 <b>Subscriber</b>", "👤 <b>مشترك</b>"),
        lang.label("Telegram ID", "معرف تلجرام")
    );
    if let Some(first) = rows.first() {
        let mut username: Option<String> = first.try_get("telegram_username")?;
        let mut display_name: Option<String> = first.try_get("telegram_first_name")?;
        let mut last_name: Option<String> = first.try_get("telegram_last_name")?;
        if username.as_deref().is_none_or(|s| s.trim().is_empty()) {
            if let Ok(Ok(info)) = tokio::time::timeout(
                std::time::Duration::from_secs(3),
                bot.get_chat(teloxide::types::ChatId(user_id)),
            )
            .await
            {
                username = info.username().map(str::to_owned);
                display_name = info.first_name().map(str::to_owned).or(display_name);
                last_name = info.last_name().map(str::to_owned).or(last_name);
                let _=sqlx::query("UPDATE public.bot_users SET telegram_username=$1,telegram_first_name=$2,telegram_last_name=$3 WHERE telegram_user_id=$4")
                    .bind(username.as_deref()).bind(display_name.as_deref()).bind(last_name.as_deref())
                    .bind(user_id).execute(pool).await;
            }
        }
        let resolved_name = subscriber_name(
            username.as_deref(),
            display_name.as_deref(),
            last_name.as_deref(),
            user_id,
        );
        text = format!(
            "👤 <b>{}</b>\n{}: {}\n{}: <code>{user_id}</code>",
            lang.label("Subscriber", "المشترك"),
            lang.label("Name / username", "الاسم / المستخدم"),
            crate::utils::html_escape(&resolved_name),
            lang.label("Telegram ID", "معرف تلجرام")
        );
        if username.as_deref().is_none_or(|v| v.trim().is_empty()) {
            text.push_str(lang.label(
                "\nUsername: not available from Telegram",
                "\nاسم المستخدم: غير متاح من Telegram",
            ));
        }
        let status: Option<String> = first.try_get("status")?;
        let locale: Option<String> = first.try_get("language")?;
        let registered: Option<chrono::DateTime<Utc>> = first.try_get("first_seen_at")?;
        let last_seen: Option<chrono::DateTime<Utc>> = first.try_get("last_seen_at")?;
        let monitoring: Option<bool> = first.try_get("monitoring_enabled")?;
        if let Some(value) = username {
            text.push_str(&format!(
                "\n{}: @{}",
                lang.label("Username", "اسم المستخدم"),
                crate::utils::html_escape(&value)
            ));
        }
        if let Some(value) = display_name {
            text.push_str(&format!(
                "\n{}: {}",
                lang.label("Name", "الاسم"),
                crate::utils::html_escape(&value)
            ));
        }
        if let Some(value) = status {
            text.push_str(&format!(
                "\n{}: <code>{}</code>",
                lang.label("Status", "الحالة"),
                crate::utils::html_escape(&value)
            ));
        }
        if let Some(value) = locale {
            text.push_str(&format!(
                "\n{}: <code>{}</code>",
                lang.label("Language", "اللغة"),
                crate::utils::html_escape(&value)
            ));
        }
        if let Some(registered) = registered {
            text.push_str(&format!(
                "\n{}: <code>{}</code>",
                lang.label("Registered", "التسجيل"),
                registered.format("%Y-%m-%d")
            ));
        }
        if let Some(last_seen) = last_seen {
            text.push_str(&format!(
                "\n{}: <code>{}</code>",
                lang.label("Last seen", "آخر نشاط"),
                last_seen.format("%Y-%m-%d")
            ));
        }
        if let Some(monitoring) = monitoring {
            text.push_str(&format!(
                "\n{}: {}",
                lang.label("Monitoring", "المراقبة"),
                if monitoring { "🟢" } else { "⚪" }
            ));
        }
        let node = crate::infrastructure::node::kaspa_adapter::KaspaRpcAdapter {
            client: context.rpc.clone(),
        };
        let price = context.current_kas_usd_price().await;
        let mut total_sompi = 0u64;
        let mut all_balances = true;
        text.push_str(&format!(
            "\n\n<b>{} ({})</b>",
            lang.label("Tracked wallets", "المحافظ المسجلة"),
            rows.len()
        ));
        for (index, row) in rows.iter().enumerate() {
            let wallet: String = row.try_get("wallet")?;
            let enabled: Option<bool> = row.try_get("alerts_enabled").ok();
            let active: Option<bool> = row.try_get("is_active").ok();
            let seen: Option<chrono::DateTime<Utc>> = row.try_get("last_active").ok();
            text.push_str(&format!(
                "\n\n{}. <code>{}</code> {} {}",
                index + 1,
                crate::utils::html_escape(&wallet),
                if enabled == Some(false) {
                    "🔕"
                } else {
                    "🔔"
                },
                if active == Some(false) { "⚪" } else { "🟢" }
            ));
            if let Some(seen) = seen {
                text.push_str(&format!(" · <code>{}</code>", seen.format("%Y-%m-%d")));
            }
            match tokio::time::timeout(std::time::Duration::from_secs(6), node.get_balance(&wallet))
                .await
            {
                Ok(Ok((sompi, utxos))) => {
                    total_sompi = total_sompi.saturating_add(sompi);
                    text.push_str(&format!(
                        "\n💰 <b>{} KAS</b> · {} UTXO",
                        exact_kas(sompi),
                        utxos
                    ));
                    if let Some(price) = price {
                        text.push_str(&format!(" · ≈ ${:.2}", sompi as f64 / 1e8 * price));
                    }
                }
                _ => {
                    all_balances = false;
                    text.push_str(lang.label(
                        "\n⚠️ Live balance unavailable (not zero)",
                        "\n⚠️ الرصيد المباشر غير متاح (ليس صفرًا)",
                    ));
                    tracing::warn!(wallet=%crate::utils::format_short_wallet(&wallet),
                        "[ADMIN] Subscriber balance RPC unavailable; no zero substituted.");
                }
            }
            if let Some(url) = kaspa_stream_url(&wallet) {
                wallet_buttons.push(vec![InlineKeyboardButton::url(
                    format!("🌐 Kaspa.stream · {}", index + 1),
                    url,
                )]);
            }
        }
        if all_balances && !rows.is_empty() {
            text.push_str(&format!(
                "\n\n💰 {}: <b>{} KAS</b>",
                lang.label("Total balance", "إجمالي الأرصدة"),
                exact_kas(total_sompi)
            ));
            if let Some(price) = price {
                text.push_str(&format!(" (≈ ${:.2})", total_sompi as f64 / 1e8 * price));
            }
        } else {
            text.push_str(lang.label(
                "\nTotal omitted: live balance unavailable.",
                "\nلم يُحسب الإجمالي لتعذر قراءة رصيد مباشر.",
            ));
        }
        text.push_str(lang.label(
            "\nBalances read live from Kaspa RPC.",
            "\nالأرصدة مباشرة من عقدة كاسبا وقت فتح الصفحة.",
        ));
        if rows.len() == 12 {
            text.push_str(lang.label("\nAdditional wallets may exist.", "\nقد توجد محافظ إضافية."));
        }
    } else {
        text.push_str(lang.label("\nNo tracked wallets found.", "\nلم تُعثر على محافظ مسجلة."));
    }
    wallet_buttons.push(vec![
        InlineKeyboardButton::callback(
            lang.label("🔄 Refresh", "🔄 تحديث"),
            format!("ui:admin_user:{user_id}"),
        ),
        InlineKeyboardButton::callback(
            lang.label("↩️ Subscribers", "↩️ المشتركون"),
            "ui:admin_subs:0",
        ),
    ]);
    wallet_buttons.push(vec![InlineKeyboardButton::callback(
        lang.label("🏠 Home", "🏠 الرئيسية"),
        "ui:home",
    )]);
    bot.edit_message_text(chat, message, text)
        .parse_mode(ParseMode::Html)
        .reply_markup(InlineKeyboardMarkup::new(wallet_buttons))
        .await?;
    Ok(())
}

#[cfg(test)]
mod op040_subscriber_tests {
    use super::{exact_kas, kaspa_stream_url, subscriber_name};
    #[test]
    fn display_prefers_real_username_then_name_else_explicit_id() {
        assert_eq!(
            subscriber_name(Some("name"), Some("Ali"), None, 123),
            "@name"
        );
        assert_eq!(
            subscriber_name(None, Some("Ali"), Some("Nasser"), 123),
            "Ali Nasser"
        );
        assert_eq!(subscriber_name(None, None, None, 123), "ID 123");
    }
    #[test]
    fn valid_address_opens_only_fixed_kaspa_stream_host() {
        let wallet = format!("kaspa:{}", "q".repeat(61));
        let url = kaspa_stream_url(&wallet).unwrap();
        assert_eq!(
            url.as_str(),
            format!("https://kaspa.stream/addresses/kaspa%3A{}", "q".repeat(61))
        );
        assert!(kaspa_stream_url("https://evil.example/test").is_none());
        assert!(kaspa_stream_url("kaspa:../evil").is_none());
    }
    #[test]
    fn exact_wallet_balance_precision() {
        assert_eq!(exact_kas(123456789), "1.23456789");
        assert_eq!(exact_kas(0), "0.00000000");
    }
}
