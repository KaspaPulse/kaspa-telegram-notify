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
    let mut text = String::from("🛠️ <b>Network Health</b>\n━━━━━━━━━━━━━━━━━━\n");

    let mut network_name = String::from("unknown");

    if let Ok(info) = app_context.rpc.get_server_info().await {
        network_name = info.network_id.to_string();

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

    let live_bps = estimate_live_bps(app_context.clone()).await;
    let expected_bps = expected_bps_for_network(&network_name);

    text.push_str(&format!(
        "\n⚡ <b>Live BPS:</b> <code>{:.2}</code>\n🎯 <b>Expected BPS:</b> <code>{:.1}</code>",
        live_bps.unwrap_or(0.0),
        expected_bps
    ));

    let text = format!(
        "{}\n\n⏱️ <code>{}</code>",
        text,
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    );

    let markup = crate::utils::refresh_markup("refresh_network");

    let _ = crate::utils::send_reply_or_edit_log(
        &bot,
        msg.chat.id,
        msg.id,
        msg.from.as_ref().filter(|u| u.is_bot).map(|_| msg.id),
        text,
        Some(markup),
    )
    .await;

    Ok(())
}

pub async fn handle_dag(
    bot: Bot,
    msg: Message,
    app_context: Arc<AppContext>,
    dag_uc: Arc<crate::network::analyze_dag::AnalyzeDagUseCase>,
) -> anyhow::Result<()> {
    let server_info = app_context.rpc.get_server_info().await.ok();
    let network_name = server_info
        .as_ref()
        .map(|i| i.network_id.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    if let Ok(info) = app_context.rpc.get_block_dag_info().await {
        let mut text = String::from("📊 <b>BlockDAG Overview</b>\n━━━━━━━━━━━━━━━━━━\n");

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
            KaspaFormatter::format_difficulty(info.difficulty)
        ));
        text.push_str(&format!(
            "✂️ <b>Pruning Point:</b> <code>{}...</code>\n",
            info.pruning_point_hash
                .to_string()
                .chars()
                .take(8)
                .collect::<String>()
        ));

        if let Some(block) = dag_uc
            .get_pruning_block(&info.pruning_point_hash.to_string())
            .await
        {
            text.push_str(&format!(
                "⏳ <b>Pruning Time:</b> <code>{}</code>\n",
                format_kaspa_timestamp(block.timestamp)
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

        let live_bps = estimate_live_bps(app_context.clone()).await;
        let expected_bps = expected_bps_for_network(&network_name);

        text.push_str(&format!(
            "\n🩺 <b>DAG Health:</b> {}\n⚡ <b>Live BPS:</b> <code>{:.2}</code>\n🎯 <b>Expected BPS:</b> <code>{:.1}</code>",
            health,
            live_bps.unwrap_or(0.0),
            expected_bps
        ));

        let text = format!(
            "{}\n\n⏱️ <code>{}</code>",
            text,
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
        );

        let markup = crate::utils::refresh_markup("refresh_dag");

        let _ = crate::utils::send_reply_or_edit_log(
            &bot,
            msg.chat.id,
            msg.id,
            msg.from.as_ref().filter(|u| u.is_bot).map(|_| msg.id),
            text,
            Some(markup),
        )
        .await;
    } else {
        crate::send_logged!(bot, msg, "⚠️ Node offline.");
    }

    Ok(())
}

async fn fetch_fee_estimate() -> Result<serde_json::Value, String> {
    let client = crate::infrastructure::resilience::runtime::build_http_client()
        .map_err(|error| error.to_string())?;
    let timeout = crate::infrastructure::resilience::runtime::http_timeout_duration();
    let response = crate::infrastructure::resilience::runtime::with_timeout_result(
        "kaspa.org fee estimate request",
        timeout,
        client.get("https://api.kaspa.org/info/fee-estimate").send(),
    )
    .await?;
    let response = response
        .error_for_status()
        .map_err(|error| format!("kaspa.org fee estimate HTTP status failed: {error}"))?;

    crate::infrastructure::resilience::runtime::with_timeout_result(
        "kaspa.org fee estimate json parse",
        timeout,
        response.json::<serde_json::Value>(),
    )
    .await
}

pub async fn handle_fees(bot: Bot, msg: Message) -> anyhow::Result<()> {
    match fetch_fee_estimate().await {
        Ok(json) => {
            let normal = json["normalBuckets"][0]["feerate"].as_f64().unwrap_or(1.0);
            let priority = json["priorityBucket"]["feerate"]
                .as_f64()
                .unwrap_or(normal * 1.5);
            let low = json["lowBuckets"][0]["feerate"]
                .as_f64()
                .unwrap_or(normal * 0.5);

            let text = format!(
                "⛽ <b>Network Fee Market</b>\n\
                 ━━━━━━━━━━━━━━━━━━\n\
                 🚀 <b>Priority:</b> <code>{:.2} sompi/gram</code>\n\
                 ⚡ <b>Normal:</b> <code>{:.2} sompi/gram</code>\n\
                 🐢 <b>Low:</b> <code>{:.2} sompi/gram</code>\n\n\
                 <i>* Standard transaction size is ~3000 mass.</i>\n\n\
                 ⏱️ <code>{}</code>",
                priority,
                normal,
                low,
                chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
            );

            let markup = crate::utils::refresh_markup("refresh_fees");
            let _ = crate::utils::send_reply_or_edit_log(
                &bot,
                msg.chat.id,
                msg.id,
                msg.from.as_ref().filter(|u| u.is_bot).map(|_| msg.id),
                text,
                Some(markup),
            )
            .await;
        }
        Err(error) => {
            tracing::warn!(
                error = %crate::utils::sanitize_for_log(&error),
                "[NETWORK FEES] Kaspa.org fee estimate request failed."
            );
            crate::send_logged!(bot, msg, "⚠️ Kaspa.org API unreachable.");
        }
    }

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

        let text = format!(
            "🪙 <b>Coin Supply</b>\n\
             ━━━━━━━━━━━━━━━━━━\n\
             ├ <b>Circulating:</b> <code>{} KAS</code>\n\
             ├ <b>Max Supply:</b> <code>{} KAS</code>\n\
             └ <b>Minted:</b> <code>{:.2}%</code>\n\n\
             ⏱️ <code>{}</code>",
            circ,
            max,
            (circ / max) * 100.0,
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
        );

        let markup = crate::utils::refresh_markup("refresh_supply");

        let _ = crate::utils::send_reply_or_edit_log(
            &bot,
            msg.chat.id,
            msg.id,
            msg.from.as_ref().filter(|u| u.is_bot).map(|_| msg.id),
            text,
            Some(markup),
        )
        .await;
    } else {
        crate::send_logged!(bot, msg, "⚠️ Node offline. Cannot fetch supply.");
    }

    Ok(())
}

pub async fn handle_market_data(
    bot: Bot,
    msg: Message,
    app_context: Arc<AppContext>,
    market_stats: Arc<GetMarketStatsUseCase>,
) -> anyhow::Result<()> {
    let server_info = app_context.rpc.get_server_info().await.ok();
    let network_name = server_info
        .as_ref()
        .map(|i| i.network_id.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let live_bps = estimate_live_bps(app_context.clone()).await.unwrap_or(0.0);
    let expected_bps = expected_bps_for_network(&network_name);

    match market_stats.execute().await {
        Ok(res) => {
            let online_indicator = if res.is_online {
                "🟢 Online"
            } else {
                "🔴 Offline"
            };

            let text = format!(
                "📈 <b>Kaspa Market Data</b>\n\
                 ━━━━━━━━━━━━━━━━━━\n\
                 💲 <b>Price:</b> <code>${:.4} USD</code>\n\
                 🏦 <b>Market Cap:</b> <code>${}</code>\n\
                 🌐 <b>Network:</b> <code>{}</code>\n\
                 ⛏️ <b>Network Hashrate:</b> <code>{}</code>\n\
                 👥 <b>Node Peers:</b> <code>{}</code>\n\
                 🩺 <b>Status:</b> {}\n\
                 ✂️ <b>Pruning Point:</b> <code>{}...</code>\n\
                 ⚡ <b>Live BPS:</b> <code>{:.2}</code>\n\
                 🎯 <b>Expected BPS:</b> <code>{:.1}</code>\n\n\
                 ⏱️ <code>{}</code>",
                res.price,
                format_number(res.mcap),
                network_name,
                KaspaFormatter::format_hashrate(res.hashrate),
                res.peers,
                online_indicator,
                res.pruning_point.chars().take(8).collect::<String>(),
                live_bps,
                expected_bps,
                chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
            );

            let markup = crate::utils::refresh_markup("refresh_market");

            let _ = crate::utils::send_reply_or_edit_log(
                &bot,
                msg.chat.id,
                msg.id,
                msg.from.as_ref().filter(|u| u.is_bot).map(|_| msg.id),
                text,
                Some(markup),
            )
            .await;
        }
        Err(_) => {
            crate::send_logged!(bot, msg, "⚠️ <b>Market data API unreachable.</b>");
        }
    }

    Ok(())
}
async fn estimate_live_bps(app_context: Arc<AppContext>) -> Option<f64> {
    let first = app_context.rpc.get_block_dag_info().await.ok()?;
    let first_score = first.block_count;

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let second = app_context.rpc.get_block_dag_info().await.ok()?;
    let second_score = second.block_count;

    if second_score <= first_score {
        return Some(0.0);
    }

    Some((second_score - first_score) as f64 / 2.0)
}

fn expected_bps_for_network(network_name: &str) -> f64 {
    if network_name.to_lowercase().contains("mainnet") {
        10.0
    } else {
        1.0
    }
}

fn format_kaspa_timestamp(timestamp: u64) -> String {
    let seconds = if timestamp > 10_000_000_000 {
        (timestamp / 1000) as i64
    } else {
        timestamp as i64
    };

    match chrono::DateTime::<chrono::Utc>::from_timestamp(seconds, 0) {
        Some(dt) => dt.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        None => format!("Invalid timestamp ({})", timestamp),
    }
}

fn format_number(value: f64) -> String {
    let raw = format!("{:.0}", value);
    let mut out = String::new();

    for (i, ch) in raw.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }

    out.chars().rev().collect()
}
