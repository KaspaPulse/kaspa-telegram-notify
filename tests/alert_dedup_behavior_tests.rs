use kaspa_pulse::wallet::alert_dedup::{AlertIdentity, build_alert_identity, build_alert_key};

fn is_same_alert_for_test(first: &AlertIdentity, second: &AlertIdentity) -> bool {
    first.wallet == second.wallet && first.alert_key == second.alert_key
}

#[test]
fn alert_key_prefers_mined_block_hash_when_available() {
    let key = build_alert_key(Some("block_hash_1"), "tx_1");

    assert_eq!(key, "block_hash_1");
}

#[test]
fn alert_key_falls_back_to_transaction_id_when_mined_block_missing() {
    let key = build_alert_key(None, "tx_1");

    assert_eq!(key, "tx_1");
}

#[test]
fn alert_key_falls_back_to_transaction_id_when_mined_block_empty() {
    let key = build_alert_key(Some("   "), "tx_1");

    assert_eq!(key, "tx_1");
}

#[test]
fn same_wallet_and_same_alert_key_is_duplicate() {
    let first = build_alert_identity("kaspa:wallet1", Some("block_hash_1"), "tx_1");
    let second = build_alert_identity("kaspa:wallet1", Some("block_hash_1"), "tx_2");

    assert!(is_same_alert_for_test(&first, &second));
}

#[test]
fn same_wallet_and_different_alert_key_is_new_alert() {
    let first = build_alert_identity("kaspa:wallet1", Some("block_hash_1"), "tx_1");
    let second = build_alert_identity("kaspa:wallet1", Some("block_hash_2"), "tx_2");

    assert!(!is_same_alert_for_test(&first, &second));
}

#[test]
fn different_wallet_and_same_alert_key_is_not_duplicate_for_wallet_scoped_dedup() {
    let first = build_alert_identity("kaspa:wallet1", Some("block_hash_1"), "tx_1");
    let second = build_alert_identity("kaspa:wallet2", Some("block_hash_1"), "tx_1");

    assert!(!is_same_alert_for_test(&first, &second));
}

#[test]
fn fallback_transaction_id_dedup_is_stable() {
    let first = build_alert_identity("kaspa:wallet1", None, "tx_1");
    let second = build_alert_identity("kaspa:wallet1", None, "tx_1");

    assert!(is_same_alert_for_test(&first, &second));
}
