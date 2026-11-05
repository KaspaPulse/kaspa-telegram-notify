#[derive(Debug, Clone)]
pub struct LiveBlockEvent {
    pub is_coinbase: bool,
    pub wallet_address: String,
    pub amount_kas: f64,
    pub live_balance_kas: f64,
    pub source_outpoint: String,
    pub alert_key: String,
    pub tx_id: String,
    pub block_time_ms: u64,
    pub acc_block_hash: String,
    pub mined_block_hash: Option<String>,
    pub extracted_worker: Option<String>,
    pub daa_score: u64,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BotEventType {
    SystemStart,
    SystemShutdown,
    WebhookStart,
    AlertDetected,
    AlertDelivered,
    AlertDeliveryFailed,
    AlertDeliverySuppressed,
    AlertDuplicateSkipped,
    DbError,
    RpcError,
    RpcRecovered,
    TelegramError,
    PanicEvent,
    CommandIn,
    CallbackIn,
    RateLimited,
    AdminAction,
    EventLogPurged,
}

impl BotEventType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SystemStart => "SYSTEM_START",
            Self::SystemShutdown => "SYSTEM_SHUTDOWN",
            Self::WebhookStart => "WEBHOOK_START",
            Self::AlertDetected => "ALERT_DETECTED",
            Self::AlertDelivered => "ALERT_DELIVERED",
            Self::AlertDeliveryFailed => "ALERT_DELIVERY_FAILED",
            Self::AlertDeliverySuppressed => "ALERT_DELIVERY_SUPPRESSED",
            Self::AlertDuplicateSkipped => "ALERT_DUPLICATE_SKIPPED",
            Self::DbError => "DB_ERROR",
            Self::RpcError => "RPC_ERROR",
            Self::RpcRecovered => "RPC_RECOVERED",
            Self::TelegramError => "TELEGRAM_ERROR",
            Self::PanicEvent => "PANIC_EVENT",
            Self::CommandIn => "COMMAND_IN",
            Self::CallbackIn => "CALLBACK_IN",
            Self::RateLimited => "RATE_LIMITED",
            Self::AdminAction => "ADMIN_ACTION",
            Self::EventLogPurged => "EVENT_LOG_PURGED",
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventSeverity {
    Info,
    Warn,
    Error,
}

impl EventSeverity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone)]
pub struct BotEventRecord<'a> {
    pub event_type: BotEventType,
    pub severity: EventSeverity,
    pub chat_id: Option<i64>,
    pub user_name: Option<&'a str>,
    pub command: Option<&'a str>,
    pub callback_data: Option<&'a str>,
    pub wallet_masked: Option<&'a str>,
    pub txid_masked: Option<&'a str>,
    pub block_hash_masked: Option<&'a str>,
    pub status: Option<&'a str>,
    pub error_message: Option<&'a str>,
    pub duration_ms: Option<i64>,
    pub metadata_json: &'a str,
}

impl<'a> BotEventRecord<'a> {
    pub const fn new(event_type: BotEventType, severity: EventSeverity) -> Self {
        Self {
            event_type,
            severity,
            chat_id: None,
            user_name: None,
            command: None,
            callback_data: None,
            wallet_masked: None,
            txid_masked: None,
            block_hash_masked: None,
            status: None,
            error_message: None,
            duration_ms: None,
            metadata_json: "{}",
        }
    }
}
