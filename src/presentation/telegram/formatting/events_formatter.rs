use crate::domain::models::LiveBlockEvent;
use crate::presentation::telegram::menus::Language;
use crate::utils::{format_hash, format_short_wallet};
use chrono::{TimeZone, Utc};

fn coinbase_has_unresolved_block_details(
    is_coinbase: bool,
    accepting_hash: &str,
    mined_hash: Option<&str>,
) -> bool {
    is_coinbase && accepting_hash.is_empty() && mined_hash.is_none()
}

pub fn formatted_usd(value_kas: f64, price_usd: Option<f64>) -> String {
    match price_usd {
        Some(price) if price.is_finite() && price > 0.0 && value_kas.is_finite() => {
            format!("{}{:0.2}", '$', value_kas * price)
        }
        _ => "USD unavailable".to_string(),
    }
}

fn formatted_usd_lang(value_kas: f64, price_usd: Option<f64>, lang: Language) -> String {
    let value = formatted_usd(value_kas, price_usd);
    if lang == Language::Ar && value == "USD unavailable" {
        "الدولار غير متاح".to_string()
    } else {
        value
    }
}

pub fn format_live_event(event: &LiveBlockEvent, price_usd: Option<f64>) -> String {
    format_live_event_lang(event, price_usd, Language::En)
}

pub fn format_live_event_lang(
    event: &LiveBlockEvent,
    price_usd: Option<f64>,
    lang: Language,
) -> String {
    let time_str = if event.block_time_ms > 0 {
        if let chrono::LocalResult::Single(dt) =
            Utc.timestamp_millis_opt(event.block_time_ms as i64)
        {
            dt.format("%Y-%m-%d %H:%M:%S.%3f UTC").to_string()
        } else {
            Utc::now().format("%Y-%m-%d %H:%M:%S.%3f UTC").to_string()
        }
    } else {
        Utc::now().format("%Y-%m-%d %H:%M:%S.%3f UTC").to_string()
    };

    let degraded_coinbase = coinbase_has_unresolved_block_details(
        event.is_coinbase,
        &event.acc_block_hash,
        event.mined_block_hash.as_deref(),
    );
    let header_emoji = if event.is_coinbase {
        lang.label(
            "⚡ <b>Kaspa Mining Reward!</b>",
            "⚡ <b>مكافأة تعدين كاسبا!</b>",
        )
    } else {
        lang.label("💸 <b>Incoming Transfer!</b>", "💸 <b>تحويل وارد!</b>")
    };
    let acc_block_str = if event.acc_block_hash.is_empty() {
        format!(
            "<code>{}</code>",
            lang.label("Not available from RPC", "غير متاح من RPC")
        )
    } else {
        format_hash(&event.acc_block_hash, "blocks")
    };
    let mined_block_str = match &event.mined_block_hash {
        Some(hash) => format_hash(hash, "blocks"),
        None => {
            if event.is_coinbase {
                format!("<code>{}</code>", lang.label("Unresolved", "لم يُتحقق منه"))
            } else {
                format!("<code>{}</code>", lang.label("N/A", "غير متاح"))
            }
        }
    };

    let time_label = if event.block_time_ms > 0 {
        lang.label("Time", "الوقت")
    } else {
        lang.label("Detected", "وقت الاكتشاف")
    };
    let mut final_msg = format!(
        "{}\n━━━━━━━━━━━━━━━━━━\n<b>{}:</b> <code>{}</code>\n<b>{}:</b> {}\n<b>{}:</b> <code>+{:.2} KAS ({})</code>\n<b>{}:</b> <code>{:.2} KAS ({})</code>\n<blockquote expandable>",
        header_emoji,
        time_label,
        time_str,
        lang.label("Wallet", "المحفظة"),
        format_short_wallet(&event.wallet_address),
        lang.label("Amount", "المبلغ"),
        event.amount_kas,
        formatted_usd_lang(event.amount_kas, price_usd, lang),
        lang.label("Balance", "الرصيد"),
        event.live_balance_kas,
        formatted_usd_lang(event.live_balance_kas, price_usd, lang)
    );

    final_msg.push_str(&format!(
        "<b>{}:</b> {}\n",
        lang.label("TXID", "معرف المعاملة"),
        format_hash(&event.tx_id, "transactions")
    ));
    if event.is_coinbase {
        if degraded_coinbase {
            // Do not claim a mined/accepting hash unless independently verified.
            // The confirmed Coinbase UTXO remains a valid mining payout alert.
            final_msg.push_str(&format!(
                "<b>{}:</b> <code>{}</code>\n",
                lang.label("Block Details", "تفاصيل الكتلة"),
                lang.label("Not verified", "لم يُتحقق منها")
            ));
        } else {
            final_msg.push_str(&format!(
                "<b>{}:</b> {}\n<b>{}:</b> {}\n",
                lang.label("Mined Block", "الكتلة المعدّنة"),
                mined_block_str,
                lang.label("Accepting Block", "الكتلة المُعتمِدة"),
                acc_block_str
            ));
        }
        if let Some(worker) = &event.extracted_worker {
            let safe_worker = teloxide::utils::html::escape(worker);
            final_msg.push_str(&format!(
                "<b>{}:</b> <code>{}</code>\n",
                lang.label("Worker", "العامل"),
                safe_worker
            ));
        }
    } else {
        final_msg.push_str(&format!(
            "<b>{}:</b> {}\n",
            lang.label("Accepting Block", "الكتلة المُعتمِدة"),
            acc_block_str
        ));
    }
    final_msg.push_str(&format!(
        "<b>{}:</b> <code>{}</code>\n</blockquote>",
        lang.label("DAA Score", "درجة DAA"),
        event.daa_score
    ));
    final_msg
}

#[cfg(test)]
mod coinbase_fallback_formatter_tests {
    use super::coinbase_has_unresolved_block_details;
    #[test]
    fn payout_without_dag_proof_keeps_reward_title_and_never_fakes_block() {
        let event = crate::domain::models::LiveBlockEvent {
            is_coinbase: true,
            wallet_address: "kaspa:qtestwallet".to_string(),
            amount_kas: 2.06,
            live_balance_kas: 1948.09,
            source_outpoint: "proof:0".to_string(),
            alert_key: "txproof".to_string(),
            tx_id: "abc012".to_string(),
            block_time_ms: 0,
            acc_block_hash: String::new(),
            mined_block_hash: None,
            extracted_worker: None,
            daa_score: 581_000_000,
        };
        let result = super::format_live_event(&event, Some(0.033));
        assert!(result.contains("⚡ <b>Kaspa Mining Reward!</b>"));
        assert!(!result.contains("Coinbase Payout Detected"));
        assert!(result.contains("<b>Detected:</b>"));
        assert!(result.contains("<b>Block Details:</b> <code>Not verified</code>"));
        assert!(result.contains("<b>Balance:</b> <code>1948.09 KAS"));
        assert!(!result.contains("<b>Mined Block:</b>"));
        assert!(!result.contains("<b>Accepting Block:</b>"));
        assert!(!result.contains("<b>Worker:</b>"));
    }

    #[test]
    fn fallback_never_claims_a_mined_block_hash() {
        assert!(coinbase_has_unresolved_block_details(true, "", None));
        assert!(!coinbase_has_unresolved_block_details(
            true,
            "verified-accepting-hash",
            None
        ));
        assert!(!coinbase_has_unresolved_block_details(
            true,
            "",
            Some("verified-mined-hash")
        ));
        assert!(!coinbase_has_unresolved_block_details(false, "", None));
    }
}

#[cfg(test)]
mod usd_reward_format_tests {
    use super::formatted_usd;
    #[test]
    fn two_decimal_usd_only_for_valid_price() {
        assert_eq!(formatted_usd(10.0, Some(0.03254682)), "$0.33");
        assert_eq!(formatted_usd(10.0, Some(0.0)), "USD unavailable");
        assert_eq!(formatted_usd(10.0, None), "USD unavailable");
        assert_eq!(formatted_usd(10.0, Some(f64::NAN)), "USD unavailable");
    }
}

#[cfg(test)]
mod op010_mobile_compact_time_tests {
    use super::{Language, LiveBlockEvent, format_live_event_lang};

    #[test]
    fn compact_time_label_preserves_original_block_timestamp_for_english_and_arabic() {
        let event = LiveBlockEvent {
            is_coinbase: true,
            wallet_address: "kaspa:op010test".to_string(),
            amount_kas: 1.94,
            live_balance_kas: 5065.54,
            source_outpoint: "op010test:0".to_string(),
            alert_key: "op010test".to_string(),
            tx_id: "a".repeat(64),
            block_time_ms: 1793826978812,
            acc_block_hash: "b".repeat(64),
            mined_block_hash: Some("c".repeat(64)),
            extracted_worker: None,
            daa_score: 584092789,
        };
        let en = format_live_event_lang(&event, Some(0.03), Language::En);
        let ar = format_live_event_lang(&event, Some(0.03), Language::Ar);
        assert!(en.contains("<b>Time:</b> <code>2026-11-04 21:16:18.812 UTC</code>"));
        assert!(!en.contains("Block Time"));
        assert!(ar.contains("<b>الوقت:</b> <code>2026-11-04 21:16:18.812 UTC</code>"));
        assert!(!ar.contains("وقت الكتلة"));
        assert!(en.contains("<b>Mined Block:</b>"));
        assert!(ar.contains("<b>الكتلة المعدّنة:</b>"));
    }
}
