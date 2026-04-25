use kaspa_addresses::Address;
use kaspa_rpc_core::api::rpc::RpcApi;
use std::collections::HashSet;
use crate::domain::models::AppContext;

pub async fn scan_wallet_for_new_utxos(
    ctx: &AppContext,
    wallet: &str,
) -> Option<Vec<(String, String, i64, u64, bool)>> {
    let addr = Address::try_from(wallet).ok()?;
    let utxos = ctx.rpc.get_utxos_by_addresses(vec![addr]).await.ok()?;

    let mut current_outpoints = HashSet::new();
    let mut new_rewards = Vec::new();

    let mut known = ctx.utxo_state.entry(wallet.to_string()).or_insert_with(HashSet::new);
    let is_first_run = known.is_empty();

    for entry in utxos {
        let tx_id = entry.outpoint.transaction_id.to_string();
        let outpoint_id = format!("{}:{}", tx_id, entry.outpoint.index);
        current_outpoints.insert(outpoint_id.clone());

        if !is_first_run && !known.contains(&outpoint_id) {
            new_rewards.push((
                outpoint_id.clone(),
                tx_id,
                entry.utxo_entry.amount as i64,
                entry.utxo_entry.block_daa_score,
                entry.utxo_entry.is_coinbase,
            ));
            known.insert(outpoint_id);
        } else if is_first_run {
            known.insert(outpoint_id);
        }
    }

    known.retain(|k| current_outpoints.contains(k));

    if new_rewards.is_empty() {
        None
    } else {
        Some(new_rewards)
    }
}

