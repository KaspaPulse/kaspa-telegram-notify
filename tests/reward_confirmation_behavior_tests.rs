use kaspa_pulse::wallet::reward_confirmation::reward_confirmation_status;

#[test]
fn coinbase_reward_below_required_confirmations_waits() {
    let status = reward_confirmation_status(true, 100, 109, 10);

    assert_eq!(status.confirmations, 9);
    assert!(!status.is_confirmed);
}

#[test]
fn coinbase_reward_at_required_confirmations_is_confirmed() {
    let status = reward_confirmation_status(true, 100, 110, 10);

    assert_eq!(status.confirmations, 10);
    assert!(status.is_confirmed);
}

#[test]
fn coinbase_reward_above_required_confirmations_is_confirmed() {
    let status = reward_confirmation_status(true, 100, 115, 10);

    assert_eq!(status.confirmations, 15);
    assert!(status.is_confirmed);
}

#[test]
fn non_coinbase_utxo_is_not_blocked_by_reward_confirmation_gate() {
    let status = reward_confirmation_status(false, 100, 100, 10);

    assert_eq!(status.confirmations, 0);
    assert!(status.is_confirmed);
}

#[test]
fn virtual_daa_behind_reward_daa_saturates_to_zero() {
    let status = reward_confirmation_status(true, 200, 100, 10);

    assert_eq!(status.confirmations, 0);
    assert!(!status.is_confirmed);
}

#[test]
fn required_confirmations_are_clamped_to_safe_range() {
    let zero_required = reward_confirmation_status(true, 100, 100, 0);
    assert_eq!(zero_required.confirmations, 0);
    assert!(!zero_required.is_confirmed);

    let huge_required = reward_confirmation_status(true, 100, 10_101, u64::MAX);
    assert_eq!(huge_required.confirmations, 10_001);
    assert!(huge_required.is_confirmed);
}

use kaspa_pulse::wallet::reward_confirmation::{
    RewardProcessingDecision, reward_processing_decision,
};

#[test]
fn unseen_unconfirmed_coinbase_reward_is_persisted_pending() {
    let decision = reward_processing_decision(false, false, true, 100, 109, 10);

    assert_eq!(decision, RewardProcessingDecision::PersistPending);
}

#[test]
fn unseen_confirmed_coinbase_reward_is_processed_now() {
    let decision = reward_processing_decision(false, false, true, 100, 110, 10);

    assert_eq!(decision, RewardProcessingDecision::ProcessNow);
}

#[test]
fn seen_reward_is_not_reprocessed() {
    let decision = reward_processing_decision(false, true, true, 100, 500, 10);

    assert_eq!(decision, RewardProcessingDecision::AlreadySeen);
}

#[test]
fn first_run_snapshots_utxos_without_processing_alerts() {
    let decision = reward_processing_decision(true, false, true, 100, 500, 10);

    assert_eq!(decision, RewardProcessingDecision::FirstRunSnapshot);
}

#[test]
fn normal_non_coinbase_new_utxo_can_process_now() {
    let decision = reward_processing_decision(false, false, false, 100, 100, 10);

    assert_eq!(decision, RewardProcessingDecision::ProcessNow);
}
