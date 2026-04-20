use chrono::Utc;
use kaspa_addresses::Address;
use kaspa_rpc_core::api::rpc::RpcApi;
use rev_lines::RevLines;
use std::collections::HashSet;
use std::io::BufReader;
use std::sync::atomic::Ordering;
use sysinfo::System;
use teloxide::{prelude::*, types::ChatId};
use tokio::time::Instant;

use crate::context::AppContext;
use crate::utils::{format_short_wallet, refresh_markup, send_or_edit_log};

pub async fn handle_stats(
    bot: Bot,
    chat_id: ChatId,
    user_id: i64,
    ctx: &AppContext,
    edit_msg_id: Option<teloxide::types::MessageId>,
    current_utc_time: String,
) {
    if user_id != ctx.admin_id {
        return;
    }
    let total_users: HashSet<i64> = ctx.state.iter().flat_map(|e| e.value().clone()).collect();
    let ping = Instant::now();
    let status = match ctx.rpc.get_server_info().await {
        Ok(_) => format!("Online 🟢 ({}ms)", ping.elapsed().as_millis()),
        Err(_) => "Offline 🔴".to_string(),
    };
    let mut text = format!("📊 <b>Enterprise Analytics</b>\n━━━━━━━━━━━━━━━━━━\n👥 <b>Active Users:</b> {}\n💼 <b>Tracked Wallets:</b> {}\n🌐 <b>Node Ping:</b> {}\n\n📋 <b>Detailed User Report:</b>\n", total_users.len(), ctx.state.len(), status);

    for e in ctx.state.iter() {
        let wallet = e.key();
        let users = e.value();
        let mut bal = 0.0;
        if let Ok(addr) = Address::try_from(wallet.as_str()) {
            if let Ok(utxos) = ctx.rpc.get_utxos_by_addresses(vec![addr]).await {
                bal = utxos
                    .iter()
                    .map(|u| u.utxo_entry.amount as f64)
                    .sum::<f64>()
                    / 1e8;
            }
        }
        let blocks: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM mined_blocks WHERE wallet = $1")
            .bind(wallet)
            .fetch_one(&ctx.pool)
            .await
            .unwrap_or((0,));

        let mut user_names = Vec::new();
        for &u_id in users {
            if let Ok(chat) = bot.get_chat(ChatId(u_id)).await {
                let name = chat
                    .username()
                    .map(|u| format!("@{}", u))
                    .unwrap_or_else(|| "User".to_string());
                user_names.push(format!("{} [{}]", name, u_id));
            } else {
                user_names.push(u_id.to_string());
            }
        }
        text.push_str(&format!(
            "▪ <code>{}</code>\n  ├ Users: {}\n  ├ Balance: {:.2} KAS\n  └ Mined Blocks: {}\n\n",
            format_short_wallet(wallet),
            user_names.join(", "),
            bal,
            blocks.0
        ));
    }
    text.push_str(&format!("⏱️ <code>{}</code>", current_utc_time));
    if let Err(e) = send_or_edit_log(
        &bot,
        chat_id,
        edit_msg_id,
        text,
        Some(refresh_markup("refresh_stats")),
    )
    .await { tracing::error!("[UI ERROR] Failed to send/edit message: {}", e); }
}

pub async fn handle_sys(
    bot: Bot,
    chat_id: ChatId,
    user_id: i64,
    ctx: &AppContext,
    edit_msg_id: Option<teloxide::types::MessageId>,
    current_time: String,
) {
    if user_id != ctx.admin_id {
        return;
    }
    let monitoring = ctx.monitoring.load(Ordering::Relaxed);
    let (used_mem, total_mem, cores, uptime_secs, os_name, used_disk, total_disk) =
        tokio::task::spawn_blocking(move || {
            let mut s = System::new_all();
            s.refresh_all();
            let used_m = s.used_memory() / 1024 / 1024;
            let total_m = s.total_memory() / 1024 / 1024;
            let c = s.physical_core_count().unwrap_or(0);
            let up = sysinfo::System::uptime();
            let os = sysinfo::System::long_os_version().unwrap_or_else(|| "Unknown OS".to_string());
            let disks = sysinfo::Disks::new_with_refreshed_list();
            let mut total_d_bytes = 0;
            let mut avail_d_bytes = 0;
            for disk in &disks {
                total_d_bytes += disk.total_space();
                avail_d_bytes += disk.available_space();
            }
            let used_d_gb = (total_d_bytes.saturating_sub(avail_d_bytes)) / 1024 / 1024 / 1024;
            let total_d_gb = total_d_bytes / 1024 / 1024 / 1024;
            (used_m, total_m, c, up, os, used_d_gb, total_d_gb)
        })
        .await
        .unwrap_or((0, 0, 0, 0, "Unknown".to_string(), 0, 0));

    let days = uptime_secs / 86400;
    let hours = (uptime_secs % 86400) / 3600;
    let minutes = (uptime_secs % 3600) / 60;
    let text = format!("⚙️ <b>Enterprise Node Diagnostics:</b>\n🖥️ <b>OS:</b> <code>{}</code>\n⏳ <b>Uptime:</b> <code>{}d {}h {}m</code>\n🎛️ <b>CPU:</b> <code>{} Cores</code>\n🧠 <b>RAM:</b> <code>{} / {} MB</code>\n💾 <b>Storage:</b> <code>{} / {} GB</code>\n👀 <b>Monitor:</b> <code>{}</code>\n\n⏱️ <code>{}</code>", os_name, days, hours, minutes, cores, used_mem, total_mem, used_disk, total_disk, monitoring, current_time);
    if let Err(e) = send_or_edit_log(
        &bot,
        chat_id,
        edit_msg_id,
        text,
        Some(refresh_markup("refresh_sys")),
    )
    .await { tracing::error!("[UI ERROR] Failed to send/edit message: {}", e); }
}

pub async fn handle_pause(bot: Bot, chat_id: ChatId, user_id: i64, ctx: &AppContext) {
    if user_id == ctx.admin_id {
        ctx.monitoring.store(false, Ordering::Relaxed);
        if let Err(e) = send_or_edit_log(
            &bot,
            chat_id,
            None,
            "⏸️ <b>Monitoring Paused.</b>".to_string(),
            None,
        )
        .await { tracing::error!("[UI ERROR] Failed to send/edit message: {}", e); }
    }
}

pub async fn handle_resume(bot: Bot, chat_id: ChatId, user_id: i64, ctx: &AppContext) {
    if user_id == ctx.admin_id {
        ctx.monitoring.store(true, Ordering::Relaxed);
        if let Err(e) = send_or_edit_log(
            &bot,
            chat_id,
            None,
            "▶️ <b>Monitoring Active.</b>".to_string(),
            None,
        )
        .await { tracing::error!("[UI ERROR] Failed to send/edit message: {}", e); }
    }
}

pub async fn handle_restart(bot: Bot, chat_id: ChatId, user_id: i64, ctx: &AppContext) {
    if user_id == ctx.admin_id {
        if let Err(e) = send_or_edit_log(
            &bot,
            chat_id,
            None,
            "🔄 <b>Restarting safely...</b>".to_string(),
            None,
        )
        .await { tracing::error!("[UI ERROR] Failed to send/edit message: {}", e); }
        tracing::info!("[SYSTEM] Restarting binary per admin request...");        
        std::process::exit(0);
    }
}

pub async fn handle_broadcast(
    bot: Bot,
    chat_id: ChatId,
    user_id: i64,
    m: String,
    ctx: &AppContext,
) {
    if user_id == ctx.admin_id {
        let users: HashSet<i64> = ctx.state.iter().flat_map(|e| e.value().clone()).collect();
        let count = users.len();
        for u in users {
            let bot_clone = bot.clone();
            let msg_text = format!("📢 <b>Admin Broadcast:</b>\n\n{}", m);
            tokio::spawn(async move {
                let _ = bot_clone
                    .send_message(ChatId(u), msg_text)
                    .parse_mode(teloxide::types::ParseMode::Html)
                    .await;
            });
        }
        if let Err(e) = send_or_edit_log(
            &bot,
            chat_id,
            None,
            format!("✅ Broadcast sent to {} users.", count),
            None,
        )
        .await { tracing::error!("[UI ERROR] Failed to send/edit message: {}", e); }
    }
}

pub async fn handle_logs(bot: Bot, chat_id: ChatId, user_id: i64, ctx: &AppContext) {
    if user_id == ctx.admin_id {
        if let Ok(file) = std::fs::File::open("bot.log") {
            let mut lines: Vec<String> = RevLines::new(BufReader::new(file))
                .take(25)
                .filter_map(Result::ok)
                .collect();
            lines.reverse();
            if let Err(e) = send_or_edit_log(
                &bot,
                chat_id,
                None,
                format!(
                    "📜 <b>System Logs (Tail):</b>\n<pre>{}</pre>",
                    lines.join("\n")
                ),
                None,
            )
            .await { tracing::error!("[UI ERROR] Failed to send/edit message: {}", e); }
        }
    }
}

pub async fn handle_learn(
    bot: Bot,
    chat_id: ChatId,
    user_id: i64,
    new_fact: String,
    ctx: &AppContext,
) {
    if user_id != ctx.admin_id {
        return;
    }
    if new_fact.trim().is_empty() {
        let _ = bot
            .send_message(chat_id, "⚠️ Usage: /learn [New Kaspa Information]")
            .await;
        return;
    }

    let title = format!("Manual Input: {}", Utc::now().format("%Y-%m-%d %H:%M"));
    let link = format!("manual-{}", Utc::now().timestamp_nanos_opt().unwrap_or(0));

    // Write directly to the new PostgreSQL Knowledge Base
    let res = sqlx::query(
        "INSERT INTO knowledge_base (title, link, content, source, published_at) VALUES ($1, $2, $3, 'Admin Manual Input', CURRENT_TIMESTAMP) ON CONFLICT (link) DO NOTHING"
    )
    .bind(&title)
    .bind(&link)
    .bind(&new_fact)
    .execute(&ctx.pool)
    .await;

    if res.is_ok() {
        let _ = bot
            .send_message(
                chat_id,
                "🧠 <b>Knowledge Added!</b>\nI have securely stored this in my vector database.",
            )
            .parse_mode(teloxide::types::ParseMode::Html)
            .await;
    } else {
        let _ = bot
            .send_message(
                chat_id,
                "❌ <b>Database Error:</b> Failed to store new knowledge.",
            )
            .parse_mode(teloxide::types::ParseMode::Html)
            .await;
    }
}

pub async fn handle_autolearn(bot: Bot, chat_id: ChatId, user_id: i64, ctx: &AppContext) {
    if user_id != ctx.admin_id {
        return;
    }
    tracing::info!("🔍 [AUTOLEARN] Connecting to Official RSS via Manual Trigger...");
    let _ = bot
        .send_message(
            chat_id,
            "🔍 <b>Kaspa AI:</b> Force-scanning the Official Kaspa News Feed...",
        )
        .parse_mode(teloxide::types::ParseMode::Html)
        .await;

    let feeds =
        vec!["https://api.rss2json.com/v1/api.json?rss_url=https://medium.com/feed/kaspa-currency"];
    let mut added_count = 0;
    let mut titles_added = String::new();

    for feed_url in feeds {
        if let Ok(r) = reqwest::get(feed_url).await {
            if let Ok(j) = r.json::<serde_json::Value>().await {
                if let Some(items) = j["items"].as_array() {
                    for item in items {
                        let title = item["title"].as_str().unwrap_or("Unknown").to_string();
                        let link = item["link"]
                            .as_str()
                            .unwrap_or(&format!(
                                "rss2json-{}",
                                Utc::now().timestamp_nanos_opt().unwrap_or(0)
                            ))
                            .to_string();
                        let content_html = item["description"].as_str().unwrap_or("").to_string();
                        let clean_content = crate::utils::clean_for_log(&content_html);

                        if clean_content.len() < 50 {
                            continue;
                        }

                        // Write directly to the new PostgreSQL Knowledge Base
                        let res = sqlx::query(
                            "INSERT INTO knowledge_base (title, link, content, source, published_at) VALUES ($1, $2, $3, 'Admin Autolearn Trigger', CURRENT_TIMESTAMP) ON CONFLICT (link) DO NOTHING"
                        )
                        .bind(&title)
                        .bind(&link)
                        .bind(&clean_content)
                        .execute(&ctx.pool)
                        .await;

                        if let Ok(db_res) = res {
                            if db_res.rows_affected() > 0 {
                                added_count += 1;
                                titles_added.push_str(&format!("▪ {}\n", title));
                            }
                        }
                    }
                }
            }
        }
    }

    if added_count > 0 {
        tracing::info!(
            "💾 [AUTOLEARN] Saved {} new articles directly to local database.",
            added_count
        );
        if let Err(e) = bot.send_message(chat_id, format!("🧠 <b>Official Learning Complete!</b>\n\n📖 <b>Learned {} New Topics:</b>\n{}\n<i>My internal database has been updated instantly!</i>", added_count, titles_added)).parse_mode(teloxide::types::ParseMode::Html).await { tracing::error!("[TELEGRAM API ERROR] Failed to execute: {}", e); }
    } else {
        if let Err(e) = bot.send_message(chat_id, "✅ <b>AI Status:</b> Scanned Official Medium. No new posts found. Database is completely up to date.").parse_mode(teloxide::types::ParseMode::Html).await { tracing::error!("[TELEGRAM API ERROR] Failed to execute: {}", e); }
    }
}

// Access Control Verification
#[allow(dead_code)]
pub fn verify_admin_access(user_id: Option<u64>) -> bool {
    let admin_id: u64 = std::env::var("ADMIN_ID")
        .unwrap_or_else(|_| "0".to_string())
        .parse()
        .unwrap_or(0);
        
    if let Some(id) = user_id {
        if id == admin_id {
            return true;
        } else {
            tracing::warn!("⚠️ UNAUTHORIZED ADMIN ATTEMPT from User ID: {}", id);
            return false;
        }
    }
    false
}

// ==============================================================================
// ENTERPRISE CONFIGURATION PANEL
// ==============================================================================

pub async fn handle_settings(bot: Bot, chat_id: ChatId, user_id: i64, ctx: &AppContext) {
    if user_id != ctx.admin_id { return; }

    let keys = vec![
        ("ENABLE_RSS_WORKER", "📰 News crawler activity.", "true"),
        ("ENABLE_MEMORY_CLEANER", "🧠 Periodic RAM purge.", "true"),
        ("ENABLE_LIVE_SYNC", "🔄 Real-time node indexing.", "true"),
        ("ENABLE_AI_VECTORIZER", "🤖 AI Knowledge indexing.", "true"),
        ("ENABLE_AI_CHAT", "💬 AI Text Chat (LLM).", "true"),
        ("ENABLE_AI_VOICE", "🎤 AI Voice Analysis (Whisper).", "true"),
        ("MAINTENANCE_MODE", "🔒 Restricted admin-only mode.", "false"),
    ];

    let mut response = String::from("⚙️ <b>Enterprise Control Panel (Database)</b>\n━━━━━━━━━━━━━━━━━━\n");

    for (key, desc, default_val) in keys {
        let value = crate::state::get_setting(&ctx.pool, key, default_val).await;
        let status_icon = if value == "true" { "🟢" } else if value == "false" { "🔴" } else { "⚙️" };
        response.push_str(&format!(
            "{} <b>{}</b>: <code>{}</code>\n<i>{}</i>\nToggle: <code>/toggle {}</code>\n\n",
            status_icon, key, value, desc, key
        ));
    }

    if let Err(e) = bot.send_message(chat_id, response).parse_mode(teloxide::types::ParseMode::Html).await { tracing::error!("[TELEGRAM API ERROR] Failed to execute: {}", e); }
}

pub async fn handle_toggle(bot: Bot, chat_id: ChatId, user_id: i64, input: String, ctx: &AppContext) {
    if user_id != ctx.admin_id { return; }

    let parts: Vec<&str> = input.split('=').collect();
    let key = parts[0].trim().to_uppercase();

    // Enterprise Security: Block modification of critical environment secrets
    let restricted = vec!["BOT_TOKEN", "DATABASE_URL", "AI_API_KEY", "ADMIN_ID"];
    if restricted.contains(&key.as_str()) {
        let _ = bot.send_message(chat_id, "🚫 <b>Security Alert:</b> Modifying core secrets is restricted. Use the .env file on the server.").parse_mode(teloxide::types::ParseMode::Html).await;
        return;
    }

    let current_val = crate::state::get_setting(&ctx.pool, &key, "false").await;
    
    let new_val = if parts.len() > 1 {
        parts[1].trim().to_string() 
    } else if current_val == "true" {
        "false".to_string() 
    } else {
        "true".to_string() 
    };

    if let Ok(_) = crate::state::update_setting(&ctx.pool, &key, &new_val).await {
        let _ = bot.send_message(chat_id, format!("✅ <b>{}</b> updated to <code>{}</code>\n<i>Changes applied instantly. No restart required.</i>", key, new_val)).parse_mode(teloxide::types::ParseMode::Html).await;
    } else {
        let _ = bot.send_message(chat_id, "❌ <b>Database Error:</b> Failed to update the setting.").parse_mode(teloxide::types::ParseMode::Html).await;
    }
}
