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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArchiveBlockRole {
    Accepting,
    Containing,
    MinedBlue,
}

// Retry exactly ONE 404 and ONLY for blocks. Never retry transactions,
// rate limits, server errors or other HTTP failures in this attempt.
fn retry_initial_block_404(path: &str, status: u16, already_retried: bool) -> bool {
    !already_retried && status == 404 && path.starts_with("/blocks/")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoricalCoinbaseProof {
    pub accepting_hash: String,
    pub containing_hash: String,
    pub mined_hashes: Vec<String>,
    pub nonce: String,
    pub worker: String,
    pub block_time_ms: u64,
}

// Production currently has only post-cutover proof retries. One request at
// a time, with a bounded start-to-start gap; global HTTP limiter remains 300ms.
fn parse_archive_cooldown(raw: Option<&str>) -> Duration {
    Duration::from_secs(
        raw.and_then(|x| x.parse::<u64>().ok())
            .unwrap_or(20)
            .clamp(15, 120),
    )
}
fn historical_proof_cooldown() -> Duration {
    parse_archive_cooldown(
        std::env::var("KASPA_HISTORICAL_PROOF_COOLDOWN_SECS")
            .ok()
            .as_deref(),
    )
}

fn incomplete_verified_proof(
    reason: crate::infrastructure::metrics::ArchiveIncompleteReason,
) -> Option<HistoricalCoinbaseProof> {
    crate::infrastructure::metrics::record_archive_incomplete(reason);
    None
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
    // One Coinbase transaction can pay multiple blue rewards to the same
    // wallet. Identical scripts are not ambiguous, but different scripts
    // purporting to be the same wallet must be rejected.
    let first = matches.next()?;
    let first_script =
        decode_hex_limited(first["scriptPublicKey"]["scriptPublicKey"].as_str()?, 512)?;
    if first_script.is_empty() {
        return None;
    }
    for output in matches {
        let script =
            decode_hex_limited(output["scriptPublicKey"]["scriptPublicKey"].as_str()?, 512)?;
        if script != first_script {
            return None;
        }
    }
    Some(first_script)
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
    let metadata = crate::network::coinbase_payload::canonical_miner_extra(&bytes, script)?;
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

/// OP083: decode ONLY the canonical declared miner script bytes and version
/// from the Coinbase payload. Never search the payload for arbitrary patterns.
fn coinbase_miner_script_version(block: &Value) -> Option<(u64, Vec<u8>)> {
    let encoded = block["transactions"][0]["payload"].as_str()?;
    let bytes = decode_hex_limited(encoded, 8192)?;
    if bytes.len() < 19 || bytes[18] == 0 {
        return None;
    }
    let end = 19_usize.checked_add(bytes[18] as usize)?;
    let script = bytes.get(19..end)?.to_vec();
    Some((u16::from_le_bytes([bytes[16], bytes[17]]) as u64, script))
}

/// Every blue payout must be bound to the SAME indexed blue block's canonical
/// miner script. This proof is required for red-self reward attribution only.
/// Existing blue reward acceptance stays byte-for-byte unchanged.
fn blue_payout_strictly_bound(blue: &Value, payout: &Value) -> bool {
    let Some((miner_version, miner_script)) = coinbase_miner_script_version(blue) else {
        return false;
    };
    let Some(payout_version) = json_u64(&payout["scriptPublicKey"]["version"]) else {
        return false;
    };
    let Some(payout_hex) = payout["scriptPublicKey"]["scriptPublicKey"].as_str() else {
        return false;
    };
    let Some(payout_script) = decode_hex_limited(payout_hex, 512) else {
        return false;
    };
    miner_version == payout_version
        && miner_script == payout_script
        && json_u64(&payout["amount"]).is_some_and(|amount| amount > 0)
}

/// OP083: self-red is distinct from blue rewards. Kaspa's accepted Coinbase
/// pays each merge-set blue miner, followed (ONLY when red reward >0) by a
/// last output to the miner of the *containing* block. Require:
/// - complete unique ordered binding of all blue payouts to their miners;
/// - one extra, positive last output exclusively matching the user's wallet;
/// - nonempty canonical red set and the containing miner's exact payload
///   script+version and independently verified worker/nonce/timestamp.
///
/// The caller already checked tx acceptance, accepting-to-containing DAG
/// relation, containing block hash and exact first Coinbase transaction ID.
fn strict_self_red_reward(
    containing: &Value,
    wallet: &str,
    script: &[u8],
    blue_count: usize,
    every_blue_payout_bound: bool,
) -> Option<(String, String, u64)> {
    if !every_blue_payout_bound {
        return None;
    }
    let reds = containing["verboseData"]["mergeSetRedsHashes"].as_array()?;
    if reds.is_empty() || reds.len() > MAX_MERGE_SET_BLUES {
        return None;
    }
    if !reds.iter().all(|red| red.as_str().is_some_and(valid_hash)) {
        return None;
    }
    let outputs = containing["transactions"][0]["outputs"].as_array()?;
    if outputs.len() != blue_count.checked_add(1)? {
        return None;
    }
    let last = outputs.last()?;
    if last["verboseData"]["scriptPublicKeyAddress"].as_str()? != wallet
        || outputs
            .iter()
            .filter(|out| out["verboseData"]["scriptPublicKeyAddress"].as_str() == Some(wallet))
            .count()
            != 1
    {
        return None;
    }
    let (own_version, own_script) = coinbase_miner_script_version(containing)?;
    let last_script =
        decode_hex_limited(last["scriptPublicKey"]["scriptPublicKey"].as_str()?, 512)?;
    if own_script != script
        || last_script != script
        || json_u64(&last["scriptPublicKey"]["version"])? != own_version
        || !json_u64(&last["amount"]).is_some_and(|amount| amount > 0)
    {
        return None;
    }
    mine_match(containing, script)
}

fn validate_transaction_metadata_detailed(
    meta: &Value,
    txid: &str,
) -> Result<(String, String), crate::infrastructure::metrics::ArchiveTxMetadataFailure> {
    use crate::infrastructure::metrics::ArchiveTxMetadataFailure as R;
    match meta["is_accepted"].as_bool() {
        Some(true) => {}
        Some(false) => return Err(R::NotAccepted),
        None => return Err(R::AcceptanceFlagInvalid),
    }
    let Some(given_txid) = meta["transaction_id"].as_str() else {
        return Err(R::TransactionIdMismatch);
    };
    if !given_txid.eq_ignore_ascii_case(txid) {
        return Err(R::TransactionIdMismatch);
    }
    let Some(accepting) = meta["accepting_block_hash"].as_str() else {
        return Err(R::AcceptingHashMissing);
    };
    if !valid_hash(accepting) {
        return Err(R::AcceptingHashInvalid);
    }
    let Some(containing) = meta["block_hash"].as_array() else {
        return Err(R::ContainingBlockNotUnique);
    };
    if containing.len() != 1 {
        return Err(R::ContainingBlockNotUnique);
    }
    let Some(block) = containing[0].as_str() else {
        return Err(R::ContainingBlockHashInvalid);
    };
    if !valid_hash(block) {
        return Err(R::ContainingBlockHashInvalid);
    }
    Ok((accepting.to_ascii_lowercase(), block.to_ascii_lowercase()))
}

fn validate_transaction_metadata(meta: &Value, txid: &str) -> Option<(String, String)> {
    validate_transaction_metadata_detailed(meta, txid).ok()
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
    let mut response = client.get(&url).send().await.map_err(|e| e.to_string())?;
    if retry_initial_block_404(path, response.status().as_u16(), false) {
        crate::infrastructure::metrics::record_archive_block_404_retry_attempt();
        // One bounded delayed repeat mitigates a transient public REST
        // indexing miss. The outer 35s proof timeout and 8s HTTP timeout
        // still apply. Never cache, guess, or accept missing blocks.
        tokio::time::sleep(Duration::from_millis(1500)).await;
        response = client.get(&url).send().await.map_err(|e| e.to_string())?;
        if response.status().is_success() {
            crate::infrastructure::metrics::record_archive_block_404_retry_recovered();
        } else if response.status().as_u16() == 404 {
            crate::infrastructure::metrics::record_archive_block_404_retry_exhausted();
        }
    }
    if !response.status().is_success() {
        // Only fixed, privacy-safe endpoint class is attached. Never include
        // transaction IDs, wallets, URLs or response bodies in this error.
        let stage = if path.starts_with("/transactions/") {
            "transaction"
        } else if path.starts_with("/blocks/") {
            "block"
        } else {
            "unknown"
        };
        return Err(format!(
            "REST HTTP {} [archive_stage={stage}]",
            response.status()
        ));
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

// A final 404 after the one bounded retry is still an error and remains
// governed by durable postcutover proof retry; the fixed role is diagnostic.
async fn get_block_json(
    hash: &str,
    include_transactions: bool,
    role: ArchiveBlockRole,
) -> Result<Value, String> {
    let result = get_json(&format!(
        "/blocks/{hash}?includeTransactions={include_transactions}"
    ))
    .await;
    if let Err(ref error) = result
        && error.starts_with("REST HTTP 404 ")
        && error.ends_with("[archive_stage=block]")
    {
        crate::infrastructure::metrics::record_archive_block_final_404(role.into());
    }
    result
}

impl From<ArchiveBlockRole> for crate::infrastructure::metrics::ArchiveBlock404Role {
    fn from(role: ArchiveBlockRole) -> Self {
        use crate::infrastructure::metrics::ArchiveBlock404Role as M;
        match role {
            ArchiveBlockRole::Accepting => M::Accepting,
            ArchiveBlockRole::Containing => M::Containing,
            ArchiveBlockRole::MinedBlue => M::MinedBlue,
        }
    }
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
    // OP070: ONLY newer, already persisted Coinbase proof rows enter
    // this route (enforced by the fixed SQL admission and outbox gates).
    // A try_acquire here made competing wallet scans discard valid proof
    // attempts immediately; cooldown then repeatedly discarded the next.
    // Wait in Tokio's FIFO semaphore queue for a bounded interval, while
    // retaining STRICTLY ONE active public REST proof at any time.
    let gate_started = Instant::now();
    let proof_gate = PROOF_GATE.get_or_init(|| Semaphore::new(1));
    let _proof_permit =
        match tokio::time::timeout(Duration::from_secs(25), proof_gate.acquire()).await {
            Ok(Ok(permit)) => permit,
            _ => {
                crate::infrastructure::metrics::record_archive_gate_busy();
                crate::infrastructure::metrics::record_archive_wait_timeout();
                return Ok(None);
            }
        };
    // Cooldown is a wait, not a skipped admission. Keep the permit across
    // cooldown so the next queued request cannot leapfrog an older proof.
    let mut last_start = LAST_PROOF_START
        .get_or_init(|| Mutex::new(None))
        .lock()
        .await;
    if let Some(at) = *last_start {
        let wait = historical_proof_cooldown().saturating_sub(at.elapsed());
        // An operator-configured cooldown can reach 120s; never block an
        // entire wallet scan that long. Such a request remains durable.
        if wait > Duration::from_secs(40) {
            crate::infrastructure::metrics::record_archive_cooldown();
            return Ok(None);
        }
        if !wait.is_zero() {
            crate::infrastructure::metrics::record_archive_cooldown();
            tokio::time::sleep(wait).await;
        }
    }
    *last_start = Some(Instant::now());
    drop(last_start);
    crate::infrastructure::metrics::record_archive_admitted(
        gate_started.elapsed().as_millis() as u64
    );
    let meta = get_json(&format!("/transactions/{txid}?inputs=false&outputs=false")).await?;
    let Some((accepting_hash, containing_hash)) = validate_transaction_metadata(&meta, txid) else {
        // Record the ONE fixed rejection reason while keeping the existing
        // fail-closed incomplete-proof disposition byte-for-byte equivalent.
        if let Err(reason) = validate_transaction_metadata_detailed(&meta, txid) {
            crate::infrastructure::metrics::record_archive_tx_metadata_failure(reason);
        }
        return Ok(incomplete_verified_proof(
            crate::infrastructure::metrics::ArchiveIncompleteReason::TransactionMetadata,
        ));
    };
    let accepting = get_block_json(&accepting_hash, false, ArchiveBlockRole::Accepting).await?;
    if !block_hash_matches(&accepting, &accepting_hash) {
        return Ok(incomplete_verified_proof(
            crate::infrastructure::metrics::ArchiveIncompleteReason::AcceptingBlock,
        ));
    }
    let Some(accepted_blues) = accepting["verboseData"]["mergeSetBluesHashes"].as_array() else {
        return Ok(incomplete_verified_proof(
            crate::infrastructure::metrics::ArchiveIncompleteReason::AcceptingBlock,
        ));
    };
    if !accepted_blues.iter().any(|v| {
        v.as_str()
            .is_some_and(|hash| hash.eq_ignore_ascii_case(&containing_hash))
    }) {
        return Ok(incomplete_verified_proof(
            crate::infrastructure::metrics::ArchiveIncompleteReason::AcceptanceRelation,
        ));
    }

    let containing = get_block_json(&containing_hash, true, ArchiveBlockRole::Containing).await?;
    if !block_hash_matches(&containing, &containing_hash)
        || !coinbase_txid_matches(&containing, txid)
    {
        return Ok(incomplete_verified_proof(
            crate::infrastructure::metrics::ArchiveIncompleteReason::ContainingBlock,
        ));
    }
    let Some(script) = wallet_script(&containing, wallet) else {
        return Ok(incomplete_verified_proof(
            crate::infrastructure::metrics::ArchiveIncompleteReason::WalletScript,
        ));
    };
    let Some(blues) = containing["verboseData"]["mergeSetBluesHashes"].as_array() else {
        return Ok(incomplete_verified_proof(
            crate::infrastructure::metrics::ArchiveIncompleteReason::BlueSetShape,
        ));
    };
    if blues.is_empty() || blues.len() > MAX_MERGE_SET_BLUES {
        return Ok(incomplete_verified_proof(
            crate::infrastructure::metrics::ArchiveIncompleteReason::BlueSetShape,
        ));
    }
    let mut mined_hashes = Vec::new();
    let mut nonce = String::new();
    let mut worker = String::new();
    let mut block_time_ms = 0;
    let mut every_blue_payout_bound = true;
    for (blue_index, blue) in blues.iter().enumerate() {
        let Some(hash) = blue.as_str().filter(|s| valid_hash(s)) else {
            return Ok(incomplete_verified_proof(
                crate::infrastructure::metrics::ArchiveIncompleteReason::BlueSetShape,
            ));
        };
        let block = get_block_json(hash, true, ArchiveBlockRole::MinedBlue).await?;
        if !block_hash_matches(&block, hash) {
            return Ok(incomplete_verified_proof(
                crate::infrastructure::metrics::ArchiveIncompleteReason::MinedBlueBlock,
            ));
        }
        // Only the NEW self-red path uses the strict indexed blue-output
        // mapping. Do not change existing blue miner attribution behavior.
        every_blue_payout_bound &= blue_payout_strictly_bound(
            &block,
            &containing["transactions"][0]["outputs"][blue_index],
        );
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
        // No blue block was attributable to this wallet. A verified chain
        // block may instead earn the red merge-set self-reward. Never infer
        // one from a matching address alone: all strict checks are mandatory.
        if let Some((self_nonce, self_worker, self_timestamp)) = strict_self_red_reward(
            &containing,
            wallet,
            &script,
            blues.len(),
            every_blue_payout_bound,
        ) {
            // The mined block for the self-red reward is the containing
            // block, never a fabricated blue/red block hash.
            mined_hashes.push(containing_hash.to_ascii_lowercase());
            nonce = self_nonce;
            worker = self_worker;
            block_time_ms = self_timestamp;
        } else {
            return Ok(incomplete_verified_proof(
                crate::infrastructure::metrics::ArchiveIncompleteReason::MinerAttribution,
            ));
        }
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
    fn archive_cooldown_is_bounded_and_safer_for_fresh_rewards() {
        assert_eq!(parse_archive_cooldown(None), Duration::from_secs(20));
        assert_eq!(parse_archive_cooldown(Some("0")), Duration::from_secs(15));
        assert_eq!(parse_archive_cooldown(Some("20")), Duration::from_secs(20));
        assert_eq!(
            parse_archive_cooldown(Some("999")),
            Duration::from_secs(120)
        );
        assert_eq!(
            parse_archive_cooldown(Some("invalid")),
            Duration::from_secs(20)
        );
    }

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
            "transactions":[{"payload":format!("{}03abcd1248656c6c6f", "00".repeat(18))}]
        });
        let mined = mine_match(&block, &script).expect("payload match");
        assert_eq!(mined.0, "123");
        assert_eq!(mined.1, "Hello");
        assert_eq!(mine_match(&block, &[0xff, 0x01]), None);
    }
    #[test]
    fn same_wallet_can_receive_multiple_blue_rewards_but_scripts_must_match() {
        let wallet = "kaspa:wallet-placeholder";
        let output = |script: &str| {
            serde_json::json!({
                "verboseData":{"scriptPublicKeyAddress":wallet},
                "scriptPublicKey":{"scriptPublicKey":script}
            })
        };
        let block = serde_json::json!({
            "transactions":[{"outputs":[output("abcd12"),output("abcd12")]}]
        });
        assert_eq!(wallet_script(&block, wallet), Some(vec![0xab, 0xcd, 0x12]));
        let conflicting = serde_json::json!({
            "transactions":[{"outputs":[output("abcd12"),output("ffff12")]}]
        });
        assert_eq!(wallet_script(&conflicting, wallet), None);
    }

    #[test]
    fn mine_match_requires_canonical_coinbase_script_offset_and_length() {
        let script = vec![0xab, 0xcd, 0x12];
        let forged_location = serde_json::json!({
            "header":{"nonce":123u64,"timestamp":1793708190541u64},
            "transactions":[{"payload":format!("{}{}",
                "00".repeat(19), "abcd1248656c6c6f")}]
        });
        assert_eq!(mine_match(&forged_location, &script), None);
        let wrong_declared_len = serde_json::json!({
            "header":{"nonce":123u64,"timestamp":1793708190541u64},
            "transactions":[{"payload":format!("{}05abcd1248656c6c6f", "00".repeat(18))}]
        });
        assert_eq!(mine_match(&wrong_declared_len, &script), None);
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

#[cfg(test)]
mod op070_public_archive_gate_contract_tests {
    use super::*;
    #[tokio::test]
    async fn fifo_queue_does_not_let_later_claims_jump_an_older_one() {
        let gate = std::sync::Arc::new(Semaphore::new(1));
        let initial = gate.acquire().await.expect("initial permit");
        let (sender, mut receiver) = tokio::sync::mpsc::channel::<usize>(2);
        for id in [1usize, 2] {
            let semaphore = gate.clone();
            let sender = sender.clone();
            tokio::spawn(async move {
                let _permit = semaphore.acquire().await.unwrap();
                sender.send(id).await.unwrap();
            });
            tokio::task::yield_now().await;
        }
        drop(initial);
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), receiver.recv())
                .await
                .unwrap(),
            Some(1)
        );
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), receiver.recv())
                .await
                .unwrap(),
            Some(2)
        );
    }
    #[test]
    fn fixed_historical_cooldown_retains_rate_limit() {
        assert!(parse_archive_cooldown(None) >= Duration::from_secs(15));
        assert!(parse_archive_cooldown(Some("0")) >= Duration::from_secs(15));
        assert!(parse_archive_cooldown(Some("999")) <= Duration::from_secs(120));
    }
}

#[cfg(test)]
mod op080_readonly_metadata_reason_contract_tests {
    use super::*;
    use crate::infrastructure::metrics::ArchiveTxMetadataFailure as R;
    fn valid() -> (serde_json::Value, String) {
        let tx = "c".repeat(64);
        let v = serde_json::json!({
            "is_accepted":true,
            "transaction_id":tx,
            "accepting_block_hash":"a".repeat(64),
            "block_hash":["b".repeat(64)]
        });
        (v, tx)
    }
    #[test]
    fn false_acceptance_has_priority_over_missing_hash_and_is_never_delivered() {
        let (mut v, tx) = valid();
        v["is_accepted"] = serde_json::json!(false);
        v.as_object_mut().unwrap().remove("accepting_block_hash");
        assert_eq!(
            validate_transaction_metadata_detailed(&v, &tx),
            Err(R::NotAccepted)
        );
        assert_eq!(validate_transaction_metadata(&v, &tx), None);
        assert_eq!(
            validate_transaction_metadata(&valid().0, &tx),
            Some(("a".repeat(64), "b".repeat(64)))
        );
    }
    #[test]
    fn invalid_or_missing_fields_classified_without_relaxing_original_gate() {
        let (v, tx) = valid();
        let mut cases: Vec<(serde_json::Value, R)> = vec![];
        let mut bad = v.clone();
        bad["is_accepted"] = serde_json::Value::Null;
        cases.push((bad, R::AcceptanceFlagInvalid));
        let mut bad = v.clone();
        bad["transaction_id"] = serde_json::json!("f".repeat(64));
        cases.push((bad, R::TransactionIdMismatch));
        let mut bad = v.clone();
        bad.as_object_mut().unwrap().remove("accepting_block_hash");
        cases.push((bad, R::AcceptingHashMissing));
        let mut bad = v.clone();
        bad["accepting_block_hash"] = serde_json::json!("not-a-hash");
        cases.push((bad, R::AcceptingHashInvalid));
        let mut bad = v.clone();
        bad["block_hash"] = serde_json::json!([]);
        cases.push((bad, R::ContainingBlockNotUnique));
        let mut bad = v.clone();
        bad["block_hash"] = serde_json::json!(["not-a-hash"]);
        cases.push((bad, R::ContainingBlockHashInvalid));
        for (meta, reason) in cases {
            assert_eq!(
                validate_transaction_metadata_detailed(&meta, &tx),
                Err(reason)
            );
            assert_eq!(validate_transaction_metadata(&meta, &tx), None);
        }
    }
    #[test]
    fn valid_hashes_still_require_unique_containing_and_exact_tx_id() {
        let (mut v, tx) = valid();
        v["block_hash"] = serde_json::json!(["b".repeat(64), "d".repeat(64)]);
        assert_eq!(validate_transaction_metadata(&v, &tx), None);
        let (v, tx) = valid();
        assert_eq!(
            validate_transaction_metadata(&v, &tx),
            Some(("a".repeat(64), "b".repeat(64)))
        );
    }
}

#[cfg(test)]
mod op081_bounded_public_archive_retry_policy_tests {
    use super::retry_initial_block_404;
    #[test]
    fn retries_only_one_block_404_and_never_transaction_or_other_errors() {
        assert!(retry_initial_block_404(
            "/blocks/abcd?includeTransactions=true",
            404,
            false
        ));
        assert!(retry_initial_block_404(
            "/blocks/abcd?includeTransactions=false",
            404,
            false
        ));
        for code in [200, 400, 401, 403, 408, 410, 429, 500, 502, 503] {
            assert!(!retry_initial_block_404("/blocks/abcd", code, false));
        }
        assert!(!retry_initial_block_404("/transactions/abcd", 404, false));
        assert!(!retry_initial_block_404("/unknown/abcd", 404, false));
        assert!(!retry_initial_block_404("/blocks/abcd", 404, true));
    }
    #[test]
    fn block_roles_are_fixed_and_no_new_bypass_of_failed_verification() {
        use super::ArchiveBlockRole;
        use crate::infrastructure::metrics::ArchiveBlock404Role as M;
        assert_eq!(M::from(ArchiveBlockRole::Accepting), M::Accepting);
        assert_eq!(M::from(ArchiveBlockRole::Containing), M::Containing);
        assert_eq!(M::from(ArchiveBlockRole::MinedBlue), M::MinedBlue);
    }
}

#[cfg(test)]
mod op083_canonical_self_red_reward_contract_tests {
    use super::*;
    fn example() -> (serde_json::Value, String, Vec<u8>) {
        let wallet = "kaspa:test-wallet".to_owned();
        let script = vec![0xab, 0xcd, 0x12];
        let output = |destination: &str, hex: &str| {
            serde_json::json!({
                "amount":1u64, "verboseData":{"scriptPublicKeyAddress":destination},
                "scriptPublicKey":{"version":0,"scriptPublicKey":hex}
            })
        };
        let payload = format!("{}000003abcd12576f726b6572", "00".repeat(16));
        let block = serde_json::json!({
            "header":{"nonce":123u64,"timestamp":1793708190541u64},
            "verboseData":{"mergeSetRedsHashes":["a".repeat(64)]},
            "transactions":[{
                "payload":payload,
                "outputs":[
                    output("kaspa:blue1","aabb"),
                    output("kaspa:blue2","1122"),
                    output(&wallet,"abcd12")
                ]
            }]
        });
        (block, wallet, script)
    }
    #[test]
    fn accepted_self_red_requires_exact_last_output_and_canonical_miner() {
        let (block, wallet, script) = example();
        assert_eq!(
            strict_self_red_reward(&block, &wallet, &script, 2, true),
            Some(("123".to_owned(), "Worker".to_owned(), 1793708190541u64))
        );
        assert_eq!(
            strict_self_red_reward(&block, &wallet, &script, 2, false),
            None
        );
        assert_eq!(
            strict_self_red_reward(&block, &wallet, &script, 3, true),
            None
        );
        assert_eq!(
            strict_self_red_reward(&block, &wallet, &[0xff], 2, true),
            None
        );
    }
    #[test]
    fn rejects_missing_reds_nonpositive_self_output_bad_version_and_extra_payee() {
        let (base, wallet, script) = example();
        let mut invalid = base.clone();
        invalid["verboseData"]["mergeSetRedsHashes"] = serde_json::json!([]);
        assert_eq!(
            strict_self_red_reward(&invalid, &wallet, &script, 2, true),
            None
        );
        let mut invalid = base.clone();
        invalid["transactions"][0]["outputs"][2]["amount"] = serde_json::json!(0);
        assert_eq!(
            strict_self_red_reward(&invalid, &wallet, &script, 2, true),
            None
        );
        let mut invalid = base.clone();
        invalid["transactions"][0]["outputs"][2]["scriptPublicKey"]["version"] =
            serde_json::json!(1);
        assert_eq!(
            strict_self_red_reward(&invalid, &wallet, &script, 2, true),
            None
        );
        let mut invalid = base.clone();
        invalid["transactions"][0]["outputs"][1]["verboseData"]["scriptPublicKeyAddress"] =
            serde_json::json!(wallet);
        assert_eq!(
            strict_self_red_reward(&invalid, &wallet, &script, 2, true),
            None
        );
        let mut invalid = base;
        invalid["verboseData"]["mergeSetRedsHashes"] = serde_json::json!(["INVALID"]);
        assert_eq!(
            strict_self_red_reward(&invalid, &wallet, &script, 2, true),
            None
        );
    }
    #[test]
    fn indexed_blue_payout_binding_requires_miner_payload_version_script_and_amount() {
        let blue = serde_json::json!({"transactions":[{"payload":
            format!("{}000003abcd12576f726b6572","00".repeat(16))}]});
        let payout = serde_json::json!({"amount":1000,"scriptPublicKey":
            {"version":0,"scriptPublicKey":"abcd12"}});
        assert!(blue_payout_strictly_bound(&blue, &payout));
        let mut bad = payout.clone();
        bad["scriptPublicKey"]["version"] = serde_json::json!(1);
        assert!(!blue_payout_strictly_bound(&blue, &bad));
        let mut bad = payout.clone();
        bad["scriptPublicKey"]["scriptPublicKey"] = serde_json::json!("ffff");
        assert!(!blue_payout_strictly_bound(&blue, &bad));
        let mut bad = payout;
        bad["amount"] = serde_json::json!(0);
        assert!(!blue_payout_strictly_bound(&blue, &bad));
    }
}
