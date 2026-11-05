use kaspa_pulse::infrastructure::resilience::runtime::TaskSupervisor;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

#[test]
fn main_owns_signal_and_database_shutdown_lifecycle() {
    let source = include_str!("../src/main.rs");
    assert!(source.contains("async fn wait_for_shutdown_signal()"));
    assert!(source.contains("drive_dispatcher_until_shutdown"));
    let joined = source
        .rfind("drain_tracked_tasks(drain_timeout).await?")
        .unwrap();
    let closed = source.rfind("pool.close().await;").unwrap();
    assert!(
        joined < closed,
        "database close requires successful owned task joins"
    );
    assert!(!source.contains(".enable_ctrlc_handler()"));
    assert!(!source.contains("timed out after {} seconds; closing database pool"));
}

#[test]
fn health_and_request_work_share_owned_runtime_lifecycle() {
    let health = include_str!("../src/infrastructure/webhook_security.rs");
    let requests = include_str!("../src/presentation/telegram/handlers/lifecycle.rs");
    assert!(health.contains("spawn_resilient("));
    assert!(health.contains("\"health_endpoint\""));
    assert!(requests.contains("spawn_tracked(name, future)"));
}

#[tokio::test]
async fn drain_waits_for_a_registered_worker_to_finish() {
    let owner = TaskSupervisor::default();
    let finished = Arc::new(AtomicBool::new(false));
    let worker_finished = finished.clone();
    let worker = owner
        .spawn("graceful_shutdown_contract_test", async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            worker_finished.store(true, Ordering::Release);
        })
        .unwrap();
    assert!(
        owner
            .shutdown(Duration::from_secs(1))
            .await
            .unwrap()
            .cancelled
            .is_empty()
    );
    worker.join().await.unwrap();
    assert!(finished.load(Ordering::Acquire));
}
