use crate::domain::models::AppContext;
use crate::network::stats_use_cases::GetMarketStatsUseCase;
use crate::network::stats_use_cases::NetworkStatsUseCase;
use crate::presentation::telegram::formatting::kaspa::KaspaFormatter;
use crate::presentation::telegram::menus::{Language, TelegramMenus, user_language};
use kaspa_rpc_core::api::rpc::RpcApi;
use std::sync::Arc;
use teloxide::prelude::*;

fn expected_bps_for_network(network_name: &str) -> Option<f64> {
    let network_name = network_name.to_ascii_lowercase();
    if network_name.contains("mainnet") {
        Some(10.0)
    } else if network_name.contains("testnet") {
        Some(1.0)
    } else {
        None
    }
}

fn format_expected_bps(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:.1}"))
        .unwrap_or_else(|| "Unavailable".to_string())
}

/// Never display endpoint credentials, paths or query parameters from PNN URL.
fn display_pnn_endpoint(raw: Option<&str>) -> String {
    raw.and_then(|raw| reqwest::Url::parse(raw).ok())
        .and_then(|url| {
            let host = url.host_str()?;
            let port = url.port().map(|n| format!(":{n}")).unwrap_or_default();
            Some(format!("{}://{}{}", url.scheme(), host, port))
        })
        .unwrap_or_else(|| "Unavailable".to_string())
}

fn ui_markup(lang: Language, refresh: &str, back: &str) -> teloxide::types::InlineKeyboardMarkup {
    let mut buttons = TelegramMenus::navigation_markup(lang, refresh, back);
    if refresh == "refresh_network" {
        // Network source and health are visible immediately when opened.
        buttons.inline_keyboard.insert(
            0,
            vec![
                teloxide::types::InlineKeyboardButton::callback(
                    lang.label("📊 BlockDAG", "📊 BlockDAG"),
                    "cmd_dag",
                ),
                teloxide::types::InlineKeyboardButton::callback(
                    lang.label("⛽ Fees", "⛽ الرسوم"),
                    "cmd_fees",
                ),
                teloxide::types::InlineKeyboardButton::callback(
                    lang.label("🪙 Supply", "🪙 المعروض"),
                    "cmd_supply",
                ),
            ],
        );
    }
    buttons
}

async fn render_data(
    bot: &Bot,
    msg: &Message,
    target: Option<teloxide::types::MessageId>,
    text: String,
    markup: teloxide::types::InlineKeyboardMarkup,
) {
    crate::utils::send_reply_or_edit_log(bot, msg.chat.id, msg.id, target, text, Some(markup))
        .await;
}
async fn data_language(ctx: &AppContext, msg: &Message) -> Language {
    user_language(
        &ctx.pool,
        msg.from
            .as_ref()
            .map(|v| v.id.0 as i64)
            .unwrap_or(msg.chat.id.0),
    )
    .await
}
async fn sample_bps(ctx: &AppContext, block_count: u64) -> Option<f64> {
    let now = std::time::Instant::now();
    let mut sample = ctx.bps_last_sample.lock().await;
    if let Some((at, prev)) = *sample {
        let elapsed = now.duration_since(at).as_secs_f64();
        if elapsed >= 4.0 {
            *sample = Some((now, block_count));
            return Some((block_count.saturating_sub(prev)) as f64 / elapsed);
        }
    } else {
        *sample = Some((now, block_count));
    }
    None
}

pub async fn handle_network_overview(
    bot: Bot,
    msg: Message,
    app_context: Arc<AppContext>,
    network_stats: Arc<NetworkStatsUseCase>,
    target: Option<teloxide::types::MessageId>,
) -> anyhow::Result<()> {
    let lang = data_language(&app_context, &msg).await;
    let info = app_context.rpc.get_server_info().await.ok();
    let network_name = info
        .as_ref()
        .map(|i| i.network_id.to_string())
        .unwrap_or_else(|| "unknown".to_owned());
    let endpoint = display_pnn_endpoint(app_context.rpc.url().as_deref());
    let mut lines = vec![
        lang.label("🌐 <b>Kaspa Network</b>", "🌐 <b>شبكة كاسبا</b>")
            .to_owned(),
        lang.label("Public PNN (wRPC)", "عقدة PNN العامة (wRPC)")
            .to_owned(),
        format!(
            "{}: <code>{}</code>",
            lang.label("Source", "المصدر"),
            crate::utils::html_escape(&endpoint)
        ),
        format!(
            "{}: <code>{}</code>",
            lang.label("Network", "الشبكة"),
            crate::utils::html_escape(&network_name)
        ),
    ];
    if let Some(ref info) = info {
        lines.push(format!(
            "{}: <code>{}</code>",
            lang.label("Core", "الإصدار"),
            crate::utils::html_escape(&info.server_version.to_string())
        ));
        lines.push(format!(
            "{}: {}",
            lang.label("Sync", "المزامنة"),
            if info.is_synced {
                lang.label("✅ Synced", "✅ متزامنة")
            } else {
                lang.label("⚠️ Syncing", "⚠️ قيد المزامنة")
            }
        ));
    } else {
        lines.push(
            lang.label("⚠️ PNN data unavailable", "⚠️ تعذر جلب معلومات PNN")
                .to_owned(),
        );
    }
    if let Ok((online, peers, hashrate)) = network_stats.get_network_overview().await {
        lines.push(format!(
            "{}: {}",
            lang.label("Status", "الحالة"),
            if online {
                "🟢 Online"
            } else {
                "🔴 Offline"
            }
        ));
        lines.push(format!(
            "{}: <code>{peers}</code>",
            lang.label("Peers", "العقد المتصلة")
        ));
        lines.push(format!(
            "{}: <code>{}</code>",
            lang.label("Global hashrate", "هاش الشبكة"),
            KaspaFormatter::format_hashrate(hashrate)
        ));
    }
    if let Ok(dag) = app_context.rpc.get_block_dag_info().await {
        lines.push(format!(
            "{}: <code>{}</code>",
            lang.label("Tips", "أطراف DAG"),
            dag.tip_hashes.len()
        ));
        let bps = sample_bps(&app_context, dag.block_count).await;
        lines.push(format!(
            "{}: <code>{}</code>",
            lang.label("Observed BPS", "الكتل المرصودة/ث"),
            bps.map(|v| format!("{v:.2}"))
                .unwrap_or_else(|| lang.label("Collecting", "قيد القياس").to_owned())
        ));
    }
    lines.push(format!(
        "{}: <code>{}</code>",
        lang.label("Target BPS", "المعدل المستهدف"),
        format_expected_bps(expected_bps_for_network(&network_name))
    ));
    lines.push(format!(
        "⏱️ <code>{}</code>",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    ));
    render_data(
        &bot,
        &msg,
        target,
        lines.join("\n"),
        ui_markup(lang, "refresh_network", "ui:home"),
    )
    .await;
    Ok(())
}

pub async fn handle_dag(
    bot: Bot,
    msg: Message,
    app_context: Arc<AppContext>,
    dag_uc: Arc<crate::network::analyze_dag::AnalyzeDagUseCase>,
    target: Option<teloxide::types::MessageId>,
) -> anyhow::Result<()> {
    let lang = data_language(&app_context, &msg).await;
    if let Ok(info) = app_context.rpc.get_block_dag_info().await {
        let bps = sample_bps(&app_context, info.block_count).await;
        let mut lines = vec![
            lang.label("📊 <b>BlockDAG</b>", "📊 <b>الكتل BlockDAG</b>")
                .to_owned(),
            format!(
                "{}: <code>{}</code>",
                lang.label("Blocks", "الكتل"),
                info.block_count
            ),
            format!(
                "{}: <code>{}</code>",
                lang.label("Headers", "الترويسات"),
                info.header_count
            ),
            format!(
                "{}: <code>{}</code>",
                lang.label("Difficulty", "الصعوبة"),
                KaspaFormatter::format_difficulty(info.difficulty)
            ),
            format!(
                "{}: <code>{}...</code>",
                lang.label("Pruning point", "نقطة التقليم"),
                info.pruning_point_hash
                    .to_string()
                    .chars()
                    .take(8)
                    .collect::<String>()
            ),
        ];
        if let Some(block) = dag_uc
            .get_pruning_block(&info.pruning_point_hash.to_string())
            .await
        {
            lines.push(format!(
                "{}: <code>{}</code>",
                lang.label("Pruning time", "وقت التقليم"),
                format_kaspa_timestamp(block.timestamp)
            ));
        }
        lines.push(format!(
            "{}: <code>{}</code>",
            lang.label("Observed BPS", "الكتل المرصودة/ث"),
            bps.map(|v| format!("{v:.2}"))
                .unwrap_or_else(|| lang.label("Collecting", "قيد القياس").to_owned())
        ));
        lines.push(format!(
            "⏱️ <code>{}</code>",
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
        ));
        render_data(
            &bot,
            &msg,
            target,
            lines.join("\n"),
            ui_markup(lang, "refresh_dag", "cmd_network"),
        )
        .await;
    } else {
        render_data(
            &bot,
            &msg,
            target,
            lang.label(
                "⚠️ Network temporarily unavailable",
                "⚠️ الشبكة غير متاحة مؤقتًا",
            )
            .to_owned(),
            ui_markup(lang, "refresh_dag", "cmd_network"),
        )
        .await;
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

// Missing/malformed provider values are not estimates; never synthesize live fees.
fn required_fee_rate(bucket: Option<&serde_json::Value>, name: &str) -> Result<f64, String> {
    bucket
        .and_then(|bucket| bucket.get("feerate"))
        .and_then(serde_json::Value::as_f64)
        .filter(|rate| rate.is_finite() && *rate > 0.0)
        .ok_or_else(|| format!("Fee estimate {name} must contain a finite positive feerate"))
}

fn fee_estimate_message(json: &serde_json::Value, timestamp: &str) -> Result<String, String> {
    let priority = required_fee_rate(json.get("priorityBucket"), "priorityBucket")?;
    let first_bucket = |key| {
        json.get(key)
            .and_then(serde_json::Value::as_array)
            .and_then(|buckets| buckets.first())
    };
    let normal = required_fee_rate(first_bucket("normalBuckets"), "normalBuckets[0]")?;
    let low = required_fee_rate(first_bucket("lowBuckets"), "lowBuckets[0]")?;

    let text = format!(
        "⛽ <b>Network Fee Market</b>\n\
         ━━━━━━━━━━━━━━━━━━\n\
         🚀 <b>Priority:</b> <code>{:.2} sompi/gram</code>\n\
         ⚡ <b>Normal:</b> <code>{:.2} sompi/gram</code>\n\
         🐢 <b>Low:</b> <code>{:.2} sompi/gram</code>\n\n\
         <i>* Standard transaction size is ~3000 mass.</i>\n\n\
         ⏱️ <code>{}</code>",
        priority, normal, low, timestamp
    );

    Ok(text)
}

pub async fn handle_fees(
    bot: Bot,
    msg: Message,
    app_context: Arc<AppContext>,
    target: Option<teloxide::types::MessageId>,
) -> anyhow::Result<()> {
    let lang = data_language(&app_context, &msg).await;
    let estimate = fetch_fee_estimate().await.and_then(|json| {
        fee_estimate_message(
            &json,
            &chrono::Utc::now()
                .format("%Y-%m-%d %H:%M:%S UTC")
                .to_string(),
        )
    });
    let text = match estimate {
        Ok(data) => data,
        Err(error) => {
            tracing::warn!(error=%crate::utils::sanitize_for_log(&error),"PNN fee endpoint unavailable");
            lang.label(
                "⚠️ Fee estimates unavailable",
                "⚠️ تقديرات الرسوم غير متاحة",
            )
            .to_string()
        }
    };
    render_data(
        &bot,
        &msg,
        target,
        text,
        ui_markup(lang, "refresh_fees", "cmd_network"),
    )
    .await;
    Ok(())
}

pub async fn handle_supply(
    bot: Bot,
    msg: Message,
    app_context: Arc<AppContext>,
    target: Option<teloxide::types::MessageId>,
) -> anyhow::Result<()> {
    let lang = data_language(&app_context, &msg).await;
    let text = match app_context.rpc.get_coin_supply().await {
        Ok(supply) => {
            let circ = supply.circulating_sompi as f64 / 1e8;
            let max = supply.max_sompi as f64 / 1e8;
            format!(
                "{}\n{}: <code>{:.0} KAS</code>\n{}: <code>{:.0} KAS</code>\n{}: <code>{:.2}%</code>",
                lang.label("🪙 <b>Coin Supply</b>", "🪙 <b>معروض كاسبا</b>"),
                lang.label("Circulating", "المتداول"),
                circ,
                lang.label("Max Supply", "الحد الأقصى"),
                max,
                lang.label("Minted", "نسبة الإصدار"),
                if max > 0.0 { 100.0 * circ / max } else { 0.0 }
            )
        }
        Err(_) => lang
            .label("⚠️ Supply unavailable", "⚠️ بيانات المعروض غير متاحة")
            .to_owned(),
    };
    render_data(
        &bot,
        &msg,
        target,
        text,
        ui_markup(lang, "refresh_supply", "cmd_network"),
    )
    .await;
    Ok(())
}

pub async fn handle_market_data(
    bot: Bot,
    msg: Message,
    app_context: Arc<AppContext>,
    market_stats: Arc<GetMarketStatsUseCase>,
    target: Option<teloxide::types::MessageId>,
) -> anyhow::Result<()> {
    let lang = data_language(&app_context, &msg).await;
    // Separate financial API data completely from PNN node health and BPS.
    let text = match market_stats.market_only().await {
        Ok((price, mcap)) => {
            let mut lines = vec![
                lang.label("💹 <b>Kaspa Market</b>", "💹 <b>سوق كاسبا</b>")
                    .to_owned(),
                format!(
                    "{}: <code>${price:.4} USD</code>",
                    lang.label("KAS price", "سعر KAS")
                ),
                format!(
                    "{}: <code>${}</code>",
                    lang.label("Market cap", "القيمة السوقية"),
                    format_number(mcap)
                ),
            ];
            lines.push(format!(
                "⏱️ <code>{}</code>",
                chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
            ));
            lines.join("\n")
        }
        Err(error) => {
            tracing::warn!(error=%crate::utils::sanitize_for_log(&error.to_string()),"Market provider unavailable");
            lang.label(
                "⚠️ Market data temporarily unavailable",
                "⚠️ بيانات السوق غير متاحة مؤقتًا",
            )
            .to_owned()
        }
    };
    render_data(
        &bot,
        &msg,
        target,
        text,
        ui_markup(lang, "refresh_market", "ui:home"),
    )
    .await;
    Ok(())
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

#[cfg(test)]
mod fee_estimate_regression_tests {
    use super::fee_estimate_message;
    use serde_json::{Value, json};

    const TIME: &str = "2026-09-14 00:00:00 UTC";
    const POINTERS: [&str; 3] = [
        "/priorityBucket/feerate",
        "/normalBuckets/0/feerate",
        "/lowBuckets/0/feerate",
    ];

    fn valid_estimate() -> Value {
        json!({
            "priorityBucket": {"feerate": 4.25},
            "normalBuckets": [{"feerate": 2.5}],
            "lowBuckets": [{"feerate": 1.25}],
        })
    }

    #[test]
    fn missing_estimate_is_not_rendered_as_live_fees() {
        for input in [json!({}), Value::Null, json!([]), json!(true)] {
            let result = fee_estimate_message(&input, TIME);
            assert!(
                result.is_err(),
                "missing provider data rendered as live fees: {result:?}"
            );
        }
    }

    #[test]
    fn each_missing_bucket_or_rate_is_rejected() {
        for key in ["priorityBucket", "normalBuckets", "lowBuckets"] {
            let mut input = valid_estimate();
            input.as_object_mut().unwrap().remove(key);
            assert!(fee_estimate_message(&input, TIME).is_err(), "missing {key}");
        }
        for pointer in POINTERS {
            let mut input = valid_estimate();
            *input.pointer_mut(pointer).unwrap() = Value::Null;
            assert!(
                fee_estimate_message(&input, TIME).is_err(),
                "missing rate {pointer}"
            );
        }
        for key in ["normalBuckets", "lowBuckets"] {
            let mut input = valid_estimate();
            input[key] = json!([]);
            assert!(fee_estimate_message(&input, TIME).is_err(), "empty {key}");
        }
    }

    #[test]
    fn wrong_rate_types_are_rejected_not_defaulted() {
        for pointer in POINTERS {
            for value in [
                json!("2.0"),
                json!("NaN"),
                json!(true),
                json!([]),
                json!({}),
            ] {
                let mut input = valid_estimate();
                *input.pointer_mut(pointer).unwrap() = value;
                assert!(
                    fee_estimate_message(&input, TIME).is_err(),
                    "invalid {pointer}: {input}"
                );
            }
        }
    }

    #[test]
    fn non_positive_rates_are_rejected() {
        for pointer in POINTERS {
            for value in [-1.0, 0.0, -0.0] {
                let mut input = valid_estimate();
                *input.pointer_mut(pointer).unwrap() = json!(value);
                assert!(
                    fee_estimate_message(&input, TIME).is_err(),
                    "invalid {pointer}: {value}"
                );
            }
        }
    }

    #[test]
    fn valid_provider_values_keep_existing_message_format() {
        let rendered = fee_estimate_message(&valid_estimate(), TIME).unwrap();
        assert_eq!(
            rendered,
            concat!(
                "⛽ <b>Network Fee Market</b>\n",
                "━━━━━━━━━━━━━━━━━━\n",
                "🚀 <b>Priority:</b> <code>4.25 sompi/gram</code>\n",
                "⚡ <b>Normal:</b> <code>2.50 sompi/gram</code>\n",
                "🐢 <b>Low:</b> <code>1.25 sompi/gram</code>\n\n",
                "<i>* Standard transaction size is ~3000 mass.</i>\n\n",
                "⏱️ <code>2026-09-14 00:00:00 UTC</code>"
            )
        );
    }

    #[test]
    fn additional_buckets_and_provider_metadata_do_not_change_first_values() {
        let mut input = valid_estimate();
        input["normalBuckets"]
            .as_array_mut()
            .unwrap()
            .push(json!({"feerate": 1.75}));
        input["lowBuckets"]
            .as_array_mut()
            .unwrap()
            .push(json!({"feerate": 1.0}));
        input["priorityBucket"]["estimatedSeconds"] = json!(0.5);
        input["providerMetadata"] = json!({"synthetic": true});
        assert_eq!(
            fee_estimate_message(&input, TIME),
            fee_estimate_message(&valid_estimate(), TIME)
        );
    }

    #[test]
    fn bucket_lists_must_be_arrays_not_numeric_key_objects() {
        for key in ["normalBuckets", "lowBuckets"] {
            let mut input = valid_estimate();
            input[key] = json!({"0": {"feerate": 2.0}});
            assert!(fee_estimate_message(&input, TIME).is_err());
        }
    }

    #[test]
    fn equal_positive_buckets_are_preserved() {
        let mut input = valid_estimate();
        for pointer in POINTERS {
            *input.pointer_mut(pointer).unwrap() = json!(1.0);
        }
        assert_eq!(
            fee_estimate_message(&input, TIME)
                .unwrap()
                .matches("1.00 sompi/gram")
                .count(),
            3
        );
    }
}

#[cfg(test)]
mod pnn_ui_contract_tests {
    use super::display_pnn_endpoint;
    #[test]
    fn pnn_url_shows_only_host_and_explicit_port() {
        let source = "wss://user:secret@pnn.example.net:17110/private?token=do-not-show#fragment";
        let visible = display_pnn_endpoint(Some(source));
        assert_eq!(visible, "wss://pnn.example.net:17110");
        assert!(!visible.contains("secret"));
        assert!(!visible.contains("token"));
        assert!(!visible.contains("private"));
    }
    #[test]
    fn unknown_pnn_host_is_not_fabricated() {
        assert_eq!(display_pnn_endpoint(None), "Unavailable");
        assert_eq!(display_pnn_endpoint(Some("invalid")), "Unavailable");
    }
}
