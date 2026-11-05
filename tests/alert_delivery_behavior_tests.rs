use kaspa_pulse::wallet::alert_delivery::{
    AlertDeliveryAttempt, AlertDeliveryOutcome, delivery_outcome, should_record_delivered,
    should_record_failed,
};

#[test]
fn successful_send_records_delivered() {
    let outcome = delivery_outcome(AlertDeliveryAttempt::SendSucceeded);

    assert_eq!(outcome, AlertDeliveryOutcome::Delivered);
    assert!(should_record_delivered(outcome));
    assert!(!should_record_failed(outcome));
}

#[test]
fn failed_send_records_failed() {
    let outcome = delivery_outcome(AlertDeliveryAttempt::SendFailed);

    assert_eq!(outcome, AlertDeliveryOutcome::Failed);
    assert!(!should_record_delivered(outcome));
    assert!(should_record_failed(outcome));
}

#[test]
fn delivered_and_failed_are_mutually_exclusive() {
    let delivered = delivery_outcome(AlertDeliveryAttempt::SendSucceeded);
    let failed = delivery_outcome(AlertDeliveryAttempt::SendFailed);

    assert_ne!(delivered, failed);
    assert!(should_record_delivered(delivered));
    assert!(should_record_failed(failed));
}
