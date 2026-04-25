pub mod context;
pub mod events;

pub use context::*;
pub use events::*;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtxoRecord {
    pub address: String,
    pub amount: u64,
    pub script_public_key: String,
    pub outpoint: String,
    pub block_daa_score: u64,
    pub is_coinbase: bool,
    pub transaction_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockData {
    pub hash: String,
    pub blue_score: u64,
    pub timestamp: u64,
    pub daa_score: u64,
    pub parents: Vec<String>,
    pub transaction_ids: Vec<String>,
}
