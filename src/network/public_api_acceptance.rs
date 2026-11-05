//! Bounded, read-only public REST acceptance lookup. Never replaces on-chain mined-block proof.
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
static LAST_QUERY: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
const ACCEPTANCE_URL: &str = "https://api.kaspa.org/transactions/acceptance";

fn valid_hash(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn parse_acceptance(payload: &[u8], expected_txid: &str) -> Option<String> {
    if !valid_hash(expected_txid) || payload.len() > 4096 {
        return None;
    }
    let rows: serde_json::Value = serde_json::from_slice(payload).ok()?;
    let values = rows.as_array()?;
    if values.len() != 1 {
        return None;
    }
    let row = values.first()?;
    if row.get("accepted")?.as_bool()? != true {
        return None;
    }
    let txid = row.get("transactionId")?.as_str()?;
    let block = row.get("acceptingBlockHash")?.as_str()?;
    if !txid.eq_ignore_ascii_case(expected_txid) || !valid_hash(block) {
        return None;
    }
    Some(block.to_ascii_lowercase())
}

/// Return an acceptance *candidate*, not a mined block: the PNN DAG lookup
/// must still fetch and verify the referenced block and its merge set.
pub async fn acceptance_hash(txid: &str) -> Option<String> {
    if !valid_hash(txid) {
        return None;
    }
    let limiter = LAST_QUERY.get_or_init(|| Mutex::new(None));
    let mut last = limiter.lock().await;
    if last.is_some_and(|instant| instant.elapsed() < Duration::from_secs(2)) {
        return None;
    }
    *last = Some(Instant::now());
    drop(last);

    let client = CLIENT.get_or_init(reqwest::Client::new);
    let response = tokio::time::timeout(
        Duration::from_secs(8),
        client
            .post(ACCEPTANCE_URL)
            .json(&serde_json::json!({"transactionIds": [txid]}))
            .send(),
    )
    .await
    .ok()?
    .ok()?;
    if !response.status().is_success() {
        return None;
    }
    if response.content_length().is_some_and(|n| n > 4096) {
        return None;
    }
    let body = tokio::time::timeout(Duration::from_secs(8), async {
        let mut response = response;
        let mut payload = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if payload.len() + chunk.len() > 4096 {
                return Ok::<Option<Vec<u8>>, reqwest::Error>(None);
            }
            payload.extend_from_slice(&chunk);
        }
        Ok(Some(payload))
    })
    .await
    .ok()?
    .ok()??;
    parse_acceptance(&body, txid)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_only_exact_transaction_and_complete_acceptance_proof() {
        let txid = "a".repeat(64);
        let block = "b".repeat(64);
        let valid = serde_json::json!([{
            "transactionId": txid, "accepted": true, "acceptingBlockHash": block,
            "acceptingBlueScore": 123, "acceptingTimestamp": 1793708190541u64
        }]);
        assert_eq!(
            parse_acceptance(valid.to_string().as_bytes(), &"a".repeat(64)),
            Some("b".repeat(64))
        );
        assert_eq!(
            parse_acceptance(valid.to_string().as_bytes(), &"c".repeat(64)),
            None
        );
    }

    #[test]
    fn rejects_unaccepted_malformed_and_oversized_rest_responses() {
        let id = "a".repeat(64);
        let cases = [
            serde_json::json!([{"transactionId":id, "accepted":false,"acceptingBlockHash":"b".repeat(64)}]),
            serde_json::json!([{"transactionId":id, "accepted":true}]),
            serde_json::json!([{"transactionId":id, "accepted":true,"acceptingBlockHash":"invalid"}]),
            serde_json::json!([]),
        ];
        for bad in cases {
            assert_eq!(parse_acceptance(bad.to_string().as_bytes(), &id), None);
        }
        assert_eq!(parse_acceptance(&vec![b'x'; 4097], &id), None);
        assert_eq!(parse_acceptance(b"not json", &id), None);
    }
}
