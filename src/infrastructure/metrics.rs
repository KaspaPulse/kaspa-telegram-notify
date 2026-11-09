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

// OP063: privacy-safe counters. No wallet, tx or subscriber labels.
static PROOF_INCOMPLETE_TOTAL: AtomicU64 = AtomicU64::new(0);
static REWARD_BATCH_DEFERRED_TOTAL: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_GATE_BUSY_TOTAL: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_COOLDOWN_TOTAL: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_INCOMPLETE_TOTAL: AtomicU64 = AtomicU64::new(0);
// OP084: public aggregate PNN route diagnostics, NO dynamic TX/wallet/hash fields.
static PNN_COINBASE_ACCEPTING_CONTAINING_SPLIT_FALLBACK: AtomicU64 = AtomicU64::new(0);
static PNN_COINBASE_BLUE_MAPPING_FALLBACK: AtomicU64 = AtomicU64::new(0);
static PNN_COINBASE_NATIVE_BLUE_VERIFIED: AtomicU64 = AtomicU64::new(0);
static PNN_COINBASE_NATIVE_SELF_RED_VERIFIED: AtomicU64 = AtomicU64::new(0);
static PNN_COINBASE_NATIVE_NO_MINER_FALLBACK: AtomicU64 = AtomicU64::new(0);

// OP080: fixed labels ONLY; no raw TX, wallet, URL or REST response values.
static ARCHIVE_TX_METADATA_NOT_ACCEPTED: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_TX_METADATA_ACCEPTANCE_FLAG_INVALID: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_TX_METADATA_TRANSACTION_ID_MISMATCH: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_TX_METADATA_ACCEPTING_HASH_MISSING: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_TX_METADATA_ACCEPTING_HASH_INVALID: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_TX_METADATA_CONTAINING_BLOCK_NOT_UNIQUE: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_TX_METADATA_CONTAINING_BLOCK_HASH_INVALID: AtomicU64 = AtomicU64::new(0);

static ARCHIVE_INCOMPLETE_TRANSACTIONMETADATA: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_INCOMPLETE_ACCEPTINGBLOCK: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_INCOMPLETE_ACCEPTANCERELATION: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_INCOMPLETE_CONTAININGBLOCK: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_INCOMPLETE_WALLETSCRIPT: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_INCOMPLETE_BLUESETSHAPE: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_INCOMPLETE_MINEDBLUEBLOCK: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_INCOMPLETE_MINERATTRIBUTION: AtomicU64 = AtomicU64::new(0);

static ARCHIVE_FETCH_ERROR_TOTAL: AtomicU64 = AtomicU64::new(0);
// OP077: Aggregate category counters ONLY. No URL, txid, wallet, or
// arbitrary external error text is ever exposed in Prometheus labels.
static ARCHIVE_FETCH_HTTP_429: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_FETCH_HTTP_4XX: AtomicU64 = AtomicU64::new(0);
// OP078: fixed-code/fixed-stage classification of future REST 4xx only.
// No endpoint, user, transaction ID or dynamic Prometheus labels.
static ARCHIVE_FETCH_HTTP_404: AtomicU64 = AtomicU64::new(0);
// OP081: aggregate bounded block 404 retry and final role diagnostics.
// Counters never include public URLs, raw txids, hashes or wallets.
static ARCHIVE_BLOCK_404_RETRY_ATTEMPT: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_BLOCK_404_RETRY_RECOVERED: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_BLOCK_404_RETRY_EXHAUSTED: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_BLOCK_404_FINAL_ACCEPTING: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_BLOCK_404_FINAL_CONTAINING: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_BLOCK_404_FINAL_MINED_BLUE: AtomicU64 = AtomicU64::new(0);

static ARCHIVE_FETCH_HTTP_4XX_OTHER: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_FETCH_HTTP_4XX_TX_META: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_FETCH_HTTP_4XX_BLOCK: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_FETCH_HTTP_4XX_UNKNOWN_STAGE: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_FETCH_HTTP_5XX: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_FETCH_TIMEOUT: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_FETCH_INVALID_JSON: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_FETCH_OVERSIZE: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_FETCH_TRANSPORT_OR_OTHER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArchiveFetchErrorClass {
    RateLimited,
    Client4xx,
    Server5xx,
    Timeout,
    InvalidJson,
    Oversize,
    TransportOrOther,
}
fn classify_archive_error(error: &str) -> ArchiveFetchErrorClass {
    // Order is intentional: 429 is distinct from generic HTTP 4xx;
    // no error payload ever appears in a Prometheus series label.
    let normalized = error.to_ascii_lowercase();
    if normalized.contains("rest http 429") {
        ArchiveFetchErrorClass::RateLimited
    } else if normalized.contains("rest http 4") {
        ArchiveFetchErrorClass::Client4xx
    } else if normalized.contains("rest http 5") {
        ArchiveFetchErrorClass::Server5xx
    } else if normalized.contains("timed out") || normalized.contains("timeout") {
        ArchiveFetchErrorClass::Timeout
    } else if normalized.contains("invalid rest json") {
        ArchiveFetchErrorClass::InvalidJson
    } else if normalized.contains("response exceeds size limit")
        || normalized.contains("response chunk exceeds size limit")
    {
        ArchiveFetchErrorClass::Oversize
    } else {
        ArchiveFetchErrorClass::TransportOrOther
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArchiveFetchStage {
    Transaction,
    Block,
    Unknown,
}
fn classify_archive_http_4xx_details(error: &str) -> (bool, ArchiveFetchStage) {
    let normalized = error.to_ascii_lowercase();
    // Exact fixed HTTP status; never interpret arbitrary transaction text.
    let is_404 = normalized == "rest http 404" || normalized.starts_with("rest http 404 ");
    let stage = if normalized.ends_with("[archive_stage=transaction]") {
        ArchiveFetchStage::Transaction
    } else if normalized.ends_with("[archive_stage=block]") {
        ArchiveFetchStage::Block
    } else {
        ArchiveFetchStage::Unknown
    };
    (is_404, stage)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveBlock404Role {
    Accepting,
    Containing,
    MinedBlue,
}

pub fn record_archive_block_404_retry_attempt() {
    ARCHIVE_BLOCK_404_RETRY_ATTEMPT.fetch_add(1, Ordering::Relaxed);
}
pub fn record_archive_block_404_retry_recovered() {
    ARCHIVE_BLOCK_404_RETRY_RECOVERED.fetch_add(1, Ordering::Relaxed);
}
pub fn record_archive_block_404_retry_exhausted() {
    ARCHIVE_BLOCK_404_RETRY_EXHAUSTED.fetch_add(1, Ordering::Relaxed);
}
pub fn record_archive_block_final_404(role: ArchiveBlock404Role) {
    match role {
        ArchiveBlock404Role::Accepting => {
            ARCHIVE_BLOCK_404_FINAL_ACCEPTING.fetch_add(1, Ordering::Relaxed);
        }
        ArchiveBlock404Role::Containing => {
            ARCHIVE_BLOCK_404_FINAL_CONTAINING.fetch_add(1, Ordering::Relaxed);
        }
        ArchiveBlock404Role::MinedBlue => {
            ARCHIVE_BLOCK_404_FINAL_MINED_BLUE.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub fn record_archive_fetch_error_classified(error: &str) {
    ARCHIVE_FETCH_ERROR_TOTAL.fetch_add(1, Ordering::Relaxed);
    let category = classify_archive_error(error);
    let counter = match category {
        ArchiveFetchErrorClass::RateLimited => &ARCHIVE_FETCH_HTTP_429,
        ArchiveFetchErrorClass::Client4xx => &ARCHIVE_FETCH_HTTP_4XX,
        ArchiveFetchErrorClass::Server5xx => &ARCHIVE_FETCH_HTTP_5XX,
        ArchiveFetchErrorClass::Timeout => &ARCHIVE_FETCH_TIMEOUT,
        ArchiveFetchErrorClass::InvalidJson => &ARCHIVE_FETCH_INVALID_JSON,
        ArchiveFetchErrorClass::Oversize => &ARCHIVE_FETCH_OVERSIZE,
        ArchiveFetchErrorClass::TransportOrOther => &ARCHIVE_FETCH_TRANSPORT_OR_OTHER,
    };
    counter.fetch_add(1, Ordering::Relaxed);
    if category == ArchiveFetchErrorClass::Client4xx {
        let (not_found, stage) = classify_archive_http_4xx_details(error);
        if not_found {
            ARCHIVE_FETCH_HTTP_404.fetch_add(1, Ordering::Relaxed);
        } else {
            ARCHIVE_FETCH_HTTP_4XX_OTHER.fetch_add(1, Ordering::Relaxed);
        }
        match stage {
            ArchiveFetchStage::Transaction => {
                ARCHIVE_FETCH_HTTP_4XX_TX_META.fetch_add(1, Ordering::Relaxed)
            }
            ArchiveFetchStage::Block => {
                ARCHIVE_FETCH_HTTP_4XX_BLOCK.fetch_add(1, Ordering::Relaxed)
            }
            ArchiveFetchStage::Unknown => {
                ARCHIVE_FETCH_HTTP_4XX_UNKNOWN_STAGE.fetch_add(1, Ordering::Relaxed)
            }
        };
    }
}

static ARCHIVE_ADMITTED_TOTAL: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_ADMISSION_TIMEOUT_TOTAL: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_ADMISSION_WAIT_MS_LAST: AtomicU64 = AtomicU64::new(0);
// OP085: no identifying labels, only aggregate durations and outcomes.
static PNN_SPLIT_REST_COMPLETE_TOTAL: AtomicU64 = AtomicU64::new(0);
static PNN_SPLIT_REST_DEFERRED_TOTAL: AtomicU64 = AtomicU64::new(0);
static PNN_SPLIT_REST_CANCELLED_TOTAL: AtomicU64 = AtomicU64::new(0);
static PNN_SPLIT_REST_LATENCY_MS_LAST: AtomicU64 = AtomicU64::new(0);
static PNN_SPLIT_REST_LATENCY_MS_MAX: AtomicU64 = AtomicU64::new(0);

static PROOF_NEWER_OVER_10M: AtomicU64 = AtomicU64::new(0);
static PROOF_BACKLOG_COUNT: AtomicU64 = AtomicU64::new(0);
static PROOF_BACKLOG_OLDER_DAY: AtomicU64 = AtomicU64::new(0);
static PROOF_BACKLOG_MAX_AGE_SECONDS: AtomicU64 = AtomicU64::new(0);
static PROOF_MISSING_ACCEPTANCE_COUNT: AtomicU64 = AtomicU64::new(0);
static PROOF_MAX_RETRY_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
// OP076: scope current reward-proof health separate from the 68+ permanently
// excluded pre-cutover records. No wallet, tx or subscriber labels.
static PROOF_NEWER_WAITING: AtomicU64 = AtomicU64::new(0);
static PROOF_HISTORIC_EXCLUDED: AtomicU64 = AtomicU64::new(0);
static PROOF_NEWER_MAX_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static PROOF_NEWER_DUE_FOR_RETRY: AtomicU64 = AtomicU64::new(0);
static BLOCK_TO_OUTBOX_MS_LAST: AtomicU64 = AtomicU64::new(0);
static BLOCK_TO_OUTBOX_MS_MAX: AtomicU64 = AtomicU64::new(0);
static BLOCK_TO_OUTBOX_SAMPLES: AtomicU64 = AtomicU64::new(0);

pub fn record_proof_incomplete() {
    PROOF_INCOMPLETE_TOTAL.fetch_add(1, Ordering::Relaxed);
}
pub fn record_reward_batch_deferred(count: u64) {
    REWARD_BATCH_DEFERRED_TOTAL.fetch_add(count, Ordering::Relaxed);
}
pub fn record_archive_gate_busy() {
    ARCHIVE_GATE_BUSY_TOTAL.fetch_add(1, Ordering::Relaxed);
}
pub fn record_archive_cooldown() {
    ARCHIVE_COOLDOWN_TOTAL.fetch_add(1, Ordering::Relaxed);
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveIncompleteReason {
    TransactionMetadata,
    AcceptingBlock,
    AcceptanceRelation,
    ContainingBlock,
    WalletScript,
    BlueSetShape,
    MinedBlueBlock,
    MinerAttribution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveTxMetadataFailure {
    NotAccepted,
    AcceptanceFlagInvalid,
    TransactionIdMismatch,
    AcceptingHashMissing,
    AcceptingHashInvalid,
    ContainingBlockNotUnique,
    ContainingBlockHashInvalid,
}

pub fn record_archive_tx_metadata_failure(reason: ArchiveTxMetadataFailure) {
    match reason {
        ArchiveTxMetadataFailure::NotAccepted => {
            ARCHIVE_TX_METADATA_NOT_ACCEPTED.fetch_add(1, Ordering::Relaxed)
        }
        ArchiveTxMetadataFailure::AcceptanceFlagInvalid => {
            ARCHIVE_TX_METADATA_ACCEPTANCE_FLAG_INVALID.fetch_add(1, Ordering::Relaxed)
        }
        ArchiveTxMetadataFailure::TransactionIdMismatch => {
            ARCHIVE_TX_METADATA_TRANSACTION_ID_MISMATCH.fetch_add(1, Ordering::Relaxed)
        }
        ArchiveTxMetadataFailure::AcceptingHashMissing => {
            ARCHIVE_TX_METADATA_ACCEPTING_HASH_MISSING.fetch_add(1, Ordering::Relaxed)
        }
        ArchiveTxMetadataFailure::AcceptingHashInvalid => {
            ARCHIVE_TX_METADATA_ACCEPTING_HASH_INVALID.fetch_add(1, Ordering::Relaxed)
        }
        ArchiveTxMetadataFailure::ContainingBlockNotUnique => {
            ARCHIVE_TX_METADATA_CONTAINING_BLOCK_NOT_UNIQUE.fetch_add(1, Ordering::Relaxed)
        }
        ArchiveTxMetadataFailure::ContainingBlockHashInvalid => {
            ARCHIVE_TX_METADATA_CONTAINING_BLOCK_HASH_INVALID.fetch_add(1, Ordering::Relaxed)
        }
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PnnCoinbaseRoute {
    AcceptingContainingSplitFallback,
    BlueMappingFallback,
    NativeBlueVerified,
    NativeSelfRedVerified,
    NativeNoMinerFallback,
}

pub fn record_pnn_coinbase_route(route: PnnCoinbaseRoute) {
    let counter = match route {
        PnnCoinbaseRoute::AcceptingContainingSplitFallback => {
            &PNN_COINBASE_ACCEPTING_CONTAINING_SPLIT_FALLBACK
        }
        PnnCoinbaseRoute::BlueMappingFallback => &PNN_COINBASE_BLUE_MAPPING_FALLBACK,
        PnnCoinbaseRoute::NativeBlueVerified => &PNN_COINBASE_NATIVE_BLUE_VERIFIED,
        PnnCoinbaseRoute::NativeSelfRedVerified => &PNN_COINBASE_NATIVE_SELF_RED_VERIFIED,
        PnnCoinbaseRoute::NativeNoMinerFallback => &PNN_COINBASE_NATIVE_NO_MINER_FALLBACK,
    };
    counter.fetch_add(1, Ordering::Relaxed);
}

pub fn record_archive_incomplete(reason: ArchiveIncompleteReason) {
    ARCHIVE_INCOMPLETE_TOTAL.fetch_add(1, Ordering::Relaxed);
    match reason {
        ArchiveIncompleteReason::TransactionMetadata => {
            ARCHIVE_INCOMPLETE_TRANSACTIONMETADATA.fetch_add(1, Ordering::Relaxed);
        }
        ArchiveIncompleteReason::AcceptingBlock => {
            ARCHIVE_INCOMPLETE_ACCEPTINGBLOCK.fetch_add(1, Ordering::Relaxed);
        }
        ArchiveIncompleteReason::AcceptanceRelation => {
            ARCHIVE_INCOMPLETE_ACCEPTANCERELATION.fetch_add(1, Ordering::Relaxed);
        }
        ArchiveIncompleteReason::ContainingBlock => {
            ARCHIVE_INCOMPLETE_CONTAININGBLOCK.fetch_add(1, Ordering::Relaxed);
        }
        ArchiveIncompleteReason::WalletScript => {
            ARCHIVE_INCOMPLETE_WALLETSCRIPT.fetch_add(1, Ordering::Relaxed);
        }
        ArchiveIncompleteReason::BlueSetShape => {
            ARCHIVE_INCOMPLETE_BLUESETSHAPE.fetch_add(1, Ordering::Relaxed);
        }
        ArchiveIncompleteReason::MinedBlueBlock => {
            ARCHIVE_INCOMPLETE_MINEDBLUEBLOCK.fetch_add(1, Ordering::Relaxed);
        }
        ArchiveIncompleteReason::MinerAttribution => {
            ARCHIVE_INCOMPLETE_MINERATTRIBUTION.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub fn record_archive_admitted(wait_ms: u64) {
    ARCHIVE_ADMITTED_TOTAL.fetch_add(1, Ordering::Relaxed);
    ARCHIVE_ADMISSION_WAIT_MS_LAST.store(wait_ms, Ordering::Relaxed);
}
/// Called by the OP085 native PNN split fallback's Drop guard, including
/// cancellation. A completed true result means canonical REST evidence passed;
/// this metric is NOT itself evidence of outbox enqueue or Telegram delivery.
pub fn observe_pnn_split_rest_latency(elapsed_ms: u64, completed: Option<bool>) {
    PNN_SPLIT_REST_LATENCY_MS_LAST.store(elapsed_ms, Ordering::Relaxed);
    PNN_SPLIT_REST_LATENCY_MS_MAX.fetch_max(elapsed_ms, Ordering::Relaxed);
    match completed {
        Some(true) => {
            PNN_SPLIT_REST_COMPLETE_TOTAL.fetch_add(1, Ordering::Relaxed);
        }
        Some(false) => {
            PNN_SPLIT_REST_DEFERRED_TOTAL.fetch_add(1, Ordering::Relaxed);
        }
        None => {
            PNN_SPLIT_REST_CANCELLED_TOTAL.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub fn record_archive_wait_timeout() {
    ARCHIVE_ADMISSION_TIMEOUT_TOTAL.fetch_add(1, Ordering::Relaxed);
}
#[derive(Debug, Clone, Copy)]
pub struct ProofBacklogSnapshot {
    pub count: u64,
    pub old_day: u64,
    pub max_age: u64,
    pub missing_acceptance: u64,
    pub max_retries: u64,
    pub newer_over_10m: u64,
    pub newer_waiting: u64,
    pub historic_excluded: u64,
    pub newer_max_attempts: u64,
    pub newer_due_for_retry: u64,
}

pub fn set_proof_backlog(snapshot: ProofBacklogSnapshot) {
    let ProofBacklogSnapshot {
        count,
        old_day,
        max_age,
        missing_acceptance,
        max_retries,
        newer_over_10m,
        newer_waiting,
        historic_excluded,
        newer_max_attempts,
        newer_due_for_retry,
    } = snapshot;
    PROOF_NEWER_WAITING.store(newer_waiting, Ordering::Relaxed);
    PROOF_HISTORIC_EXCLUDED.store(historic_excluded, Ordering::Relaxed);
    PROOF_NEWER_MAX_ATTEMPTS.store(newer_max_attempts, Ordering::Relaxed);
    PROOF_NEWER_DUE_FOR_RETRY.store(newer_due_for_retry, Ordering::Relaxed);
    PROOF_BACKLOG_COUNT.store(count, Ordering::Relaxed);
    PROOF_BACKLOG_OLDER_DAY.store(old_day, Ordering::Relaxed);
    PROOF_BACKLOG_MAX_AGE_SECONDS.store(max_age, Ordering::Relaxed);
    PROOF_MISSING_ACCEPTANCE_COUNT.store(missing_acceptance, Ordering::Relaxed);
    PROOF_MAX_RETRY_ATTEMPTS.store(max_retries, Ordering::Relaxed);
    PROOF_NEWER_OVER_10M.store(newer_over_10m, Ordering::Relaxed);
}
// Only time-valid recent source blocks; archives cannot distort the live SLO.
pub fn observe_block_to_outbox(block_time_ms: u64) {
    let now_ms = now_unix_secs().saturating_mul(1000);
    if block_time_ms == 0 || block_time_ms > now_ms.saturating_add(5_000) {
        return;
    }
    let elapsed = now_ms.saturating_sub(block_time_ms);
    if elapsed > 600_000 {
        return;
    }
    BLOCK_TO_OUTBOX_MS_LAST.store(elapsed, Ordering::Relaxed);
    BLOCK_TO_OUTBOX_MS_MAX.fetch_max(elapsed, Ordering::Relaxed);
    BLOCK_TO_OUTBOX_SAMPLES.fetch_add(1, Ordering::Relaxed);
}

// OP066: public-PNN resilience telemetry, no node URLs, wallet IDs or secrets.
static PNN_STANDBY_QUALIFIED: AtomicU64 = AtomicU64::new(0);
static PNN_UTXO_SOURCE_IS_STANDBY: AtomicU64 = AtomicU64::new(0);
static PNN_FAILOVER_TRANSITIONS: AtomicU64 = AtomicU64::new(0);
static PNN_FORCED_RECOVERY_ATTEMPTS: AtomicU64 = AtomicU64::new(0);

pub fn set_pnn_standby_qualified(ready: bool) {
    PNN_STANDBY_QUALIFIED.store(u64::from(ready), Ordering::Relaxed);
}
pub fn set_pnn_utxo_standby_selected(selected: bool) {
    PNN_UTXO_SOURCE_IS_STANDBY.store(u64::from(selected), Ordering::Relaxed);
}
pub fn inc_pnn_failover_transitions() {
    PNN_FAILOVER_TRANSITIONS.fetch_add(1, Ordering::Relaxed);
}
pub fn inc_pnn_forced_recoveries() {
    PNN_FORCED_RECOVERY_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
}

// OP067: five-source selection telemetry; no URLs, wallet data or secrets.
static PNN_POOL_SIZE: AtomicU64 = AtomicU64::new(0);
static PNN_POOL_ELIGIBLE: AtomicU64 = AtomicU64::new(0);
static PNN_POOL_ACTIVE_RANK: AtomicU64 = AtomicU64::new(0);
static PNN_POOL_ELIGIBLE_SLOTS_MASK: AtomicU64 = AtomicU64::new(0);
// OP075: only aggregate counts / selected pool epoch. No public IPs,
// resolver URLs, wallet addresses, Telegram account IDs, or secrets.
static PNN_SOURCE_GENERATION: AtomicU64 = AtomicU64::new(0);
static PNN_DYNAMIC_ROTATION_ENABLED: AtomicU64 = AtomicU64::new(0);
pub fn set_pnn_dynamic_rotation_enabled(enabled: bool) {
    PNN_DYNAMIC_ROTATION_ENABLED.store(u64::from(enabled), Ordering::Relaxed);
}

static PNN_SPARE_DISCOVERY_TOTAL: AtomicU64 = AtomicU64::new(0);
static PNN_SPARE_SWAP_SUCCESS_TOTAL: AtomicU64 = AtomicU64::new(0);
static PNN_SPARE_REJECTED_TOTAL: AtomicU64 = AtomicU64::new(0);
static PNN_REJECT_DISCOVERY_UNAVAILABLE: AtomicU64 = AtomicU64::new(0);
static PNN_REJECT_STALE_OR_RECOVERED: AtomicU64 = AtomicU64::new(0);
static PNN_REJECT_COEVAL_UNQUALIFIED: AtomicU64 = AtomicU64::new(0);
static PNN_REJECT_REGISTRY_FENCE: AtomicU64 = AtomicU64::new(0);
pub fn set_pnn_source_generation(epoch: u64) {
    PNN_SOURCE_GENERATION.store(epoch, Ordering::Relaxed);
}
pub fn inc_pnn_spare_discovery() {
    PNN_SPARE_DISCOVERY_TOTAL.fetch_add(1, Ordering::Relaxed);
}
pub fn inc_pnn_spare_swap_success() {
    PNN_SPARE_SWAP_SUCCESS_TOTAL.fetch_add(1, Ordering::Relaxed);
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PnnSpareRejectReason {
    DiscoveryUnavailable,
    StaleOrRecovered,
    CoevalUnqualified,
    RegistryFence,
}

pub fn inc_pnn_spare_rejected(reason: PnnSpareRejectReason) {
    PNN_SPARE_REJECTED_TOTAL.fetch_add(1, Ordering::Relaxed);
    match reason {
        PnnSpareRejectReason::DiscoveryUnavailable => {
            PNN_REJECT_DISCOVERY_UNAVAILABLE.fetch_add(1, Ordering::Relaxed);
        }
        PnnSpareRejectReason::StaleOrRecovered => {
            PNN_REJECT_STALE_OR_RECOVERED.fetch_add(1, Ordering::Relaxed);
        }
        PnnSpareRejectReason::CoevalUnqualified => {
            PNN_REJECT_COEVAL_UNQUALIFIED.fetch_add(1, Ordering::Relaxed);
        }
        PnnSpareRejectReason::RegistryFence => {
            PNN_REJECT_REGISTRY_FENCE.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub fn set_pnn_pool_size(count: usize) {
    PNN_POOL_SIZE.store(count as u64, Ordering::Relaxed);
}
pub fn set_pnn_pool_eligible_count(count: usize) {
    PNN_POOL_ELIGIBLE.store(count as u64, Ordering::Relaxed);
}
pub fn set_pnn_pool_eligible_slots_mask(mask: u8) {
    // At most 5 publicly qualified sources; no URL, IP or wallet label.
    PNN_POOL_ELIGIBLE_SLOTS_MASK.store((mask & 0b1_1111) as u64, Ordering::Relaxed);
}
pub fn set_pnn_pool_active_index(index: Option<usize>) {
    // 0 = no safe source, 1..=5 = selected candidate ordinal.
    PNN_POOL_ACTIVE_RANK.store(index.map_or(0, |i| (i + 1) as u64), Ordering::Relaxed);
}
fn pnn_reward_source_fresh(eligible: u64, active_rank: u64, last_scan: u64, now: u64) -> bool {
    eligible > 0
        && (1..=5).contains(&active_rank)
        && last_scan > 0
        && now >= last_scan
        && now - last_scan <= 90
}
// If the owner explicitly selects the five-source mode, a failed
// startup admission cannot silently advertise green health using a
// one-source fallback. Keep the bot live for safe read-only diagnostics
// and optional primary scan, but fail CLOSED in /readyz until a genuine
// qualified public pool of at least two independently live sources exists.
fn public_five_pool_quorum(qualified: u64, eligible: u64, rank: u64) -> bool {
    qualified == 5 && (2..=5).contains(&eligible) && (1..=5).contains(&rank)
}
pub fn required_five_pool_ready() -> bool {
    if std::env::var("KASPA_PNN_FIVE_WAY_POOL").as_deref() != Ok("1") {
        return true;
    }
    public_five_pool_quorum(
        PNN_POOL_SIZE.load(Ordering::Relaxed),
        PNN_POOL_ELIGIBLE.load(Ordering::Relaxed),
        PNN_POOL_ACTIVE_RANK.load(Ordering::Relaxed),
    )
}

pub fn pnn_reward_source_ready() -> bool {
    // A historic quality snapshot cannot hide a genuine total outage.
    // Presence of eligible sources is necessary, but fresh successful
    // transactional scans are REQUIRED before silencing admin warnings.
    pnn_reward_source_fresh(
        PNN_POOL_ELIGIBLE.load(Ordering::Relaxed),
        PNN_POOL_ACTIVE_RANK.load(Ordering::Relaxed),
        crate::infrastructure::observability::snapshot().last_successful_scan_ts,
        now_unix_secs(),
    )
}
pub fn pnn_pool_active_index() -> Option<usize> {
    let ordinal = PNN_POOL_ACTIVE_RANK.load(Ordering::Relaxed);
    if ordinal == 0 {
        None
    } else {
        Some((ordinal - 1) as usize)
    }
}

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

    rendered.push_str(&format!(
        concat!(
            "# TYPE kaspa_pulse_proof_incomplete_attempts_total counter\n",
            "kaspa_pulse_proof_incomplete_attempts_total {}\n",
            "# TYPE kaspa_pulse_reward_batch_deferred_observations_total counter\n",
            "kaspa_pulse_reward_batch_deferred_observations_total {}\n",
            "kaspa_pulse_archive_gate_busy_total {}\n",
            "kaspa_pulse_archive_cooldown_total {}\n",
            "kaspa_pulse_archive_incomplete_total {}\n",
            "kaspa_pulse_archive_fetch_error_total {}\n",
            "# TYPE kaspa_pulse_archive_admitted_total counter\n",
            "kaspa_pulse_archive_admitted_total {}\n",
            "# TYPE kaspa_pulse_archive_admission_wait_timeout_total counter\n",
            "kaspa_pulse_archive_admission_wait_timeout_total {}\n",
            "# TYPE kaspa_pulse_archive_admission_wait_ms_last gauge\n",
            "kaspa_pulse_archive_admission_wait_ms_last {}\n",
            "# TYPE kaspa_pulse_proof_backlog gauge\n",
            "kaspa_pulse_proof_backlog {}\n",
            "kaspa_pulse_proof_backlog_older_day {}\n",
            "kaspa_pulse_proof_backlog_max_age_seconds {}\n",
            "kaspa_pulse_proof_missing_acceptance {}\n",
            "kaspa_pulse_proof_max_retry_attempts {}\n",
            "# TYPE kaspa_pulse_proof_newer_over_10m gauge\n",
            "kaspa_pulse_proof_newer_over_10m {}\n",
            "# TYPE kaspa_pulse_proof_newer_waiting gauge\n",
            "kaspa_pulse_proof_newer_waiting {}\n",
            "# TYPE kaspa_pulse_proof_historic_excluded gauge\n",
            "kaspa_pulse_proof_historic_excluded {}\n",
            "# TYPE kaspa_pulse_proof_newer_max_attempts gauge\n",
            "kaspa_pulse_proof_newer_max_attempts {}\n",
            "# TYPE kaspa_pulse_proof_newer_due_for_retry gauge\n",
            "kaspa_pulse_proof_newer_due_for_retry {}\n",
            "kaspa_pulse_recent_block_to_outbox_ms_last {}\n",
            "kaspa_pulse_recent_block_to_outbox_ms_max {}\n",
            "kaspa_pulse_recent_block_to_outbox_samples_total {}\n"
        ),
        PROOF_INCOMPLETE_TOTAL.load(Ordering::Relaxed),
        REWARD_BATCH_DEFERRED_TOTAL.load(Ordering::Relaxed),
        ARCHIVE_GATE_BUSY_TOTAL.load(Ordering::Relaxed),
        ARCHIVE_COOLDOWN_TOTAL.load(Ordering::Relaxed),
        ARCHIVE_INCOMPLETE_TOTAL.load(Ordering::Relaxed),
        ARCHIVE_FETCH_ERROR_TOTAL.load(Ordering::Relaxed),
        ARCHIVE_ADMITTED_TOTAL.load(Ordering::Relaxed),
        ARCHIVE_ADMISSION_TIMEOUT_TOTAL.load(Ordering::Relaxed),
        ARCHIVE_ADMISSION_WAIT_MS_LAST.load(Ordering::Relaxed),
        PROOF_BACKLOG_COUNT.load(Ordering::Relaxed),
        PROOF_BACKLOG_OLDER_DAY.load(Ordering::Relaxed),
        PROOF_BACKLOG_MAX_AGE_SECONDS.load(Ordering::Relaxed),
        PROOF_MISSING_ACCEPTANCE_COUNT.load(Ordering::Relaxed),
        PROOF_MAX_RETRY_ATTEMPTS.load(Ordering::Relaxed),
        PROOF_NEWER_OVER_10M.load(Ordering::Relaxed),
        PROOF_NEWER_WAITING.load(Ordering::Relaxed),
        PROOF_HISTORIC_EXCLUDED.load(Ordering::Relaxed),
        PROOF_NEWER_MAX_ATTEMPTS.load(Ordering::Relaxed),
        PROOF_NEWER_DUE_FOR_RETRY.load(Ordering::Relaxed),
        BLOCK_TO_OUTBOX_MS_LAST.load(Ordering::Relaxed),
        BLOCK_TO_OUTBOX_MS_MAX.load(Ordering::Relaxed),
        BLOCK_TO_OUTBOX_SAMPLES.load(Ordering::Relaxed),
    ));

    rendered.push_str(&format!(
        concat!(
            "# TYPE kaspa_pulse_pnn_standby_qualified gauge\n",
            "kaspa_pulse_pnn_standby_qualified {}\n",
            "# TYPE kaspa_pulse_pnn_utxo_source_is_standby gauge\n",
            "kaspa_pulse_pnn_utxo_source_is_standby {}\n",
            "# TYPE kaspa_pulse_pnn_failover_transitions_total counter\n",
            "kaspa_pulse_pnn_failover_transitions_total {}\n",
            "# TYPE kaspa_pulse_pnn_forced_recoveries_total counter\n",
            "kaspa_pulse_pnn_forced_recoveries_total {}\n"
        ),
        PNN_STANDBY_QUALIFIED.load(Ordering::Relaxed),
        PNN_UTXO_SOURCE_IS_STANDBY.load(Ordering::Relaxed),
        PNN_FAILOVER_TRANSITIONS.load(Ordering::Relaxed),
        PNN_FORCED_RECOVERY_ATTEMPTS.load(Ordering::Relaxed),
    ));
    rendered.push_str(&format!(
        concat!(
            "# TYPE kaspa_pulse_pnn_pool_sources_qualified gauge\n",
            "kaspa_pulse_pnn_pool_sources_qualified {}\n",
            "# TYPE kaspa_pulse_pnn_pool_sources_eligible gauge\n",
            "kaspa_pulse_pnn_pool_sources_eligible {}\n",
            "# TYPE kaspa_pulse_pnn_pool_active_rank gauge\n",
            "kaspa_pulse_pnn_pool_active_rank {}\n",
            "# TYPE kaspa_pulse_pnn_pool_eligible_slots_mask gauge\n",
            "kaspa_pulse_pnn_pool_eligible_slots_mask {}\n",
            "# TYPE kaspa_pulse_pnn_source_generation gauge\n",
            "kaspa_pulse_pnn_source_generation {}\n",
            "# TYPE kaspa_pulse_pnn_dynamic_rotation_enabled gauge\n",
            "kaspa_pulse_pnn_dynamic_rotation_enabled {}\n",
            "# TYPE kaspa_pulse_pnn_spare_discovery_total counter\n",
            "kaspa_pulse_pnn_spare_discovery_total {}\n",
            "# TYPE kaspa_pulse_pnn_spare_swap_success_total counter\n",
            "kaspa_pulse_pnn_spare_swap_success_total {}\n",
            "# TYPE kaspa_pulse_pnn_spare_rejected_total counter\n",
            "kaspa_pulse_pnn_spare_rejected_total {}\n",
        ),
        PNN_POOL_SIZE.load(Ordering::Relaxed),
        PNN_POOL_ELIGIBLE.load(Ordering::Relaxed),
        PNN_POOL_ACTIVE_RANK.load(Ordering::Relaxed),
        PNN_POOL_ELIGIBLE_SLOTS_MASK.load(Ordering::Relaxed),
        PNN_SOURCE_GENERATION.load(Ordering::Relaxed),
        PNN_DYNAMIC_ROTATION_ENABLED.load(Ordering::Relaxed),
        PNN_SPARE_DISCOVERY_TOTAL.load(Ordering::Relaxed),
        PNN_SPARE_SWAP_SUCCESS_TOTAL.load(Ordering::Relaxed),
        PNN_SPARE_REJECTED_TOTAL.load(Ordering::Relaxed),
    ));
    rendered.push_str(&format!(
        concat!(
            "# TYPE kaspa_pulse_archive_fetch_http_429_total counter\n",
            "kaspa_pulse_archive_fetch_http_429_total {}\n",
            "# TYPE kaspa_pulse_archive_fetch_http_4xx_total counter\n",
            "kaspa_pulse_archive_fetch_http_4xx_total {}\n",
            "# TYPE kaspa_pulse_archive_fetch_http_5xx_total counter\n",
            "kaspa_pulse_archive_fetch_http_5xx_total {}\n",
            "# TYPE kaspa_pulse_archive_fetch_timeout_total counter\n",
            "kaspa_pulse_archive_fetch_timeout_total {}\n",
            "# TYPE kaspa_pulse_archive_fetch_invalid_json_total counter\n",
            "kaspa_pulse_archive_fetch_invalid_json_total {}\n",
            "# TYPE kaspa_pulse_archive_fetch_oversize_total counter\n",
            "kaspa_pulse_archive_fetch_oversize_total {}\n",
            "# TYPE kaspa_pulse_archive_fetch_other_total counter\n",
            "kaspa_pulse_archive_fetch_other_total {}\n"
        ),
        ARCHIVE_FETCH_HTTP_429.load(Ordering::Relaxed),
        ARCHIVE_FETCH_HTTP_4XX.load(Ordering::Relaxed),
        ARCHIVE_FETCH_HTTP_5XX.load(Ordering::Relaxed),
        ARCHIVE_FETCH_TIMEOUT.load(Ordering::Relaxed),
        ARCHIVE_FETCH_INVALID_JSON.load(Ordering::Relaxed),
        ARCHIVE_FETCH_OVERSIZE.load(Ordering::Relaxed),
        ARCHIVE_FETCH_TRANSPORT_OR_OTHER.load(Ordering::Relaxed)
    ));
    rendered.push_str(&format!(
        concat!(
            "# TYPE kaspa_pulse_archive_fetch_http_404_total counter\n",
            "kaspa_pulse_archive_fetch_http_404_total {}\n",
            "# TYPE kaspa_pulse_archive_fetch_http_4xx_other_total counter\n",
            "kaspa_pulse_archive_fetch_http_4xx_other_total {}\n",
            "# TYPE kaspa_pulse_archive_fetch_http_4xx_transaction_total counter\n",
            "kaspa_pulse_archive_fetch_http_4xx_transaction_total {}\n",
            "# TYPE kaspa_pulse_archive_fetch_http_4xx_block_total counter\n",
            "kaspa_pulse_archive_fetch_http_4xx_block_total {}\n",
            "# TYPE kaspa_pulse_archive_fetch_http_4xx_unknown_stage_total counter\n",
            "kaspa_pulse_archive_fetch_http_4xx_unknown_stage_total {}\n"
        ),
        ARCHIVE_FETCH_HTTP_404.load(Ordering::Relaxed),
        ARCHIVE_FETCH_HTTP_4XX_OTHER.load(Ordering::Relaxed),
        ARCHIVE_FETCH_HTTP_4XX_TX_META.load(Ordering::Relaxed),
        ARCHIVE_FETCH_HTTP_4XX_BLOCK.load(Ordering::Relaxed),
        ARCHIVE_FETCH_HTTP_4XX_UNKNOWN_STAGE.load(Ordering::Relaxed)
    ));
    // OP079 — fixed reason enums only; the archive and PNN totals remain
    // intact. No URLs, wallet, tx, subscriber or arbitrary failure labels.
    rendered.push_str(&format!(
        concat!(
            "# TYPE kaspa_pulse_archive_incomplete_transactionmetadata_total counter\n",
            "kaspa_pulse_archive_incomplete_transactionmetadata_total {}\n",
            "# TYPE kaspa_pulse_archive_incomplete_acceptingblock_total counter\n",
            "kaspa_pulse_archive_incomplete_acceptingblock_total {}\n",
            "# TYPE kaspa_pulse_archive_incomplete_acceptancerelation_total counter\n",
            "kaspa_pulse_archive_incomplete_acceptancerelation_total {}\n",
            "# TYPE kaspa_pulse_archive_incomplete_containingblock_total counter\n",
            "kaspa_pulse_archive_incomplete_containingblock_total {}\n",
            "# TYPE kaspa_pulse_archive_incomplete_walletscript_total counter\n",
            "kaspa_pulse_archive_incomplete_walletscript_total {}\n",
            "# TYPE kaspa_pulse_archive_incomplete_bluesetshape_total counter\n",
            "kaspa_pulse_archive_incomplete_bluesetshape_total {}\n",
            "# TYPE kaspa_pulse_archive_incomplete_minedblueblock_total counter\n",
            "kaspa_pulse_archive_incomplete_minedblueblock_total {}\n",
            "# TYPE kaspa_pulse_archive_incomplete_minerattribution_total counter\n",
            "kaspa_pulse_archive_incomplete_minerattribution_total {}\n",
            "# TYPE kaspa_pulse_pnn_spare_reject_discovery_unavailable_total counter\n",
            "kaspa_pulse_pnn_spare_reject_discovery_unavailable_total {}\n",
            "# TYPE kaspa_pulse_pnn_spare_reject_stale_or_recovered_total counter\n",
            "kaspa_pulse_pnn_spare_reject_stale_or_recovered_total {}\n",
            "# TYPE kaspa_pulse_pnn_spare_reject_coeval_unqualified_total counter\n",
            "kaspa_pulse_pnn_spare_reject_coeval_unqualified_total {}\n",
            "# TYPE kaspa_pulse_pnn_spare_reject_registry_fence_total counter\n",
            "kaspa_pulse_pnn_spare_reject_registry_fence_total {}\n",
        ),
        ARCHIVE_INCOMPLETE_TRANSACTIONMETADATA.load(Ordering::Relaxed),
        ARCHIVE_INCOMPLETE_ACCEPTINGBLOCK.load(Ordering::Relaxed),
        ARCHIVE_INCOMPLETE_ACCEPTANCERELATION.load(Ordering::Relaxed),
        ARCHIVE_INCOMPLETE_CONTAININGBLOCK.load(Ordering::Relaxed),
        ARCHIVE_INCOMPLETE_WALLETSCRIPT.load(Ordering::Relaxed),
        ARCHIVE_INCOMPLETE_BLUESETSHAPE.load(Ordering::Relaxed),
        ARCHIVE_INCOMPLETE_MINEDBLUEBLOCK.load(Ordering::Relaxed),
        ARCHIVE_INCOMPLETE_MINERATTRIBUTION.load(Ordering::Relaxed),
        PNN_REJECT_DISCOVERY_UNAVAILABLE.load(Ordering::Relaxed),
        PNN_REJECT_STALE_OR_RECOVERED.load(Ordering::Relaxed),
        PNN_REJECT_COEVAL_UNQUALIFIED.load(Ordering::Relaxed),
        PNN_REJECT_REGISTRY_FENCE.load(Ordering::Relaxed),
    ));
    // OP080: fixed partitions of the existing OP079 transactionmetadata
    // incomplete outcome; no dynamic identifiers, URLs or error messages.
    rendered.push_str(&format!(
        concat!(
            "# TYPE kaspa_pulse_archive_tx_metadata_not_accepted_total counter\n",
            "kaspa_pulse_archive_tx_metadata_not_accepted_total {}\n",
            "# TYPE kaspa_pulse_archive_tx_metadata_acceptance_flag_invalid_total counter\n",
            "kaspa_pulse_archive_tx_metadata_acceptance_flag_invalid_total {}\n",
            "# TYPE kaspa_pulse_archive_tx_metadata_transaction_id_mismatch_total counter\n",
            "kaspa_pulse_archive_tx_metadata_transaction_id_mismatch_total {}\n",
            "# TYPE kaspa_pulse_archive_tx_metadata_accepting_hash_missing_total counter\n",
            "kaspa_pulse_archive_tx_metadata_accepting_hash_missing_total {}\n",
            "# TYPE kaspa_pulse_archive_tx_metadata_accepting_hash_invalid_total counter\n",
            "kaspa_pulse_archive_tx_metadata_accepting_hash_invalid_total {}\n",
            "# TYPE kaspa_pulse_archive_tx_metadata_containing_block_not_unique_total counter\n",
            "kaspa_pulse_archive_tx_metadata_containing_block_not_unique_total {}\n",
            "# TYPE kaspa_pulse_archive_tx_metadata_containing_block_hash_invalid_total counter\n",
            "kaspa_pulse_archive_tx_metadata_containing_block_hash_invalid_total {}\n",
        ),
        ARCHIVE_TX_METADATA_NOT_ACCEPTED.load(Ordering::Relaxed),
        ARCHIVE_TX_METADATA_ACCEPTANCE_FLAG_INVALID.load(Ordering::Relaxed),
        ARCHIVE_TX_METADATA_TRANSACTION_ID_MISMATCH.load(Ordering::Relaxed),
        ARCHIVE_TX_METADATA_ACCEPTING_HASH_MISSING.load(Ordering::Relaxed),
        ARCHIVE_TX_METADATA_ACCEPTING_HASH_INVALID.load(Ordering::Relaxed),
        ARCHIVE_TX_METADATA_CONTAINING_BLOCK_NOT_UNIQUE.load(Ordering::Relaxed),
        ARCHIVE_TX_METADATA_CONTAINING_BLOCK_HASH_INVALID.load(Ordering::Relaxed),
    ));
    // OP081 static, bounded retry and final-404 role counters.
    rendered.push_str(&format!(
        concat!(
            "# TYPE kaspa_pulse_archive_block_404_retry_attempt_total counter\n",
            "kaspa_pulse_archive_block_404_retry_attempt_total {}\n",
            "# TYPE kaspa_pulse_archive_block_404_retry_recovered_total counter\n",
            "kaspa_pulse_archive_block_404_retry_recovered_total {}\n",
            "# TYPE kaspa_pulse_archive_block_404_retry_exhausted_total counter\n",
            "kaspa_pulse_archive_block_404_retry_exhausted_total {}\n",
            "# TYPE kaspa_pulse_archive_block_404_final_accepting_total counter\n",
            "kaspa_pulse_archive_block_404_final_accepting_total {}\n",
            "# TYPE kaspa_pulse_archive_block_404_final_containing_total counter\n",
            "kaspa_pulse_archive_block_404_final_containing_total {}\n",
            "# TYPE kaspa_pulse_archive_block_404_final_mined_blue_total counter\n",
            "kaspa_pulse_archive_block_404_final_mined_blue_total {}\n",
        ),
        ARCHIVE_BLOCK_404_RETRY_ATTEMPT.load(Ordering::Relaxed),
        ARCHIVE_BLOCK_404_RETRY_RECOVERED.load(Ordering::Relaxed),
        ARCHIVE_BLOCK_404_RETRY_EXHAUSTED.load(Ordering::Relaxed),
        ARCHIVE_BLOCK_404_FINAL_ACCEPTING.load(Ordering::Relaxed),
        ARCHIVE_BLOCK_404_FINAL_CONTAINING.load(Ordering::Relaxed),
        ARCHIVE_BLOCK_404_FINAL_MINED_BLUE.load(Ordering::Relaxed),
    ));
    rendered.push_str(&format!(
        concat!(
            "# TYPE kaspa_pulse_pnn_coinbase_accepting_containing_split_fallback_total counter\n",
            "kaspa_pulse_pnn_coinbase_accepting_containing_split_fallback_total {}\n",
            "# TYPE kaspa_pulse_pnn_coinbase_blue_mapping_fallback_total counter\n",
            "kaspa_pulse_pnn_coinbase_blue_mapping_fallback_total {}\n",
            "# TYPE kaspa_pulse_pnn_coinbase_native_blue_verified_total counter\n",
            "kaspa_pulse_pnn_coinbase_native_blue_verified_total {}\n",
            "# TYPE kaspa_pulse_pnn_coinbase_native_self_red_verified_total counter\n",
            "kaspa_pulse_pnn_coinbase_native_self_red_verified_total {}\n",
            "# TYPE kaspa_pulse_pnn_coinbase_native_no_miner_fallback_total counter\n",
            "kaspa_pulse_pnn_coinbase_native_no_miner_fallback_total {}\n",
        ),
        PNN_COINBASE_ACCEPTING_CONTAINING_SPLIT_FALLBACK.load(Ordering::Relaxed),
        PNN_COINBASE_BLUE_MAPPING_FALLBACK.load(Ordering::Relaxed),
        PNN_COINBASE_NATIVE_BLUE_VERIFIED.load(Ordering::Relaxed),
        PNN_COINBASE_NATIVE_SELF_RED_VERIFIED.load(Ordering::Relaxed),
        PNN_COINBASE_NATIVE_NO_MINER_FALLBACK.load(Ordering::Relaxed),
    ));
    rendered.push_str(&format!(
        concat!(
            "# TYPE kaspa_pulse_pnn_split_rest_complete_total counter\n",
            "kaspa_pulse_pnn_split_rest_complete_total {}\n",
            "# TYPE kaspa_pulse_pnn_split_rest_deferred_total counter\n",
            "kaspa_pulse_pnn_split_rest_deferred_total {}\n",
            "# TYPE kaspa_pulse_pnn_split_rest_cancelled_total counter\n",
            "kaspa_pulse_pnn_split_rest_cancelled_total {}\n",
            "# TYPE kaspa_pulse_pnn_split_rest_latency_ms_last gauge\n",
            "kaspa_pulse_pnn_split_rest_latency_ms_last {}\n",
            "# TYPE kaspa_pulse_pnn_split_rest_latency_ms_max gauge\n",
            "kaspa_pulse_pnn_split_rest_latency_ms_max {}\n",
        ),
        PNN_SPLIT_REST_COMPLETE_TOTAL.load(Ordering::Relaxed),
        PNN_SPLIT_REST_DEFERRED_TOTAL.load(Ordering::Relaxed),
        PNN_SPLIT_REST_CANCELLED_TOTAL.load(Ordering::Relaxed),
        PNN_SPLIT_REST_LATENCY_MS_LAST.load(Ordering::Relaxed),
        PNN_SPLIT_REST_LATENCY_MS_MAX.load(Ordering::Relaxed),
    ));
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

#[cfg(test)]
mod op070_pnn_admin_warning_gate_tests {
    use super::pnn_reward_source_fresh;
    #[test]
    fn only_fresh_successful_scans_suppress_primary_only_disconnect_notices() {
        assert!(pnn_reward_source_fresh(5, 2, 1000, 1005));
        assert!(!pnn_reward_source_fresh(5, 2, 1000, 1100));
        assert!(!pnn_reward_source_fresh(0, 0, 1000, 1005));
        assert!(!pnn_reward_source_fresh(5, 0, 1000, 1005));
        assert!(!pnn_reward_source_fresh(5, 1, 0, 1005));
        assert!(!pnn_reward_source_fresh(5, 1, 1100, 1000));
    }
}

#[cfg(test)]
mod op071_public_five_pool_quorum_tests {
    use super::public_five_pool_quorum;
    #[test]
    fn operator_required_five_way_pool_never_reports_ready_on_silent_primary_fallback() {
        assert!(!public_five_pool_quorum(0, 0, 0));
        assert!(!public_five_pool_quorum(0, 1, 1));
        assert!(!public_five_pool_quorum(4, 4, 2));
        assert!(!public_five_pool_quorum(5, 1, 1));
        assert!(!public_five_pool_quorum(5, 4, 0));
        assert!(!public_five_pool_quorum(5, 4, 6));
        assert!(public_five_pool_quorum(5, 2, 2));
        assert!(public_five_pool_quorum(5, 4, 4));
        assert!(public_five_pool_quorum(5, 5, 5));
    }
}

#[cfg(test)]
mod op077_public_rest_error_classes_tests {
    use super::{ArchiveFetchErrorClass as C, classify_archive_error};
    #[test]
    fn classifies_real_http_and_transport_failures_without_dynamic_labels() {
        assert_eq!(classify_archive_error("REST HTTP 429"), C::RateLimited);
        assert_eq!(classify_archive_error("REST HTTP 404"), C::Client4xx);
        assert_eq!(classify_archive_error("REST HTTP 503"), C::Server5xx);
        assert_eq!(classify_archive_error("request timed out"), C::Timeout);
        assert_eq!(classify_archive_error("Invalid REST JSON"), C::InvalidJson);
        assert_eq!(
            classify_archive_error("REST response exceeds size limit"),
            C::Oversize
        );
        assert_eq!(
            classify_archive_error("REST response chunk exceeds size limit"),
            C::Oversize
        );
        assert_eq!(
            classify_archive_error("connection reset by peer"),
            C::TransportOrOther
        );
    }
    #[test]
    fn error_classes_are_independent_of_transaction_or_wallet_identifiers() {
        let tx = "a".repeat(64);
        let message = format!("request timeout for https://api.kaspa.org/transactions/{tx}");
        assert_eq!(classify_archive_error(&message), C::Timeout);
        let rendered = super::render_metrics();
        assert!(!rendered.contains(&tx));
        assert!(!rendered.contains("kaspa:"));
        assert!(rendered.contains("kaspa_pulse_archive_fetch_http_429_total"));
        assert!(rendered.contains("kaspa_pulse_archive_fetch_other_total"));
    }
}

#[cfg(test)]
mod op078_privacy_safe_4xx_stage_tests {
    use super::{
        ArchiveFetchErrorClass, ArchiveFetchStage, classify_archive_error,
        classify_archive_http_4xx_details, record_archive_fetch_error_classified, render_metrics,
    };
    #[test]
    fn differentiates_http_404_from_other_http_4xx_without_changing_429_or_5xx() {
        assert_eq!(
            classify_archive_http_4xx_details("REST HTTP 404 [archive_stage=transaction]"),
            (true, ArchiveFetchStage::Transaction)
        );
        assert_eq!(
            classify_archive_http_4xx_details("REST HTTP 403 Forbidden [archive_stage=block]"),
            (false, ArchiveFetchStage::Block)
        );
        assert_eq!(
            classify_archive_http_4xx_details("REST HTTP 400"),
            (false, ArchiveFetchStage::Unknown)
        );
        assert_eq!(
            classify_archive_error("REST HTTP 429 [archive_stage=block]"),
            ArchiveFetchErrorClass::RateLimited
        );
        assert_eq!(
            classify_archive_error("REST HTTP 503 [archive_stage=transaction]"),
            ArchiveFetchErrorClass::Server5xx
        );
    }
    #[test]
    fn only_trusted_fixed_stage_suffix_counts_as_a_known_archive_stage() {
        assert_eq!(
            classify_archive_http_4xx_details(
                "REST HTTP 404 [archive_stage=transaction] malicious-trailing-text"
            ),
            (true, ArchiveFetchStage::Unknown)
        );
        assert_eq!(
            classify_archive_http_4xx_details("REST HTTP 4040 [archive_stage=block]"),
            (false, ArchiveFetchStage::Block)
        );
        assert_eq!(
            classify_archive_http_4xx_details("REST HTTP 404"),
            (true, ArchiveFetchStage::Unknown)
        );
    }
    #[test]
    fn classification_never_exposes_identifiers_or_raw_error_bodies_in_metrics() {
        let private_tx = "b".repeat(64);
        let msg = format!("REST HTTP 404 [archive_stage=transaction] {private_tx}");
        record_archive_fetch_error_classified(&msg);
        let rendered = render_metrics();
        assert!(!rendered.contains(&private_tx));
        assert!(!rendered.contains("archive_stage="));
        assert!(!rendered.contains("kaspa:"));
        for name in [
            "archive_fetch_http_404_total",
            "archive_fetch_http_4xx_other_total",
            "archive_fetch_http_4xx_transaction_total",
            "archive_fetch_http_4xx_block_total",
            "archive_fetch_http_4xx_unknown_stage_total",
        ] {
            assert!(rendered.contains(name), "missing static counter: {name}");
        }
    }
}

#[cfg(test)]
mod op079_reason_telemetry_contract_tests {
    use super::*;
    #[test]
    fn all_archive_incomplete_reason_variants_have_static_metrics() {
        let rendered = render_metrics();
        for n in [
            "transactionmetadata",
            "acceptingblock",
            "acceptancerelation",
            "containingblock",
            "walletscript",
            "bluesetshape",
            "minedblueblock",
            "minerattribution",
        ] {
            assert!(rendered.contains(&format!("kaspa_pulse_archive_incomplete_{n}_total")));
        }
        for n in [
            "discovery_unavailable",
            "stale_or_recovered",
            "coeval_unqualified",
            "registry_fence",
        ] {
            assert!(rendered.contains(&format!("kaspa_pulse_pnn_spare_reject_{n}_total")));
        }
        assert!(!rendered.contains("kaspa:"));
        assert!(!rendered.contains("archive_stage="));
    }
    #[test]
    fn archive_rejection_tracks_one_aggregate_and_one_specific_reason() {
        let before_total = ARCHIVE_INCOMPLETE_TOTAL.load(Ordering::Relaxed);
        let before_specific = ARCHIVE_INCOMPLETE_MINERATTRIBUTION.load(Ordering::Relaxed);
        record_archive_incomplete(ArchiveIncompleteReason::MinerAttribution);
        assert_eq!(
            ARCHIVE_INCOMPLETE_MINERATTRIBUTION.load(Ordering::Relaxed),
            before_specific + 1
        );
        assert!(ARCHIVE_INCOMPLETE_TOTAL.load(Ordering::Relaxed) > before_total);
    }
    #[test]
    fn pnn_rejection_tracks_one_total_and_exact_one_static_category() {
        let before_total = PNN_SPARE_REJECTED_TOTAL.load(Ordering::Relaxed);
        let before_specific = PNN_REJECT_REGISTRY_FENCE.load(Ordering::Relaxed);
        inc_pnn_spare_rejected(PnnSpareRejectReason::RegistryFence);
        assert_eq!(
            PNN_REJECT_REGISTRY_FENCE.load(Ordering::Relaxed),
            before_specific + 1
        );
        assert!(PNN_SPARE_REJECTED_TOTAL.load(Ordering::Relaxed) > before_total);
    }
}

#[cfg(test)]
mod op080_static_metadata_metric_contract_tests {
    use super::*;
    #[test]
    fn all_reasons_are_fixed_and_do_not_reveal_identifiers() {
        let text = render_metrics();
        for name in [
            "not_accepted",
            "acceptance_flag_invalid",
            "transaction_id_mismatch",
            "accepting_hash_missing",
            "accepting_hash_invalid",
            "containing_block_not_unique",
            "containing_block_hash_invalid",
        ] {
            assert!(text.contains(&format!("kaspa_pulse_archive_tx_metadata_{name}_total")));
        }
        assert!(!text.contains("kaspa:"));
        assert!(!text.contains("archive_stage="));
    }
}

#[cfg(test)]
mod op081_block_404_static_metric_contract_tests {
    use super::*;
    #[test]
    fn all_bounded_retry_outcome_and_role_metrics_have_fixed_names() {
        let rendered = render_metrics();
        for k in [
            "retry_attempt",
            "retry_recovered",
            "retry_exhausted",
            "final_accepting",
            "final_containing",
            "final_mined_blue",
        ] {
            assert!(rendered.contains(&format!("kaspa_pulse_archive_block_404_{k}_total")));
        }
        assert!(!rendered.contains("kaspa:"));
        assert!(!rendered.contains("archive_stage="));
    }
    #[test]
    fn archive_block_role_increments_only_the_fixed_counter() {
        let old = ARCHIVE_BLOCK_404_FINAL_ACCEPTING.load(Ordering::Relaxed);
        record_archive_block_final_404(ArchiveBlock404Role::Accepting);
        assert_eq!(
            ARCHIVE_BLOCK_404_FINAL_ACCEPTING.load(Ordering::Relaxed),
            old + 1
        );
    }
}

#[cfg(test)]
mod op084_privacy_safe_pnn_route_tests {
    use super::*;
    #[test]
    fn fixed_pnn_routes_have_no_wallet_transaction_or_hash_labels() {
        let text = render_metrics();
        for name in [
            "accepting_containing_split_fallback",
            "blue_mapping_fallback",
            "native_blue_verified",
            "native_self_red_verified",
            "native_no_miner_fallback",
        ] {
            assert!(text.contains(&format!("kaspa_pulse_pnn_coinbase_{name}_total")));
        }
        assert!(!text.contains("kaspa:"));
        assert!(!text.contains("txid="));
        assert!(!text.contains("wallet="));
    }
}

#[cfg(test)]
mod op085_private_split_latency_metrics_tests {
    use super::*;
    #[test]
    fn split_latency_counters_record_bounded_outcomes_without_identifiers() {
        let initial = PNN_SPLIT_REST_COMPLETE_TOTAL.load(Ordering::Relaxed);
        observe_pnn_split_rest_latency(100, Some(true));
        assert!(PNN_SPLIT_REST_COMPLETE_TOTAL.load(Ordering::Relaxed) > initial);
        observe_pnn_split_rest_latency(200, Some(false));
        observe_pnn_split_rest_latency(300, None);
        assert!(PNN_SPLIT_REST_LATENCY_MS_MAX.load(Ordering::Relaxed) >= 300);
        let metrics = render_metrics();
        for kind in [
            "complete_total",
            "deferred_total",
            "cancelled_total",
            "latency_ms_last",
            "latency_ms_max",
        ] {
            assert!(metrics.contains(&format!("kaspa_pulse_pnn_split_rest_{kind}")));
        }
        assert!(!metrics.contains("kaspa:"));
        assert!(!metrics.contains("txid="));
        assert!(!metrics.contains("wallet="));
    }
}
