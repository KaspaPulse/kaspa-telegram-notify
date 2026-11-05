// Retained as a small characterization helper for the integration test suite.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertIdentity {
    pub wallet: String,
    pub alert_key: String,
}

pub fn build_alert_key(mined_block_hash: Option<&str>, transaction_id: &str) -> String {
    mined_block_hash
        .filter(|hash| !hash.trim().is_empty())
        .unwrap_or(transaction_id)
        .to_string()
}

#[allow(dead_code)]
pub fn build_alert_identity(
    wallet: &str,
    mined_block_hash: Option<&str>,
    transaction_id: &str,
) -> AlertIdentity {
    AlertIdentity {
        wallet: wallet.to_string(),
        alert_key: build_alert_key(mined_block_hash, transaction_id),
    }
}
