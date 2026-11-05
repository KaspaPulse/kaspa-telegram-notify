//! Bounded public REST fallback for *historical* Coinbase block attribution.
//! Accepting block != containing block != mined blue block. Nothing is fabricated.
use serde_json::Value;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Semaphore};

const BASE: &str = "https://api.kaspa.org";
const MAX_RESPONSE_BYTES: usize = 1_048_576;
const MAX_MERGE_SET_BLUES: usize = 32;
static HTTP: OnceLock<reqwest::Client> = OnceLock::new();
static GATE: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
static PROOF_GATE: OnceLock<Semaphore> = OnceLock::new();
static LAST_PROOF_START: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoricalCoinbaseProof {
    pub accepting_hash: String,
    pub containing_hash: String,
    pub mined_hashes: Vec<String>,
    pub nonce: String,
    pub worker: String,
    pub block_time_ms: u64,
}

fn valid_hash(v: &str) -> bool {
    v.len() == 64 && v.bytes().all(|c| c.is_ascii_hexdigit())
}

fn decode_hex_limited(s: &str, limit: usize) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || s.len() / 2 > limit {
        return None;
    }
    s.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let a = (pair[0] as char).to_digit(16)? as u8;
            let b = (pair[1] as char).to_digit(16)? as u8;
            Some((a << 4) | b)
        })
        .collect()
}

fn block_hash_matches(block: &Value, expected: &str) -> bool {
    block["verboseData"]["hash"]
        .as_str()
        .is_some_and(|value| valid_hash(value) && value.eq_ignore_ascii_case(expected))
}

fn coinbase_txid_matches(block: &Value, txid: &str) -> bool {
    block["transactions"][0]["verboseData"]["transactionId"]
        .as_str()
        .is_some_and(|id| valid_hash(id) && id.eq_ignore_ascii_case(txid))
}

fn wallet_script(block: &Value, wallet: &str) -> Option<Vec<u8>> {
    let outputs = block["transactions"][0]["outputs"].as_array()?;
    let mut matches = outputs
        .iter()
        .filter(|out| out["verboseData"]["scriptPublicKeyAddress"].as_str() == Some(wallet));
    let matched = matches.next()?;
    if matches.next().is_some() {
        // Ambiguous scripts for the same wallet: do not guess.
        return None;
    }
    let code = matched["scriptPublicKey"]["scriptPublicKey"].as_str()?;
    let decoded = decode_hex_limited(code, 512)?;
    if decoded.is_empty() {
        return None;
    }
    Some(decoded)
}

fn json_u64(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}

fn mine_match(block: &Value, script: &[u8]) -> Option<(String, String, u64)> {
    if script.is_empty() {
        return None;
    }
    let payload_hex = block["transactions"][0]["payload"].as_str()?;
    let bytes = decode_hex_limited(payload_hex, 8192)?;
    let position = bytes.windows(script.len()).position(|w| w == script)?;
    let metadata = &bytes[position + script.len()..];
    let worker: String = metadata
        .iter()
        .copied()
        .filter(|b| (32..=126).contains(b))
        .take(90)
        .map(char::from)
        .collect();
    // Public Kaspa REST may serialize u64 quantities as strings to avoid
    // JavaScript integer precision loss. Both representations are verified.
    let nonce = json_u64(&block["header"]["nonce"])?;
    let timestamp = json_u64(&block["header"]["timestamp"])?;
    if worker.is_empty() || timestamp == 0 {
        return None;
    }
    Some((nonce.to_string(), worker, timestamp))
}

fn validate_transaction_metadata(meta: &Value, txid: &str) -> Option<(String, String)> {
    if !meta["is_accepted"].as_bool()?
        || !meta["transaction_id"].as_str()?.eq_ignore_ascii_case(txid)
    {
        return None;
    }
    let accepting = meta["accepting_block_hash"].as_str()?;
    let containing = meta["block_hash"].as_array()?;
    if containing.len() != 1 || !valid_hash(accepting) {
        return None;
    }
    let containing = containing.first()?.as_str()?;
    if !valid_hash(containing) {
        return None;
    }
    Some((
        accepting.to_ascii_lowercase(),
        containing.to_ascii_lowercase(),
    ))
}

async fn get_json(path: &str) -> Result<Value, String> {
    let gate = GATE.get_or_init(|| Mutex::new(None));
    let mut last = gate.lock().await;
    if let Some(instant) = *last {
        let pause = Duration::from_millis(300).saturating_sub(instant.elapsed());
        if !pause.is_zero() {
            tokio::time::sleep(pause).await;
        }
    }
    *last = Some(Instant::now());
    drop(last);

    let client = HTTP.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent("KaspaBlockAlertHistoricalProof/1")
            .connect_timeout(Duration::from_secs(4))
            .timeout(Duration::from_secs(8))
            .build()
            .expect("HTTPS client builder is static and valid")
    });
    let url = format!("{BASE}{path}");
    let response = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("REST HTTP {}", response.status()));
    }
    if response
        .content_length()
        .is_some_and(|len| len > MAX_RESPONSE_BYTES as u64)
    {
        return Err("REST response exceeds size limit".to_string());
    }
    let mut response = response;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err("REST response chunk exceeds size limit".to_string());
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| "Invalid REST JSON".to_string())
}

/// Public REST supplies historical data where a PNN has pruned its DAG.
/// This is a candidate proof; the calling alert gate still must verify each
/// actual mined hash, nonce, worker and timestamp before delivery.
pub async fn historical_coinbase_proof(
    txid: &str,
    wallet: &str,
) -> Result<Option<HistoricalCoinbaseProof>, String> {
    if !valid_hash(txid) || !wallet.starts_with("kaspa:") {
        return Ok(None);
    }
    // At most one full historical lookup at a time. A busy lookup defers
    // without claiming an unverifiable mining event.
    let Ok(_proof_permit) = PROOF_GATE.get_or_init(|| Semaphore::new(1)).try_acquire() else {
        return Ok(None);
    };
    let mut last_start = LAST_PROOF_START
        .get_or_init(|| Mutex::new(None))
        .lock()
        .await;
    if last_start.is_some_and(|at| at.elapsed() < Duration::from_secs(30)) {
        return Ok(None);
    }
    *last_start = Some(Instant::now());
    drop(last_start);
    let meta = get_json(&format!("/transactions/{txid}?inputs=false&outputs=false")).await?;
    let Some((accepting_hash, containing_hash)) = validate_transaction_metadata(&meta, txid) else {
        return Ok(None);
    };
    let accepting = get_json(&format!(
        "/blocks/{accepting_hash}?includeTransactions=false"
    ))
    .await?;
    if !block_hash_matches(&accepting, &accepting_hash) {
        return Ok(None);
    }
    let Some(accepted_blues) = accepting["verboseData"]["mergeSetBluesHashes"].as_array() else {
        return Ok(None);
    };
    if !accepted_blues.iter().any(|v| {
        v.as_str()
            .is_some_and(|hash| hash.eq_ignore_ascii_case(&containing_hash))
    }) {
        return Ok(None);
    }

    let containing = get_json(&format!(
        "/blocks/{containing_hash}?includeTransactions=true"
    ))
    .await?;
    if !block_hash_matches(&containing, &containing_hash)
        || !coinbase_txid_matches(&containing, txid)
    {
        return Ok(None);
    }
    let Some(script) = wallet_script(&containing, wallet) else {
        return Ok(None);
    };
    let Some(blues) = containing["verboseData"]["mergeSetBluesHashes"].as_array() else {
        return Ok(None);
    };
    if blues.is_empty() || blues.len() > MAX_MERGE_SET_BLUES {
        return Ok(None);
    }
    let mut mined_hashes = Vec::new();
    let mut nonce = String::new();
    let mut worker = String::new();
    let mut block_time_ms = 0;
    for blue in blues {
        let Some(hash) = blue.as_str().filter(|s| valid_hash(s)) else {
            return Ok(None);
        };
        let block = get_json(&format!("/blocks/{hash}?includeTransactions=true")).await?;
        if !block_hash_matches(&block, hash) {
            return Ok(None);
        }
        if let Some((found_nonce, found_worker, timestamp)) = mine_match(&block, &script) {
            mined_hashes.push(hash.to_ascii_lowercase());
            if nonce.is_empty() {
                nonce = found_nonce;
                worker = found_worker;
                block_time_ms = timestamp;
            }
        }
    }
    if mined_hashes.is_empty() {
        return Ok(None);
    }
    Ok(Some(HistoricalCoinbaseProof {
        accepting_hash,
        containing_hash,
        mined_hashes,
        nonce,
        worker,
        block_time_ms,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tx_metadata_keeps_accepting_distinct_from_containing() {
        let a = "a".repeat(64);
        let c = "b".repeat(64);
        let tx = "c".repeat(64);
        let v = serde_json::json!({"is_accepted":true,"transaction_id":tx,
            "accepting_block_hash":a,"block_hash":[c]});
        assert_eq!(validate_transaction_metadata(&v, &tx), Some((a, c)));
        assert_eq!(validate_transaction_metadata(&v, &"d".repeat(64)), None);
    }
    #[test]
    fn refuses_false_acceptance_or_multiple_containing_blocks() {
        let tx = "a".repeat(64);
        let hash = "b".repeat(64);
        let multi = serde_json::json!({"is_accepted":true,"transaction_id":tx,
            "accepting_block_hash":hash,"block_hash":[hash,hash]});
        assert_eq!(validate_transaction_metadata(&multi, &tx), None);
        let unaccepted = serde_json::json!({"is_accepted":false,"transaction_id":tx,
            "accepting_block_hash":hash,"block_hash":[hash]});
        assert_eq!(validate_transaction_metadata(&unaccepted, &tx), None);
    }
    #[test]
    fn blue_miner_payload_requires_script_and_worker() {
        let script = vec![0xab, 0xcd, 0x12];
        let block = serde_json::json!({
            "header":{"nonce":123u64,"timestamp":1793708190541u64},
            "transactions":[{"payload":"0102abcd1248656c6c6f"}]
        });
        let mined = mine_match(&block, &script).expect("payload match");
        assert_eq!(mined.0, "123");
        assert_eq!(mined.1, "Hello");
        assert_eq!(mine_match(&block, &[0xff, 0x01]), None);
    }
    #[test]
    fn rejects_non_hex_and_unverified_block_hash() {
        assert_eq!(decode_hex_limited("abc", 10), None);
        assert_eq!(decode_hex_limited("zz", 10), None);
        let block = serde_json::json!({"verboseData":{"hash":"a".repeat(64)}});
        assert!(block_hash_matches(&block, &"a".repeat(64)));
        assert!(!block_hash_matches(&block, &"b".repeat(64)));
    }
}

#[cfg(test)]
mod live_public_archive_contract_tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires live public Kaspa REST archival service; read-only"]
    async fn historical_coinbase_proof_matches_public_mined_block() {
        // Known public on-chain transaction, selected from the pre-existing
        // failed historical proof example. No Telegram user data is embedded.
        const TXID: &str = "484e87993362c445ee0b06804088df4019884832e26c094439b5faaac9ce60df";
        const CONTAINING: &str = "df9f0959b97cf85e88300b6ede89a68b265278f7f5f261372daa635d58adf460";
        const MINED: &str = "f733b3be9c82be94ff028d4727214d1eeac34937c4ffc5dc5c6cd670dc8eab7c";

        let containing = get_json(&format!("/blocks/{CONTAINING}?includeTransactions=true"))
            .await
            .expect("public containing block must exist");
        let mined_block = get_json(&format!("/blocks/{MINED}?includeTransactions=true"))
            .await
            .expect("public mined blue block must exist");
        let outputs = containing["transactions"][0]["outputs"]
            .as_array()
            .expect("coinbase outputs");
        let mut wallets = Vec::new();
        for output in outputs {
            if let (Some(address), Some(script_hex)) = (
                output["verboseData"]["scriptPublicKeyAddress"].as_str(),
                output["scriptPublicKey"]["scriptPublicKey"].as_str(),
            ) && let Some(script) = decode_hex_limited(script_hex, 512)
                && mine_match(&mined_block, &script).is_some()
            {
                wallets.push(address.to_string());
            }
        }
        assert_eq!(wallets.len(), 1, "one exactly-attributed wallet");
        let proof = historical_coinbase_proof(TXID, &wallets[0])
            .await
            .expect("API requests")
            .expect("full proof");
        assert_eq!(proof.containing_hash, CONTAINING);
        assert_eq!(proof.mined_hashes, vec![MINED.to_string()]);
        assert_ne!(proof.accepting_hash, proof.containing_hash);
        assert!(!proof.worker.is_empty());
        assert!(proof.block_time_ms > 0);
    }
}
