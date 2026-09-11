use std::sync::atomic::{AtomicU64, Ordering};

pub static ALERTS_DELIVERED: AtomicU64 = AtomicU64::new(0);
pub static ALERTS_SUPPRESSED: AtomicU64 = AtomicU64::new(0);
pub static ADMIN_ACTIONS_CONFIRMED: AtomicU64 = AtomicU64::new(0);
pub static TELEGRAM_SEND_FAILURES: AtomicU64 = AtomicU64::new(0);
pub static RPC_TIMEOUTS: AtomicU64 = AtomicU64::new(0);
pub static DB_ERRORS: AtomicU64 = AtomicU64::new(0);
pub static LAST_ALERT_DELIVERED_TS: AtomicU64 = AtomicU64::new(0);
pub static LAST_ALERT_DETECTED_TS: AtomicU64 = AtomicU64::new(0);
pub static LAST_UTXO_SCAN_TS: AtomicU64 = AtomicU64::new(0);

pub fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0)
}

pub fn inc_alerts_delivered() {
    ALERTS_DELIVERED.fetch_add(1, Ordering::Relaxed);
    LAST_ALERT_DELIVERED_TS.store(now_unix_secs(), Ordering::Relaxed);
}

pub fn inc_alerts_suppressed() {
    ALERTS_SUPPRESSED.fetch_add(1, Ordering::Relaxed);
}

pub fn inc_admin_actions_confirmed() {
    ADMIN_ACTIONS_CONFIRMED.fetch_add(1, Ordering::Relaxed);
}

pub fn inc_telegram_send_failures() {
    TELEGRAM_SEND_FAILURES.fetch_add(1, Ordering::Relaxed);
}

pub fn inc_rpc_timeouts() {
    RPC_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
}

pub fn inc_db_errors() {
    DB_ERRORS.fetch_add(1, Ordering::Relaxed);
}

pub fn render_metrics() -> String {
    let mut rendered = format!(
        concat!(
            "# HELP kaspa_pulse_alerts_delivered_total Total delivered mining alerts.\n",
            "# TYPE kaspa_pulse_alerts_delivered_total counter\n",
            "kaspa_pulse_alerts_delivered_total {}\n",
            "# HELP kaspa_pulse_alerts_suppressed_total Total suppressed mining alerts while alert delivery was disabled.\n",
            "# TYPE kaspa_pulse_alerts_suppressed_total counter\n",
            "kaspa_pulse_alerts_suppressed_total {}\n",
            "# HELP kaspa_pulse_admin_actions_confirmed_total Total confirmed sensitive admin actions.\n",
            "# TYPE kaspa_pulse_admin_actions_confirmed_total counter\n",
            "kaspa_pulse_admin_actions_confirmed_total {}\n",
            "# HELP kaspa_pulse_telegram_send_failures_total Telegram send failures.\n",
            "# TYPE kaspa_pulse_telegram_send_failures_total counter\n",
            "kaspa_pulse_telegram_send_failures_total {}\n",
            "# HELP kaspa_pulse_rpc_timeouts_total RPC timeout count.\n",
            "# TYPE kaspa_pulse_rpc_timeouts_total counter\n",
            "kaspa_pulse_rpc_timeouts_total {}\n",
            "# HELP kaspa_pulse_db_errors_total Database error count.\n",
            "# TYPE kaspa_pulse_db_errors_total counter\n",
            "kaspa_pulse_db_errors_total {}\n",
            "# HELP kaspa_pulse_last_alert_delivered_timestamp Last delivered alert unix timestamp.\n",
            "# TYPE kaspa_pulse_last_alert_delivered_timestamp gauge\n",
            "kaspa_pulse_last_alert_delivered_timestamp {}\n",
            "# HELP kaspa_pulse_last_alert_detected_timestamp Last detected alert unix timestamp.\n",
            "# TYPE kaspa_pulse_last_alert_detected_timestamp gauge\n",
            "kaspa_pulse_last_alert_detected_timestamp {}\n",
            "# HELP kaspa_pulse_last_utxo_scan_timestamp Last successful UTXO scan unix timestamp.\n",
            "# TYPE kaspa_pulse_last_utxo_scan_timestamp gauge\n",
            "kaspa_pulse_last_utxo_scan_timestamp {}\n"
        ),
        ALERTS_DELIVERED.load(Ordering::Relaxed),
        ALERTS_SUPPRESSED.load(Ordering::Relaxed),
        ADMIN_ACTIONS_CONFIRMED.load(Ordering::Relaxed),
        TELEGRAM_SEND_FAILURES.load(Ordering::Relaxed),
        RPC_TIMEOUTS.load(Ordering::Relaxed),
        DB_ERRORS.load(Ordering::Relaxed),
        LAST_ALERT_DELIVERED_TS.load(Ordering::Relaxed),
        LAST_ALERT_DETECTED_TS.load(Ordering::Relaxed),
        LAST_UTXO_SCAN_TS.load(Ordering::Relaxed),
    );

    rendered.push_str(&crate::build_info::render_prometheus());
    rendered.push_str(&crate::infrastructure::observability::snapshot().render_prometheus());
    rendered
}

pub fn mark_alert_detected() {
    LAST_ALERT_DETECTED_TS.store(now_unix_secs(), Ordering::Relaxed);
}

pub fn mark_utxo_scan() {
    LAST_UTXO_SCAN_TS.store(now_unix_secs(), Ordering::Relaxed);
}
