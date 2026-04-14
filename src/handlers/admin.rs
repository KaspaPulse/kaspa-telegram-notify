use teloxide::{prelude::*, types::ChatId};
use std::sync::atomic::Ordering;
use sysinfo::System;
use crate::context::AppContext;
use crate::utils::{send_or_edit_log, refresh_markup};

pub async fn handle_sys(bot: Bot, chat_id: ChatId, user_id: i64, ctx: &AppContext, edit_id: Option<teloxide::types::MessageId>, current_time: String) {
    if user_id != ctx.admin_id { return; }
    let monitoring = ctx.monitoring.load(Ordering::Relaxed);
    
    let (used_mem, total_mem, cores, uptime_secs, os_name) = tokio::task::spawn_blocking(move || {
        let mut s = System::new_all();
        s.refresh_all();
        (s.used_memory() / 1024 / 1024, s.total_memory() / 1024 / 1024, s.physical_core_count().unwrap_or(0), sysinfo::System::uptime(), sysinfo::System::long_os_version().unwrap_or_default())
    }).await.unwrap_or_default();

    let text = format!("⚙️ <b>Enterprise Node:</b>\n🖥️ <b>OS:</b> <code>{}</code>\n⏳ <b>Uptime:</b> <code>{}s</code>\n🎛️ <b>CPU:</b> <code>{} Cores</code>\n🧠 <b>RAM:</b> <code>{} / {} MB</code>\n👀 <b>Monitor:</b> <code>{}</code>\n⏱️ <code>{}</code>", os_name, uptime_secs, cores, used_mem, total_mem, monitoring, current_time);
    let _ = send_or_edit_log(&bot, chat_id, edit_id, text, Some(refresh_markup("refresh_sys"))).await;
}
