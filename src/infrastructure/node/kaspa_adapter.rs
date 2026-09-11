use crate::domain::errors::AppError;
use kaspa_addresses::Address;
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_wrpc_client::KaspaRpcClient;
use std::str::FromStr;
use std::sync::Arc;

pub struct KaspaRpcAdapter {
    pub client: Arc<KaspaRpcClient>,
}

impl KaspaRpcAdapter {
    pub fn new(client: Arc<KaspaRpcClient>) -> Self {
        Self { client }
    }

    pub async fn get_utxos(
        &self,
        address: &str,
    ) -> Result<Vec<crate::domain::models::UtxoRecord>, AppError> {
        let addr = Address::try_from(address).map_err(|_| "Invalid address format".to_string())?;

        let utxos = self
            .client
            .get_utxos_by_addresses(vec![addr])
            .await
            .map_err(|e| AppError::NodeConnection(e.to_string()))?;

        let records = utxos
            .into_iter()
            .map(|u| crate::domain::models::UtxoRecord {
                outpoint: format!("{}:{}", u.outpoint.transaction_id, u.outpoint.index),
                transaction_id: u.outpoint.transaction_id.to_string(),
                amount: u.utxo_entry.amount,
                address: address.to_string(),
                script_public_key: format!("{:?}", u.utxo_entry.script_public_key),
                block_daa_score: u.utxo_entry.block_daa_score,
                is_coinbase: u.utxo_entry.is_coinbase,
            })
            .collect();

        Ok(records)
    }

    pub async fn get_balance(&self, address: &str) -> Result<(u64, usize), AppError> {
        let addr = Address::try_from(address).map_err(|_| "Invalid address format".to_string())?;

        let utxos = self
            .client
            .get_utxos_by_addresses(vec![addr])
            .await
            .map_err(|e| AppError::NodeConnection(e.to_string()))?;

        let balance = utxos.iter().map(|u| u.utxo_entry.amount).sum::<u64>();
        Ok((balance, utxos.len()))
    }

    pub async fn get_network_hashrate(&self) -> Result<f64, AppError> {
        let hashrate = self
            .client
            .estimate_network_hashes_per_second(1000, None)
            .await
            .map_err(|e| AppError::NodeConnection(e.to_string()))?;

        Ok(hashrate as f64)
    }

    pub async fn get_node_health(&self) -> Result<(bool, usize), AppError> {
        let mut is_online = self.client.get_server_info().await.is_ok();

        if !is_online {
            tracing::warn!("[NODE ADAPTER] Node offline. Attempting reconnect.");
            let _ = self.client.connect(None).await;
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            is_online = self.client.get_server_info().await.is_ok();
        }

        if !is_online {
            return Ok((false, 0));
        }

        let peer_count = self
            .client
            .get_connected_peer_info()
            .await
            .map(|p| p.peer_info.len())
            .map_err(|error| AppError::NodeConnection(error.to_string()))?;

        Ok((true, peer_count))
    }

    pub async fn get_block(
        &self,
        hash_str: &str,
    ) -> Result<crate::domain::models::BlockData, AppError> {
        let hash = kaspa_hashes::Hash::from_str(hash_str)
            .map_err(|_| "Invalid hash format".to_string())?;

        let block = self
            .client
            .get_block(hash, true)
            .await
            .map_err(|e| AppError::NodeConnection(e.to_string()))?;

        let tx_ids = block
            .transactions
            .iter()
            .filter_map(|tx| {
                tx.verbose_data
                    .as_ref()
                    .map(|v| v.transaction_id.to_string())
            })
            .collect();

        let parents: Vec<String> = block
            .header
            .parents_by_level
            .first()
            .map(|level| level.iter().map(|p| p.to_string()).collect())
            .unwrap_or_default();

        Ok(crate::domain::models::BlockData {
            hash: hash_str.to_string(),
            blue_score: block.header.blue_score,
            daa_score: block.header.daa_score,
            timestamp: block.header.timestamp,
            parents,
            transaction_ids: tx_ids,
        })
    }

    pub async fn get_pruning_point(&self) -> Result<String, AppError> {
        let info = self
            .client
            .get_block_dag_info()
            .await
            .map_err(|e| AppError::NodeConnection(e.to_string()))?;

        Ok(info.pruning_point_hash.to_string())
    }

    pub async fn get_virtual_daa_score(&self) -> Result<u64, AppError> {
        let info = self
            .client
            .get_block_dag_info()
            .await
            .map_err(|e| AppError::NodeConnection(e.to_string()))?;

        Ok(info.virtual_daa_score)
    }

    pub async fn get_server_info(&self) -> Result<kaspa_rpc_core::GetServerInfoResponse, AppError> {
        crate::infrastructure::resilience::runtime::with_rpc_timeout("get_server_info", async {
            self.client
                .get_server_info()
                .await
                .map_err(|e| AppError::NodeError(e.to_string()))
        })
        .await
    }

    pub async fn get_sync_status(&self) -> Result<bool, AppError> {
        crate::infrastructure::resilience::runtime::with_rpc_timeout("get_sync_status", async {
            self.client
                .get_sync_status()
                .await
                .map_err(|e| AppError::NodeError(e.to_string()))
        })
        .await
    }

    pub async fn get_block_dag_info(
        &self,
    ) -> Result<kaspa_rpc_core::GetBlockDagInfoResponse, AppError> {
        crate::infrastructure::resilience::runtime::with_rpc_timeout("get_block_dag_info", async {
            self.client
                .get_block_dag_info()
                .await
                .map_err(|e| AppError::NodeError(e.to_string()))
        })
        .await
    }

    pub async fn get_coin_supply(&self) -> Result<kaspa_rpc_core::GetCoinSupplyResponse, AppError> {
        crate::infrastructure::resilience::runtime::with_rpc_timeout("get_coin_supply", async {
            self.client
                .get_coin_supply()
                .await
                .map_err(|e| AppError::NodeError(e.to_string()))
        })
        .await
    }

    pub async fn get_utxos_by_addresses(
        &self,
        addrs: Vec<String>,
    ) -> Result<Vec<kaspa_rpc_core::RpcUtxosByAddressesEntry>, AppError> {
        let addresses = addrs
            .into_iter()
            .map(|a| {
                kaspa_addresses::Address::try_from(a.as_str()).map_err(|_| {
                    AppError::NodeError(format!(
                        "Invalid Kaspa address passed to get_utxos_by_addresses: {}",
                        crate::utils::format_short_wallet(&a)
                    ))
                })
            })
            .collect::<Result<Vec<_>, AppError>>()?;

        crate::infrastructure::resilience::runtime::with_rpc_timeout(
            "get_utxos_by_addresses",
            async {
                self.client
                    .get_utxos_by_addresses(addresses)
                    .await
                    .map_err(|e| AppError::NodeError(e.to_string()))
            },
        )
        .await
    }

    pub async fn connect(&self, block: bool) -> Result<(), AppError> {
        let options = if block {
            Some(kaspa_wrpc_client::client::ConnectOptions::default())
        } else {
            None
        };

        crate::infrastructure::resilience::runtime::with_rpc_timeout("connect", async {
            self.client
                .connect(options)
                .await
                .map_err(|e| AppError::NodeConnection(e.to_string()))?;

            Ok(())
        })
        .await
    }
}
