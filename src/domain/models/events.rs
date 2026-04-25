#[derive(Debug, Clone)]
pub struct LiveBlockEvent {
    pub is_coinbase: bool,
    pub wallet_address: String,
    pub amount_kas: f64,
    pub live_balance_kas: f64,
    pub tx_id: String,
    pub block_time_ms: u64,
    pub acc_block_hash: String,
    pub mined_block_hash: Option<String>,
    pub extracted_worker: Option<String>,
    pub daa_score: u64,
}
