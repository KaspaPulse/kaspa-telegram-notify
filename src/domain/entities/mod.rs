// ==========================================
// --- Merged from news.rs ---
// ==========================================

#[derive(Debug, Clone)]
pub struct NewsItem {
    pub title: String,
    pub link: String,
    pub content: String,
    pub source: String,
}

// ==========================================
// --- Merged from wallet.rs ---
// ==========================================

/// Pure Domain Entity for a Tracked Wallet
#[derive(Debug, Clone)]
pub struct TrackedWallet {
    pub address: String,
    pub chat_id: i64,
}

/// Pure Domain Entity for a Mined Block
#[derive(Debug, Clone)]
pub struct MinedBlock {
    pub wallet_address: String,
    pub outpoint: String,
    pub amount: i64,
    pub daa_score: u64,
}
