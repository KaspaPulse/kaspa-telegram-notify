use crate::domain::models::AppContext;
use crate::network::stats_use_cases::GetMarketStatsUseCase;
use crate::network::stats_use_cases::NetworkStatsUseCase;
use crate::presentation::telegram::formatting::kaspa::KaspaFormatter;
use kaspa_rpc_core::api::rpc::RpcApi;
use std::sync::Arc;
use teloxide::prelude::*;

pub async fn handle_network_overview(
    bot: Bot,
    msg: Message,
    app_context: Arc<AppContext>,
    network_stats: Arc<NetworkStatsUseCase>,
) -> anyhow::Result<()> {
    let mut text = String::from("🛠️ <b>Enterprise Network Health</b>\n━━━━━━━━━━━━━━━━━━\n");
    if let Ok(info) = app_context.rpc.get_server_info().await {
        text.push_str(&format!(
            "⚙️ <b>Core:</b> <code>{}</code>\n🌐 <b>Network:</b> <code>{}</code>\n",
            info.server_version, info.network_id
        ));
    } else {
        text.push_str("⚠️ <b>Node RPC is offline.</b>\n");
    }

    if let Ok((is_online, peers, hashrate)) = network_stats.get_network_overview().await {
        text.push_str(&format!(
            "👥 <b>Connected Peers:</b> <code>{}</code>\n",
            peers
        ));
        text.push_str(&format!(
            "⛏️ <b>Global Hashrate:</b> <code>{}</code>\n",
            KaspaFormatter::format_hashrate(hashrate)
        ));
        text.push_str(&format!(
            "🩺 <b>Status:</b> {}\n",
            if is_online {
                "Online 🟢"
            } else {
                "Offline 🔴"
            }
        ));
    }

    if let Ok(sync) = app_context.rpc.get_sync_status().await {
        text.push_str(&format!(
            "🔄 <b>Sync Status:</b> {}\n",
            if sync {
                "100% Synced ✅"
            } else {
                "Syncing ⚠️"
            }
        ));
    }
    if let Ok(dag) = app_context.rpc.get_block_dag_info().await {
        text.push_str(&format!(
            "🎯 <b>Active Tips:</b> <code>{}</code>\n",
            dag.tip_hashes.len()
        ));
    }
    let text = format!("{}\n\n⏱️ <code>{}</code>", text, chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"));
        let markup = crate::utils::refresh_markup("refresh_network");
    let _ = crate::utils::send_reply_or_edit_log(&bot, msg.chat.id, msg.id, msg.from.as_ref().filter(|u| u.is_bot).map(|_| msg.id), text, Some(markup)).await;
    Ok(())
}

pub async fn handle_dag(
    bot: Bot,
    msg: Message,
    app_context: Arc<AppContext>,
    dag_uc: Arc<crate::network::analyze_dag::AnalyzeDagUseCase>,
) -> anyhow::Result<()> {
    if let Ok(info) = app_context.rpc.get_block_dag_info().await {
        let mut text = String::from("📊 <b>Advanced BlockDAG Forensics</b>\n━━━━━━━━━━━━━━━━━━\n");
        text.push_str(&format!(
            "🧱 <b>Total Blocks:</b> <code>{}</code>\n",
            info.block_count
        ));
        text.push_str(&format!(
            "📜 <b>Total Headers:</b> <code>{}</code>\n",
            info.header_count
        ));
        text.push_str(&format!(
            "📈 <b>Difficulty:</b> <code>{}</code>\n",
            crate::presentation::telegram::formatting::kaspa::KaspaFormatter::format_difficulty(
                info.difficulty
            )
        ));
        text.push_str(&format!(
            "✂️ <b>Pruning Point:</b> <code>{}...</code>\n",
            &info
                .pruning_point_hash
                .to_string()
                .chars()
                .take(8)
                .collect::<String>()
        ));

        // 🔥 ZERO WASTE: Utilizing the node provider and BlockData fields!
        if let Some(block) = dag_uc
            .get_pruning_block(&info.pruning_point_hash.to_string())
            .await
        {
            text.push_str(&format!(
                "⏳ <b>Pruning Timestamp:</b> <code>{}</code>\n",
                block.timestamp
            ));
            text.push_str(&format!(
                "🗃️ <b>Pruning TXs:</b> <code>{}</code>\n",
                block.transaction_ids.len()
            ));
        }

        let health = if info.block_count == info.header_count {
            "Healthy 🟢"
        } else {
            "Syncing 🟡"
        };
        text.push_str(&format!("\n🩺 <b>DAG Health:</b> {}", health));
        let text = format!("{}\n\n⏱️ <code>{}</code>", text, chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"));
        let markup = crate::utils::refresh_markup("refresh_dag");
        let _ = crate::utils::send_reply_or_edit_log(&bot, msg.chat.id, msg.id, msg.from.as_ref().filter(|u| u.is_bot).map(|_| msg.id), text, Some(markup)).await;
    } else {
        crate::send_logged!(bot, msg, "⚠️ Node offline.");
    }
    Ok(())
}

pub async fn handle_fees(bot: Bot, msg: Message) -> anyhow::Result<()> {
    if let Ok(r) = reqwest::get("https://api.kaspa.org/info/fee-estimate").await {
        if let Ok(j) = r.json::<serde_json::Value>().await {
            let normal = j["normalBuckets"][0]["feerate"].as_f64().unwrap_or(1.0);
            let priority = j["priorityBucket"]["feerate"]
                .as_f64()
                .unwrap_or(normal * 1.5);
            let low = j["lowBuckets"][0]["feerate"]
                .as_f64()
                .unwrap_or(normal * 0.5);
            let text = format!("⛽ <b>Network Fee Market (Mempool)</b>\n━━━━━━━━━━━━━━━━━━\n🚀 <b>Priority:</b> <code>{:.2} sompi/gram</code>\n⚡ <b>Normal:</b> <code>{:.2} sompi/gram</code>\n🐢 <b>Low:</b> <code>{:.2} sompi/gram</code>\n\n<i>* Standard transaction size is ~3000 mass.</i>", priority, normal, low);
            crate::send_logged!(bot, msg, text);
            return Ok(());
        }
    }
    crate::send_logged!(bot, msg, "⚠️ Kaspa.org API unreachable.");
    Ok(())
}

pub async fn handle_supply(
    bot: Bot,
    msg: Message,
    app_context: Arc<AppContext>,
) -> anyhow::Result<()> {
    if let Ok(supply) = app_context.rpc.get_coin_supply().await {
        let circ = supply.circulating_sompi as f64 / 1e8;
        let max = supply.max_sompi as f64 / 1e8;
        let text = format!("🪙 <b>Coin Supply:</b>\n├ <b>Circulating:</b> <code>{} KAS</code>\n├ <b>Max Supply:</b> <code>{} KAS</code>\n└ <b>Minted:</b> <code>{:.2}%</code>", circ, max, (circ / max) * 100.0);
        crate::send_logged!(bot, msg, text);
    } else {
        crate::send_logged!(bot, msg, "⚠️ Node offline. Cannot fetch supply.");
    }
    Ok(())
}

pub async fn handle_market_data(
    bot: teloxide::prelude::Bot,
    msg: teloxide::prelude::Message,
    market_stats: Arc<GetMarketStatsUseCase>,
) -> anyhow::Result<()> {
    match market_stats.execute().await {
        Ok(res) => {
            let online_indicator = if res.is_online {
                "🟢 Online"
            } else {
                "🔴 Offline"
            };
            let text = format!("📈 <b>Kaspa Market Data (Enterprise)</b>\n━━━━━━━━━━━━━━━━━━\n💲 <b>Price:</b> <code>${:.4} USD</code>\n🏦 <b>Market Cap:</b> <code>${:.0}</code>\n⛏️ <b>Network Hashrate:</b> <code>{}</code>\n👥 <b>Node Peers:</b> <code>{}</code>\n🩺 <b>Status:</b> {}\n✂️ <b>Pruning Pt:</b> <code>{}...</code>",
                res.price, res.mcap, KaspaFormatter::format_hashrate(res.hashrate), res.peers, online_indicator, &res.pruning_point.chars().take(8).collect::<String>());
            let text = format!("{}\n\n⏱️ <code>{}</code>", text, chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"));
        let markup = crate::utils::refresh_markup("refresh_market");
            let _ = crate::utils::send_reply_or_edit_log(&bot, msg.chat.id, msg.id, msg.from.as_ref().filter(|u| u.is_bot).map(|_| msg.id), text, Some(markup)).await;
        }
        Err(_) => {
            crate::send_logged!(bot, msg, "⚠️ <b>Market Data API unreachable.</b>");
        }
    }
    Ok(())
}

