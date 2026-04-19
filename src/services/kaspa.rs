use kaspa_addresses::Address;
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_wrpc_client::KaspaRpcClient;
use std::sync::Arc;

/// Enterprise Service Layer for Kaspa Node Operations
pub struct KaspaNodeService;

impl KaspaNodeService {
    /// Fetches the live balance and UTXO count for a given wallet address.
    /// This abstracts the complex UTXO aggregation logic away from the UI handlers.
    pub async fn get_balance(rpc: &Arc<KaspaRpcClient>, wallet_str: &str) -> Result<(f64, usize), String> {
        let addr = Address::try_from(wallet_str).map_err(|_| "Invalid address format".to_string())?;
        
        let utxos = rpc.get_utxos_by_addresses(vec![addr])
            .await
            .map_err(|e| format!("Node RPC Error: {}", e))?;
        
        let balance = utxos.iter()
            .map(|u| u.utxo_entry.amount as f64)
            .sum::<f64>() / 1e8;
            
        Ok((balance, utxos.len()))
    }
}
