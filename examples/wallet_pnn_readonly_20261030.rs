// Live, read-only verification of every distinct tracked wallet using PNN/Borsh.
// No Telegram, no writes to the database, no wallet addresses in output.
use kaspa_addresses::Address;
use kaspa_consensus_core::network::NetworkId;
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_wrpc_client::client::ConnectOptions;
use kaspa_wrpc_client::{KaspaRpcClient, Resolver, WrpcEncoding};
use std::str::FromStr;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let db_url = "postgresql://abuha@localhost:5549/kaspa_block_alert_bot?host=/run/kaspa-block-alert-bot-postgresql";
    let pool = sqlx::PgPool::connect(db_url).await?;
    let wallets: Vec<String> =
        sqlx::query_scalar("SELECT DISTINCT wallet FROM public.user_wallets WHERE is_active IS DISTINCT FROM FALSE ORDER BY wallet")
            .fetch_all(&pool).await?;
    println!("TRACKED_DISTINCT_WALLETS={}", wallets.len());
    anyhow::ensure!(!wallets.is_empty(), "no wallets to scan");
    let network = NetworkId::from_str("mainnet")?;
    let rpc = KaspaRpcClient::new(
        WrpcEncoding::Borsh,
        None,
        Some(Resolver::default()),
        Some(network),
        None,
    )?;
    tokio::time::timeout(
        Duration::from_secs(25),
        rpc.connect(Some(ConnectOptions::blocking_fallback())),
    )
    .await??;
    let info = tokio::time::timeout(Duration::from_secs(12), rpc.get_server_info()).await??;
    println!(
        "PNN_NETWORK={} SYNC={} UTXO_INDEX={}",
        info.network_id, info.is_synced, info.has_utxo_index
    );
    anyhow::ensure!(
        info.network_id.to_string() == "mainnet" && info.is_synced && info.has_utxo_index,
        "PNN not suitable for wallet scan"
    );
    let mut ok = 0_usize;
    for (i, wallet) in wallets.iter().enumerate() {
        let addr = Address::try_from(wallet.as_str())?;
        match tokio::time::timeout(
            Duration::from_secs(25),
            rpc.get_utxos_by_addresses(vec![addr]),
        )
        .await
        {
            Ok(Ok(entries)) => {
                let coinbase = entries.iter().filter(|e| e.utxo_entry.is_coinbase).count();
                println!(
                    "WALLET_{}=PASS UTXOS={} COINBASE_UTXOS={}",
                    i + 1,
                    entries.len(),
                    coinbase
                );
                ok += 1;
            }
            Ok(Err(err)) => println!(
                "WALLET_{}=RPC_ERROR:{}",
                i + 1,
                err.to_string().chars().take(120).collect::<String>()
            ),
            Err(_) => println!("WALLET_{}=TIMEOUT_25S", i + 1),
        }
    }
    println!("PNN_LIVE_WALLET_READ_PASS={}/{}", ok, wallets.len());
    anyhow::ensure!(
        ok == wallets.len(),
        "some wallets failed read-only PNN UTXO checks"
    );
    pool.close().await;
    Ok(())
}
