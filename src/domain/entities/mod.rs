#[derive(Debug, Clone)]
pub struct TrackedWallet {
    pub address: String,
    pub chat_id: i64,
}

#[derive(Debug, Clone)]
pub struct MinedBlock {
    pub wallet_address: String,
    pub outpoint: String,
    pub amount: i64,
    pub daa_score: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalletRemovalOutcome {
    Removed,
    NotFound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserDataDeletionSummary {
    pub wallets_deleted: u64,
    pub event_rows_deleted: u64,
    pub chat_history_rows_deleted: u64,
    pub queue_rows_deleted: u64,
    pub admin_audit_rows_anonymized: u64,
    pub orphan_wallets_cleaned: usize,
    pub orphan_wallet_addresses: Vec<String>,
}
