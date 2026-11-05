use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};

use crate::infrastructure::node::subscription::{MonitorTrigger, MonitoringMode};

static QUEUE_PENDING: AtomicI64 = AtomicI64::new(0);
static QUEUE_PROCESSING: AtomicI64 = AtomicI64::new(0);
static QUEUE_FAILED: AtomicI64 = AtomicI64::new(0);
static QUEUE_FAILED_RECENT: AtomicI64 = AtomicI64::new(0);
static OLDEST_QUEUE_AGE_SECONDS: AtomicU64 = AtomicU64::new(0);

static DELIVERY_LATENCY_MS_LAST: AtomicU64 = AtomicU64::new(0);
static DELIVERY_LATENCY_MS_SUM: AtomicU64 = AtomicU64::new(0);
static DELIVERY_LATENCY_SAMPLES: AtomicU64 = AtomicU64::new(0);

static OUTBOX_FAILURES_TOTAL: AtomicU64 = AtomicU64::new(0);
static NODE_RECONNECTS_TOTAL: AtomicU64 = AtomicU64::new(0);
static SUBSCRIPTION_REREGISTRATIONS_TOTAL: AtomicU64 = AtomicU64::new(0);
static SUBSCRIPTION_REGISTRATION_FAILURES_TOTAL: AtomicU64 = AtomicU64::new(0);
static SUBSCRIPTION_RUNTIME_RESTARTS_TOTAL: AtomicU64 = AtomicU64::new(0);
static CALLBACKS_REJECTED_INFLIGHT_TOTAL: AtomicU64 = AtomicU64::new(0);

static STARTUP_SCANS_TOTAL: AtomicU64 = AtomicU64::new(0);
static SUBSCRIPTION_SCANS_TOTAL: AtomicU64 = AtomicU64::new(0);
static POLL_FALLBACK_SCANS_TOTAL: AtomicU64 = AtomicU64::new(0);
static RECONCILIATION_SCANS_TOTAL: AtomicU64 = AtomicU64::new(0);

static LAST_SUCCESSFUL_SCAN_TS: AtomicU64 = AtomicU64::new(0);
static LAST_TELEGRAM_DELIVERY_TS: AtomicU64 = AtomicU64::new(0);
static LAST_SUBSCRIPTION_EVENT_TS: AtomicU64 = AtomicU64::new(0);

static NODE_CONNECTED: AtomicBool = AtomicBool::new(false);
static SUBSCRIPTION_ACTIVE: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueSnapshot {
    pub pending: i64,
    pub processing: i64,
    /// Total terminal failures retained for historical monitoring.
    pub failed: i64,
    /// Terminal failures updated inside the readiness failure window.
    pub failed_recent: i64,
    pub oldest_age_seconds: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationalSnapshot {
    pub queue: QueueSnapshot,
    pub delivery_latency_ms_last: u64,
    pub delivery_latency_ms_sum: u64,
    pub delivery_latency_samples: u64,
    pub outbox_failures_total: u64,
    pub node_reconnects_total: u64,
    pub subscription_reregistrations_total: u64,
    pub subscription_registration_failures_total: u64,
    pub subscription_runtime_restarts_total: u64,
    pub callbacks_rejected_inflight_total: u64,
    pub startup_scans_total: u64,
    pub subscription_scans_total: u64,
    pub poll_fallback_scans_total: u64,
    pub reconciliation_scans_total: u64,
    pub last_successful_scan_ts: u64,
    pub last_telegram_delivery_ts: u64,
    pub last_subscription_event_ts: u64,
    pub node_connected: bool,
    pub subscription_active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadinessPolicy {
    pub require_node_connection: bool,
    pub require_active_subscription: bool,
    pub max_scan_age_seconds: u64,
    pub max_failed_queue_items: i64,
    pub failed_queue_window_seconds: u64,
}

impl ReadinessPolicy {
    pub fn from_env() -> Self {
        let mode = MonitoringMode::from_env();
        Self {
            require_node_connection: env_bool("READINESS_REQUIRE_NODE", true),
            require_active_subscription: effective_subscription_requirement(
                mode,
                env_bool("READINESS_REQUIRE_SUBSCRIPTION", true),
            ),
            max_scan_age_seconds: env_u64("READINESS_MAX_SCAN_AGE_SECS", 120),
            max_failed_queue_items: env_i64("READINESS_MAX_FAILED_QUEUE_ITEMS", 0),
            failed_queue_window_seconds: env_u64("READINESS_FAILED_QUEUE_WINDOW_SECS", 3_600),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadinessState {
    Ready,
    Degraded,
    NotReady,
}

impl ReadinessState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Degraded => "degraded",
            Self::NotReady => "not_ready",
        }
    }
}

pub fn set_queue_snapshot(
    pending: i64,
    processing: i64,
    failed: i64,
    failed_recent: i64,
    oldest_age_seconds: u64,
) {
    QUEUE_PENDING.store(pending.max(0), Ordering::Relaxed);
    QUEUE_PROCESSING.store(processing.max(0), Ordering::Relaxed);
    QUEUE_FAILED.store(failed.max(0), Ordering::Relaxed);
    QUEUE_FAILED_RECENT.store(failed_recent.max(0), Ordering::Relaxed);
    OLDEST_QUEUE_AGE_SECONDS.store(oldest_age_seconds, Ordering::Relaxed);
}

pub fn observe_delivery_latency(milliseconds: u64) {
    DELIVERY_LATENCY_MS_LAST.store(milliseconds, Ordering::Relaxed);
    DELIVERY_LATENCY_MS_SUM.fetch_add(milliseconds, Ordering::Relaxed);
    DELIVERY_LATENCY_SAMPLES.fetch_add(1, Ordering::Relaxed);
}

pub fn increment_outbox_failures() {
    OUTBOX_FAILURES_TOTAL.fetch_add(1, Ordering::Relaxed);
}

pub fn increment_node_reconnects() {
    NODE_RECONNECTS_TOTAL.fetch_add(1, Ordering::Relaxed);
}

pub fn increment_subscription_reregistrations() {
    SUBSCRIPTION_REREGISTRATIONS_TOTAL.fetch_add(1, Ordering::Relaxed);
}

pub fn increment_subscription_registration_failures() {
    SUBSCRIPTION_REGISTRATION_FAILURES_TOTAL.fetch_add(1, Ordering::Relaxed);
}

pub fn increment_subscription_runtime_restarts() {
    SUBSCRIPTION_RUNTIME_RESTARTS_TOTAL.fetch_add(1, Ordering::Relaxed);
}

pub fn increment_callbacks_rejected_inflight() {
    CALLBACKS_REJECTED_INFLIGHT_TOTAL.fetch_add(1, Ordering::Relaxed);
}

pub fn record_scan_trigger(trigger: MonitorTrigger) {
    match trigger {
        MonitorTrigger::Startup => STARTUP_SCANS_TOTAL.fetch_add(1, Ordering::Relaxed),
        MonitorTrigger::SubscriptionNotification => {
            SUBSCRIPTION_SCANS_TOTAL.fetch_add(1, Ordering::Relaxed)
        }
        MonitorTrigger::PollFallback => POLL_FALLBACK_SCANS_TOTAL.fetch_add(1, Ordering::Relaxed),
        MonitorTrigger::PeriodicReconciliation => {
            RECONCILIATION_SCANS_TOTAL.fetch_add(1, Ordering::Relaxed)
        }
    };
}

pub fn mark_successful_scan(unix_timestamp: u64) {
    LAST_SUCCESSFUL_SCAN_TS.store(unix_timestamp, Ordering::Relaxed);
}

pub fn mark_telegram_delivery(unix_timestamp: u64) {
    LAST_TELEGRAM_DELIVERY_TS.store(unix_timestamp, Ordering::Relaxed);
}

pub fn mark_subscription_event(unix_timestamp: u64) {
    LAST_SUBSCRIPTION_EVENT_TS.store(unix_timestamp, Ordering::Relaxed);
}

pub fn set_node_connected(connected: bool) {
    NODE_CONNECTED.store(connected, Ordering::Relaxed);
}

pub fn set_subscription_active(active: bool) {
    SUBSCRIPTION_ACTIVE.store(active, Ordering::Relaxed);
}

pub fn snapshot() -> OperationalSnapshot {
    OperationalSnapshot {
        queue: QueueSnapshot {
            pending: QUEUE_PENDING.load(Ordering::Relaxed),
            processing: QUEUE_PROCESSING.load(Ordering::Relaxed),
            failed: QUEUE_FAILED.load(Ordering::Relaxed),
            failed_recent: QUEUE_FAILED_RECENT.load(Ordering::Relaxed),
            oldest_age_seconds: OLDEST_QUEUE_AGE_SECONDS.load(Ordering::Relaxed),
        },
        delivery_latency_ms_last: DELIVERY_LATENCY_MS_LAST.load(Ordering::Relaxed),
        delivery_latency_ms_sum: DELIVERY_LATENCY_MS_SUM.load(Ordering::Relaxed),
        delivery_latency_samples: DELIVERY_LATENCY_SAMPLES.load(Ordering::Relaxed),
        outbox_failures_total: OUTBOX_FAILURES_TOTAL.load(Ordering::Relaxed),
        node_reconnects_total: NODE_RECONNECTS_TOTAL.load(Ordering::Relaxed),
        subscription_reregistrations_total: SUBSCRIPTION_REREGISTRATIONS_TOTAL
            .load(Ordering::Relaxed),
        subscription_registration_failures_total: SUBSCRIPTION_REGISTRATION_FAILURES_TOTAL
            .load(Ordering::Relaxed),
        subscription_runtime_restarts_total: SUBSCRIPTION_RUNTIME_RESTARTS_TOTAL
            .load(Ordering::Relaxed),
        callbacks_rejected_inflight_total: CALLBACKS_REJECTED_INFLIGHT_TOTAL
            .load(Ordering::Relaxed),
        startup_scans_total: STARTUP_SCANS_TOTAL.load(Ordering::Relaxed),
        subscription_scans_total: SUBSCRIPTION_SCANS_TOTAL.load(Ordering::Relaxed),
        poll_fallback_scans_total: POLL_FALLBACK_SCANS_TOTAL.load(Ordering::Relaxed),
        reconciliation_scans_total: RECONCILIATION_SCANS_TOTAL.load(Ordering::Relaxed),
        last_successful_scan_ts: LAST_SUCCESSFUL_SCAN_TS.load(Ordering::Relaxed),
        last_telegram_delivery_ts: LAST_TELEGRAM_DELIVERY_TS.load(Ordering::Relaxed),
        last_subscription_event_ts: LAST_SUBSCRIPTION_EVENT_TS.load(Ordering::Relaxed),
        node_connected: NODE_CONNECTED.load(Ordering::Relaxed),
        subscription_active: SUBSCRIPTION_ACTIVE.load(Ordering::Relaxed),
    }
}

impl OperationalSnapshot {
    pub fn average_delivery_latency_ms(self) -> u64 {
        self.delivery_latency_ms_sum
            .checked_div(self.delivery_latency_samples)
            .unwrap_or(0)
    }

    pub fn readiness(self, now_ts: u64, policy: ReadinessPolicy) -> ReadinessState {
        if policy.require_node_connection && !self.node_connected {
            return ReadinessState::NotReady;
        }

        if self.last_successful_scan_ts == 0 {
            return ReadinessState::NotReady;
        }

        if now_ts.saturating_sub(self.last_successful_scan_ts) > policy.max_scan_age_seconds {
            return ReadinessState::Degraded;
        }

        if self.queue.failed_recent > policy.max_failed_queue_items
            || (policy.require_active_subscription && !self.subscription_active)
        {
            return ReadinessState::Degraded;
        }

        ReadinessState::Ready
    }

    pub fn render_prometheus(self) -> String {
        format!(
            concat!(
                "# HELP kaspa_pulse_queue_pending Pending Telegram delivery rows.\n",
                "# TYPE kaspa_pulse_queue_pending gauge\n",
                "kaspa_pulse_queue_pending {}\n",
                "# HELP kaspa_pulse_queue_processing Processing Telegram delivery rows.\n",
                "# TYPE kaspa_pulse_queue_processing gauge\n",
                "kaspa_pulse_queue_processing {}\n",
                "# HELP kaspa_pulse_queue_failed Permanently failed Telegram delivery rows.\n",
                "# TYPE kaspa_pulse_queue_failed gauge\n",
                "kaspa_pulse_queue_failed {}\n",
                "# HELP kaspa_pulse_queue_failed_recent Terminal Telegram delivery failures inside the readiness window.\n",
                "# TYPE kaspa_pulse_queue_failed_recent gauge\n",
                "kaspa_pulse_queue_failed_recent {}\n",
                "kaspa_pulse_queue_oldest_age_seconds {}\n",
                "kaspa_pulse_delivery_latency_ms_last {}\n",
                "kaspa_pulse_delivery_latency_ms_average {}\n",
                "kaspa_pulse_delivery_latency_samples_total {}\n",
                "kaspa_pulse_outbox_failures_total {}\n",
                "kaspa_pulse_node_reconnects_total {}\n",
                "kaspa_pulse_subscription_reregistrations_total {}\n",
                "kaspa_pulse_subscription_registration_failures_total {}\n",
                "kaspa_pulse_subscription_runtime_restarts_total {}\n",
                "kaspa_pulse_callbacks_rejected_inflight_total {}\n",
                "kaspa_pulse_scans_startup_total {}\n",
                "kaspa_pulse_scans_subscription_total {}\n",
                "kaspa_pulse_scans_poll_fallback_total {}\n",
                "kaspa_pulse_scans_reconciliation_total {}\n",
                "kaspa_pulse_last_successful_scan_timestamp {}\n",
                "kaspa_pulse_last_telegram_delivery_timestamp {}\n",
                "kaspa_pulse_last_subscription_event_timestamp {}\n",
                "kaspa_pulse_node_connected {}\n",
                "kaspa_pulse_subscription_active {}\n",
            ),
            self.queue.pending,
            self.queue.processing,
            self.queue.failed,
            self.queue.failed_recent,
            self.queue.oldest_age_seconds,
            self.delivery_latency_ms_last,
            self.average_delivery_latency_ms(),
            self.delivery_latency_samples,
            self.outbox_failures_total,
            self.node_reconnects_total,
            self.subscription_reregistrations_total,
            self.subscription_registration_failures_total,
            self.subscription_runtime_restarts_total,
            self.callbacks_rejected_inflight_total,
            self.startup_scans_total,
            self.subscription_scans_total,
            self.poll_fallback_scans_total,
            self.reconciliation_scans_total,
            self.last_successful_scan_ts,
            self.last_telegram_delivery_ts,
            self.last_subscription_event_ts,
            u8::from(self.node_connected),
            u8::from(self.subscription_active),
        )
    }
}

fn effective_subscription_requirement(mode: MonitoringMode, requested: bool) -> bool {
    mode.requires_subscription() && requested
}

fn env_bool(key: &str, default_value: bool) -> bool {
    let Ok(raw_value) = std::env::var(key) else {
        return default_value;
    };

    parse_bool_or_default(key, &raw_value, default_value)
}

fn parse_bool_or_default(key: &str, raw_value: &str, default_value: bool) -> bool {
    match parse_bool(raw_value) {
        Some(value) => value,
        None => {
            tracing::warn!(
                "[CONFIG] Invalid boolean value for {}='{}'; using default {}.",
                key,
                raw_value.trim(),
                default_value
            );
            default_value
        }
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn env_u64(key: &str, default_value: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default_value)
}

fn env_i64(key: &str, default_value: i64) -> i64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(default_value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_snapshot() -> OperationalSnapshot {
        OperationalSnapshot {
            queue: QueueSnapshot {
                pending: 2,
                processing: 1,
                failed: 3,
                failed_recent: 0,
                oldest_age_seconds: 7,
            },
            delivery_latency_ms_last: 30,
            delivery_latency_ms_sum: 100,
            delivery_latency_samples: 4,
            outbox_failures_total: 0,
            node_reconnects_total: 3,
            subscription_reregistrations_total: 3,
            subscription_registration_failures_total: 0,
            subscription_runtime_restarts_total: 0,
            callbacks_rejected_inflight_total: 1,
            startup_scans_total: 1,
            subscription_scans_total: 2,
            poll_fallback_scans_total: 3,
            reconciliation_scans_total: 4,
            last_successful_scan_ts: 1_000,
            last_telegram_delivery_ts: 990,
            last_subscription_event_ts: 995,
            node_connected: true,
            subscription_active: true,
        }
    }

    fn policy() -> ReadinessPolicy {
        ReadinessPolicy {
            require_node_connection: true,
            require_active_subscription: true,
            max_scan_age_seconds: 30,
            max_failed_queue_items: 0,
            failed_queue_window_seconds: 3_600,
        }
    }

    #[test]
    fn readiness_is_ready_when_dependencies_are_fresh() {
        assert_eq!(
            sample_snapshot().readiness(1_010, policy()),
            ReadinessState::Ready
        );
    }

    #[test]
    fn historical_failed_queue_rows_do_not_degrade_readiness() {
        let snapshot = sample_snapshot();
        assert_eq!(snapshot.queue.failed, 3);
        assert_eq!(snapshot.queue.failed_recent, 0);
        assert_eq!(snapshot.readiness(1_010, policy()), ReadinessState::Ready);
    }

    #[test]
    fn recent_failed_queue_rows_degrade_readiness() {
        let mut snapshot = sample_snapshot();
        snapshot.queue.failed_recent = 1;
        assert_eq!(
            snapshot.readiness(1_010, policy()),
            ReadinessState::Degraded
        );
    }

    #[test]
    fn stale_scan_is_degraded_without_sleeping() {
        assert_eq!(
            sample_snapshot().readiness(1_100, policy()),
            ReadinessState::Degraded
        );
    }

    #[test]
    fn disconnected_node_is_not_ready() {
        let mut snapshot = sample_snapshot();
        snapshot.node_connected = false;
        assert_eq!(
            snapshot.readiness(1_010, policy()),
            ReadinessState::NotReady
        );
    }

    #[test]
    fn inactive_required_subscription_is_degraded() {
        let mut snapshot = sample_snapshot();
        snapshot.subscription_active = false;
        assert_eq!(
            snapshot.readiness(1_010, policy()),
            ReadinessState::Degraded
        );
    }

    #[test]
    fn polling_only_mode_can_be_ready_without_subscription() {
        let mut snapshot = sample_snapshot();
        snapshot.subscription_active = false;
        let mut readiness_policy = policy();
        readiness_policy.require_active_subscription = false;
        assert_eq!(
            snapshot.readiness(1_010, readiness_policy),
            ReadinessState::Ready
        );
    }

    #[test]
    fn polling_only_mode_never_requires_subscription_readiness() {
        assert!(!effective_subscription_requirement(
            MonitoringMode::PollingOnly,
            true
        ));
        assert!(effective_subscription_requirement(
            MonitoringMode::SubscriptionPreferred,
            true
        ));
    }

    #[test]
    fn boolean_parser_accepts_known_values_and_rejects_typos() {
        assert_eq!(parse_bool("true"), Some(true));
        assert_eq!(parse_bool("OFF"), Some(false));
        assert_eq!(parse_bool("treu"), None);
        assert!(parse_bool_or_default("TEST_FLAG", "treu", true));
        assert!(!parse_bool_or_default("TEST_FLAG", "treu", false));
    }

    #[test]
    fn prometheus_output_contains_operational_metrics() {
        let rendered = sample_snapshot().render_prometheus();
        assert!(rendered.contains("kaspa_pulse_queue_pending 2"));
        assert!(rendered.contains("kaspa_pulse_queue_failed 3"));
        assert!(rendered.contains("kaspa_pulse_queue_failed_recent 0"));
        assert!(rendered.contains("kaspa_pulse_delivery_latency_ms_average 25"));
        assert!(rendered.contains("kaspa_pulse_node_reconnects_total 3"));
        assert!(rendered.contains("kaspa_pulse_subscription_runtime_restarts_total 0"));
        assert!(rendered.contains("kaspa_pulse_subscription_active 1"));
    }
}
