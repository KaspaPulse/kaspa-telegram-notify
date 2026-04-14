use chrono::{TimeZone, Utc};
use kaspa_addresses::Address;
use kaspa_hashes::Hash;
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_wrpc_client::KaspaRpcClient;
use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::ChatId;
use tokio::sync::Semaphore;
use tokio::time::{sleep, Duration};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::context::AppContext;
use crate::utils::{format_hash, format_short_wallet};

pub fn start_all(ctx: AppContext, bot: Bot, token: CancellationToken) {
    spawn_price_monitor(ctx.clone(), token.clone());
    spawn_node_monitor(ctx.clone(), bot.clone(), token.clone());
    spawn_utxo_monitor(ctx.clone(), bot, token.clone());
    spawn_memory_cleaner(ctx, token);
}

fn spawn_price_monitor(ctx: AppContext, token: CancellationToken) {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = token.cancelled() => { break; }
                _ = tokio::time::sleep(Duration::from_secs(60)) => {
                    let client = reqwest::Client::new();
                    if let Ok(r) = client.get("https://api.coingecko.com/api/v3/simple/price?ids=kaspa&vs_currencies=usd&include_market_cap=true")
                        .header("User-Agent", "Mozilla/5.0").send().await {
                        if let Ok(j) = r.json::<serde_json::Value>().await {
                            let price = j["kaspa"]["usd"].as_f64().unwrap_or(0.0);
                            let mcap = j["kaspa"]["usd_market_cap"].as_f64().unwrap_or(0.0);
                            let mut write_guard = ctx.price_cache.write().await;
                            *write_guard = (price, mcap);
                        }
                    }
                }
            }
        }
    });
}

fn spawn_node_monitor(ctx: AppContext, bot: Bot, token: CancellationToken) {
    tokio::spawn(async move {
        let _ = ctx.rpc.connect(None).await;
        loop {
            tokio::select! {
                _ = token.cancelled() => { break; }
                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                    if ctx.rpc.get_server_info().await.is_err() {
                        error!("[NODE ALERT] RPC Connection Lost! Attempting reconnect...");
                        let _ = bot.send_message(ChatId(ctx.admin_id), "🚨 <b>SYSTEM ALERT:</b> Secure Node connection lost! Attempting auto-reconnect...").parse_mode(teloxide::types::ParseMode::Html).await;
                        let _ = ctx.rpc.connect(None).await;
                    }
                }
            }
        }
    });
}

fn spawn_utxo_monitor(ctx: AppContext, bot: Bot, token: CancellationToken) {
    let semaphore = Arc::new(Semaphore::new(50));
    tokio::spawn(async move {
        sleep(Duration::from_secs(5)).await;
        loop {
            tokio::select! {
                _ = token.cancelled() => { break; }
                _ = tokio::time::sleep(Duration::from_secs(10)) => {
                    if !ctx.monitoring.load(Ordering::Relaxed) { continue; }
                    let check_list: Vec<(String, HashSet<i64>)> = ctx.state.iter().map(|e| (e.key().clone(), e.value().clone())).collect();
                    if check_list.is_empty() { continue; }

                    for (wallet, subs) in check_list {
                        if let Ok(addr) = Address::try_from(wallet.as_str()) {
                            if let Ok(utxos) = ctx.rpc.get_utxos_by_addresses(vec![addr.clone()]).await {
                                let mut current_outpoints = HashSet::new();
                                let mut new_rewards = Vec::new();
                                let mut known = ctx.utxo_state.entry(wallet.clone()).or_insert_with(HashSet::new);
                                let is_first_run = known.is_empty();

                                for entry in utxos {
                                    let tx_id = entry.outpoint.transaction_id.to_string();
                                    let outpoint_id = format!("{}:{}", tx_id, entry.outpoint.index);
                                    current_outpoints.insert(outpoint_id.clone());

                                    if !is_first_run && !known.contains(&outpoint_id) {
                                        new_rewards.push((outpoint_id.clone(), tx_id, entry.utxo_entry.amount as f64 / 1e8, entry.utxo_entry.block_daa_score, entry.utxo_entry.is_coinbase));
                                        known.insert(outpoint_id);
                                    } else if is_first_run { known.insert(outpoint_id); }
                                }
                                known.retain(|k| current_outpoints.contains(k));

                                for (outp, tx_id, diff, daa_score, is_coinbase) in new_rewards {
                                    let mut live_bal = 0.0;
                                    if let Ok(live_utxos) = ctx.rpc.get_utxos_by_addresses(vec![addr.clone()]).await {
                                        live_bal = live_utxos.iter().map(|u| u.utxo_entry.amount as f64).sum::<f64>() / 1e8;
                                    }

                                    let header_emoji = if is_coinbase { "⚡ <b>Native Node Reward!</b> 💎" } else { "💸 <b>Incoming Transfer!</b> 💸" }.to_string();
                                    let (f_tx, w_cl, bot_cl, rpc_cl) = (tx_id.clone(), wallet.clone(), bot.clone(), Arc::clone(&ctx.rpc));
                                    let subs_cl = subs.clone();
                                    let permit = Arc::clone(&semaphore).acquire_owned().await.unwrap();
                                    let pool_cl = ctx.pool.clone();

                                    tokio::spawn(async move {
                                        let _p = permit;
                                        if is_coinbase {
                                            crate::state::record_mined_block(&pool_cl, &outp, &w_cl, diff, daa_score).await;
                                        }

                                        let (acc_block_hash, actual_mined_blocks, extracted_nonce, extracted_worker, block_time_ms) = analyze_block_payload(Arc::clone(&rpc_cl), f_tx.clone(), w_cl.clone(), daa_score, is_coinbase).await;

                                        // 🕒 Formatting the EXACT Block Time with Milliseconds
                                        let time_str = if block_time_ms > 0 {
                                            if let chrono::LocalResult::Single(dt) = Utc.timestamp_millis_opt(block_time_ms as i64) {
                                                dt.format("%Y-%m-%d %H:%M:%S.%3f UTC").to_string()
                                            } else {
                                                Utc::now().format("%Y-%m-%d %H:%M:%S.%3f UTC").to_string()
                                            }
                                        } else {
                                            Utc::now().format("%Y-%m-%d %H:%M:%S.%3f UTC").to_string()
                                        };

                                        let msg_type = if is_coinbase { "⛏️ Solo Mining Reward" } else { "💳 Normal Transfer" };
                                        let acc_block_str = if acc_block_hash.is_empty() { "<code>Not Found (Archived)</code>".to_string() } else { format_hash(&acc_block_hash, "blocks") };
                                        let mined_block_str = if !is_coinbase { "<code>N/A</code>".to_string() } else if actual_mined_blocks.is_empty() { "<code>Not Found (Unknown Miner)</code>".to_string() } else if actual_mined_blocks.len() == 1 { format_hash(&actual_mined_blocks[0], "blocks") } else {
                                            let links: Vec<String> = actual_mined_blocks.iter().map(|b| format!("\n ├ {}", format_hash(b, "blocks"))).collect(); format!("{} Blocks!{}", actual_mined_blocks.len(), links.join(""))
                                        };

                                        let mut final_msg = format!("{}\n━━━━━━━━━━━━━━━━━━\n<b>Time:</b> <code>{}</code>\n<b>Wallet:</b> <a href=\"https://kaspa.stream/addresses/{}\">{}</a>\n<b>Amount:</b> <code>+{:.8} KAS</code>\n<b>Balance:</b> <code>{:.8} KAS</code>\n<blockquote expandable>", header_emoji, time_str, w_cl, format_short_wallet(&w_cl), diff, live_bal);
                                        final_msg.push_str(&format!("<b>TXID:</b> {}\n", format_hash(&f_tx, "transactions")));
                                        if is_coinbase {
                                            final_msg.push_str(&format!("<b>Mined Block(s):</b> {}\n<b>Accepting Block:</b> {}\n", mined_block_str, acc_block_str));
                                            if !extracted_nonce.is_empty() { final_msg.push_str(&format!("<b>Nonce:</b> <code>{}</code>\n<b>Worker:</b> <code>{}</code>\n", extracted_nonce, extracted_worker)); }
                                        } else { final_msg.push_str(&format!("<b>Type:</b> {}\n<b>Accepting Block:</b> {}\n", msg_type, acc_block_str)); }
                                        final_msg.push_str(&format!("<b>DAA Score:</b> <code>{}</code>\n</blockquote>", daa_score));

                                        info!("💎 [BLOCK DISCOVERED] +{:.8} KAS for {}", diff, w_cl);
                                        for user_id in subs_cl {
                                            let _ = bot_cl.send_message(teloxide::types::ChatId(user_id), &final_msg).parse_mode(teloxide::types::ParseMode::Html).link_preview_options(teloxide::types::LinkPreviewOptions { is_disabled: true, url: None, prefer_small_media: false, prefer_large_media: false, show_above_text: false }).await;
                                            sleep(Duration::from_millis(40)).await;
                                        }
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    });
}

async fn analyze_block_payload(
    rpc_cl: Arc<KaspaRpcClient>,
    f_tx: String,
    w_cl: String,
    daa_score: u64,
    is_coinbase: bool,
) -> (String, Vec<String>, String, String, u64) {
    let mut acc_block_hash = String::new();
    let mut actual_mined_blocks: Vec<String> = Vec::new();
    let mut extracted_nonce = String::new();
    let mut extracted_worker = String::new();
    let mut block_time_ms: u64 = 0;
    let mut visited = HashSet::new();
    let mut current_hashes = match rpc_cl.get_block_dag_info().await {
        Ok(info) => info.tip_hashes,
        Err(_) => vec![],
    };

    for _attempt in 1..=800 {
        if current_hashes.is_empty() {
            break;
        }
        let mut next_hashes = vec![];
        for hash in &current_hashes {
            if !visited.insert(*hash) {
                continue;
            }
            if let Ok(block) = rpc_cl.get_block(*hash, true).await {
                let mut found_tx = false;
                for tx in &block.transactions {
                    if let Some(tx_verb) = &tx.verbose_data {
                        if tx_verb.transaction_id.to_string() == f_tx {
                            found_tx = true;
                            break;
                        }
                    }
                }
                if found_tx {
                    acc_block_hash = hash.to_string();
                    block_time_ms = block.header.timestamp;
                    break;
                }
                if block.header.daa_score >= daa_score.saturating_sub(60) {
                    for level in &block.header.parents_by_level {
                        for p_hash in level {
                            next_hashes.push(*p_hash);
                        }
                    }
                }
            }
        }
        if !acc_block_hash.is_empty() {
            break;
        }
        current_hashes = next_hashes;
        sleep(Duration::from_millis(5)).await;
    }

    if is_coinbase && !acc_block_hash.is_empty() {
        if let Ok(acc_hash_obj) = acc_block_hash.parse::<Hash>() {
            if let Ok(full_acc_block) = rpc_cl.get_block(acc_hash_obj, true).await {
                let mut user_script_bytes: Vec<u8> = Vec::new();
                if let Some(tx0) = full_acc_block.transactions.first() {
                    for out in &tx0.outputs {
                        if let Some(ov) = &out.verbose_data {
                            if ov.script_public_key_address.to_string() == w_cl {
                                user_script_bytes = out.script_public_key.script().to_vec();
                                break;
                            }
                        }
                    }
                }
                if !user_script_bytes.is_empty() {
                    if let Some(verbose) = &full_acc_block.verbose_data {
                        for blue_hash in &verbose.merge_set_blues_hashes {
                            if let Ok(blue_block) = rpc_cl.get_block(*blue_hash, true).await {
                                if let Some(m_tx0) = blue_block.transactions.first() {
                                    if let Some(pos) = m_tx0
                                        .payload
                                        .windows(user_script_bytes.len())
                                        .position(|w| w == user_script_bytes.as_slice())
                                    {
                                        actual_mined_blocks.push(blue_hash.to_string());
                                        block_time_ms = blue_block.header.timestamp;
                                        if extracted_nonce.is_empty() {
                                            extracted_nonce = blue_block.header.nonce.to_string();
                                            let extra_data =
                                                &m_tx0.payload[pos + user_script_bytes.len()..];
                                            let decoded_worker: String = extra_data
                                                .iter()
                                                .filter(|&&c| c >= 32 && c <= 126)
                                                .map(|&c| c as char)
                                                .collect();
                                            extracted_worker = if !decoded_worker.trim().is_empty()
                                            {
                                                decoded_worker.trim().to_string()
                                            } else {
                                                "Standard Miner".to_string()
                                            };
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    (
        acc_block_hash,
        actual_mined_blocks,
        extracted_nonce,
        extracted_worker,
        block_time_ms,
    )
}

fn spawn_memory_cleaner(ctx: AppContext, token: CancellationToken) {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = token.cancelled() => { break; }
                _ = tokio::time::sleep(Duration::from_secs(3600)) => {
                    ctx.memory.clear();
                    info!("[MEMORY CLEANER] Purged AI context history.");
                }
            }
        }
    });
}
