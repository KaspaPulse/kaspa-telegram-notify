use kaspa_pulse::infrastructure::database::runtime_pool_options;
use kaspa_pulse::infrastructure::resilience::runtime::{TaskSupervisor, task_stage};
use sqlx::postgres::PgPoolOptions;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

async fn pool() -> sqlx::PgPool {
    PgPoolOptions::new()
        .max_connections(2)
        .connect(&std::env::var("DATABASE_URL").expect("isolated development PostgreSQL required"))
        .await
        .unwrap()
}

struct WorkerGuard {
    pool: sqlx::PgPool,
    events: Arc<Mutex<Vec<&'static str>>>,
}
impl Drop for WorkerGuard {
    fn drop(&mut self) {
        assert!(
            !self.pool.is_closed(),
            "worker still owned DB after pool close"
        );
        self.events.lock().unwrap().push("worker_stopped");
    }
}

#[tokio::test]
async fn normal_shutdown_joins_active_worker_before_database_close() {
    let owner = TaskSupervisor::default();
    let pool = pool().await;
    let events = Arc::new(Mutex::new(Vec::new()));
    let guard = WorkerGuard {
        pool: pool.clone(),
        events: events.clone(),
    };
    let cancel = owner.cancellation_token();
    let (started, ready) = oneshot::channel();
    let task = owner
        .spawn("utxo_normal", async move {
            let _guard = guard;
            started.send(()).unwrap();
            cancel.cancelled().await;
            sqlx::query("SELECT 1").execute(&_guard.pool).await.unwrap();
        })
        .unwrap();
    ready.await.unwrap();
    let report = owner.shutdown(Duration::from_secs(1)).await.unwrap();
    task.join().await.unwrap();
    assert!(report.cancelled.is_empty());
    assert_eq!(*events.lock().unwrap(), vec!["worker_stopped"]);
    pool.close().await;
    events.lock().unwrap().push("db_closed");
    assert_eq!(*events.lock().unwrap(), vec!["worker_stopped", "db_closed"]);
    assert!(owner.active_tasks().is_empty());
}

#[tokio::test]
async fn database_must_outlive_worker_after_drain_deadline() {
    let owner = TaskSupervisor::default();
    let pool = pool().await;
    let events = Arc::new(Mutex::new(Vec::new()));
    let guard = WorkerGuard {
        pool: pool.clone(),
        events: events.clone(),
    };
    let (started, ready) = oneshot::channel();
    let (release, wait) = oneshot::channel::<()>();
    let task = owner
        .spawn("inflight_utxo_db_regression", async move {
            let _guard = guard;
            task_stage("waiting_for_db_operation");
            started.send(()).unwrap();
            let _ = wait.await;
            sqlx::query("SELECT 1").execute(&_guard.pool).await
        })
        .unwrap();
    ready.await.unwrap();
    let report = owner.shutdown(Duration::from_millis(20)).await.unwrap();
    assert_eq!(report.cancelled.len(), 1);
    assert_eq!(report.cancelled[0].name, "inflight_utxo_db_regression");
    assert_eq!(report.cancelled[0].stage, "waiting_for_db_operation");
    assert!(task.join().await.unwrap_err().is_cancelled());
    assert_eq!(*events.lock().unwrap(), vec!["worker_stopped"]);
    pool.close().await;
    assert!(
        release.send(()).is_err(),
        "worker must no longer await new DB work"
    );
    assert!(owner.active_tasks().is_empty());
}

#[tokio::test]
async fn shutdown_during_actual_sql_query_cancels_and_releases_connection() {
    let owner = TaskSupervisor::default();
    let pool = pool().await;
    let events = Arc::new(Mutex::new(Vec::new()));
    let guard = WorkerGuard {
        pool: pool.clone(),
        events: events.clone(),
    };
    let (started, ready) = oneshot::channel();
    let task = owner
        .spawn("utxo_active_postgres", async move {
            let _guard = guard;
            let mut connection = _guard.pool.acquire().await.unwrap();
            let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *connection)
                .await
                .unwrap();
            started.send(pid).unwrap();
            task_stage("postgres_pg_sleep");
            sqlx::query("SELECT pg_sleep(0.3)")
                .execute(&mut *connection)
                .await
        })
        .unwrap();
    let pid = ready.await.unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let active: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE pid=$1 AND wait_event='PgSleep')"
            ).bind(pid).fetch_one(&pool).await.unwrap();
            if active { break; }
            tokio::task::yield_now().await;
        }
    }).await.unwrap();
    let report = owner.shutdown(Duration::from_millis(20)).await.unwrap();
    assert_eq!(report.cancelled[0].stage, "postgres_pg_sleep");
    assert!(task.join().await.unwrap_err().is_cancelled());
    tokio::time::timeout(Duration::from_secs(1), pool.close())
        .await
        .unwrap();
    assert_eq!(pool.size(), 0);
    assert_eq!(*events.lock().unwrap(), vec!["worker_stopped"]);
}

#[tokio::test]
async fn runtime_pool_policy_closes_cancelled_lock_wait_without_waiting_for_unlock() {
    let database_url =
        std::env::var("DATABASE_URL").expect("isolated development PostgreSQL required");
    let shutdown = CancellationToken::new();
    let runtime_pool = runtime_pool_options(1, shutdown.clone())
        .connect(&database_url)
        .await
        .unwrap();
    let observer = pool().await;
    let mut blocker = observer.acquire().await.unwrap();
    let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    let lock_key = 9_612_000_i64 + i64::from(std::process::id());
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(lock_key)
        .execute(&mut *blocker)
        .await
        .unwrap();

    let worker_pool = runtime_pool.clone();
    let (pid_tx, pid_rx) = oneshot::channel();
    let waiter = tokio::spawn(async move {
        let mut connection = worker_pool.acquire().await.unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *connection)
            .await
            .unwrap();
        pid_tx.send(pid).unwrap();
        sqlx::query("SELECT pg_advisory_lock($1)")
            .bind(lock_key)
            .execute(&mut *connection)
            .await
    });
    let waiter_pid = pid_rx.await.unwrap();

    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let blocked: bool = sqlx::query_scalar("SELECT $2 = ANY(pg_blocking_pids($1))")
                .bind(waiter_pid)
                .bind(blocker_pid)
                .fetch_one(&observer)
                .await
                .unwrap();
            if blocked {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("runtime connection must enter a real PostgreSQL lock wait");

    shutdown.cancel();
    waiter.abort();
    assert!(waiter.await.unwrap_err().is_cancelled());

    let close_started = Instant::now();
    tokio::time::timeout(Duration::from_secs(1), runtime_pool.close())
        .await
        .expect("shutdown pool policy must not wait for the server lock to be released");
    assert!(close_started.elapsed() < Duration::from_secs(1));
    assert_eq!(runtime_pool.size(), 0);

    let backend_close_started = Instant::now();
    let backend_closed_while_lock_held = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let backend_exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE pid = $1)")
                    .bind(waiter_pid)
                    .fetch_one(&observer)
                    .await
                    .unwrap();
            if !backend_exists {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .is_ok();
    eprintln!(
        "runtime pool close={}ms; postgres backend disappearance={}ms; blocker still held",
        close_started.elapsed().as_millis(),
        backend_close_started.elapsed().as_millis()
    );
    assert!(
        backend_closed_while_lock_held,
        "cancelled runtime connection remained server-side while blocker was still held"
    );

    let unlocked: bool = sqlx::query_scalar("SELECT pg_advisory_unlock($1)")
        .bind(lock_key)
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    assert!(unlocked);
    drop(blocker);
    observer.close().await;
}

#[tokio::test]
async fn orphaned_child_is_cancelled_and_joined_even_if_parent_result_is_dropped() {
    let owner = Arc::new(TaskSupervisor::default());
    let child_owner = owner.clone();
    let pool = pool().await;
    let events = Arc::new(Mutex::new(Vec::new()));
    let guard = WorkerGuard {
        pool: pool.clone(),
        events: events.clone(),
    };
    let (started, ready) = oneshot::channel();
    let parent = owner
        .spawn("utxo_parent", async move {
            let _child = child_owner
                .spawn("utxo_reward_child", async move {
                    let _guard = guard;
                    task_stage("pending_reward_query");
                    started.send(()).unwrap();
                    std::future::pending::<()>().await;
                })
                .unwrap();
            std::future::pending::<()>().await;
        })
        .unwrap();
    drop(parent);
    ready.await.unwrap();
    let report = owner.shutdown(Duration::from_millis(20)).await.unwrap();
    assert!(
        report
            .cancelled
            .iter()
            .any(|t| t.name == "utxo_reward_child")
    );
    assert!(report.cancelled.iter().any(|t| t.name == "utxo_parent"));
    assert_eq!(*events.lock().unwrap(), vec!["worker_stopped"]);
    pool.close().await;
    assert!(owner.active_tasks().is_empty());
}

#[tokio::test]
async fn ten_start_ready_stop_cycles_have_no_tasks_connections_or_closed_db_access() {
    let operations = Arc::new(AtomicUsize::new(0));
    for cycle in 0..10 {
        let owner = TaskSupervisor::default();
        let pool = pool().await;
        let worker_pool = pool.clone();
        let cancel = owner.cancellation_token();
        let count = operations.clone();
        let (started, ready) = oneshot::channel();
        let worker = owner
            .spawn("utxo_cycle", async move {
                sqlx::query("SELECT 1").execute(&worker_pool).await.unwrap();
                count.fetch_add(1, Ordering::SeqCst);
                started.send(()).unwrap();
                cancel.cancelled().await;
                assert!(!worker_pool.is_closed());
            })
            .unwrap();
        ready.await.unwrap();
        let report = owner.shutdown(Duration::from_secs(1)).await.unwrap();
        worker.join().await.unwrap();
        assert!(report.cancelled.is_empty());
        assert!(
            owner
                .shutdown(Duration::from_millis(20))
                .await
                .unwrap()
                .cancelled
                .is_empty(),
            "cleanup must be idempotent"
        );
        assert!(
            owner
                .spawn("late_db_work", async { panic!("late work was accepted") })
                .is_none()
        );
        pool.close().await;
        assert!(owner.active_tasks().is_empty());
        assert_eq!(pool.size(), 0);
        assert_eq!(operations.load(Ordering::SeqCst), cycle + 1);
    }
}

#[tokio::test]
async fn graceful_inflight_transaction_finishes_before_pool_closes() {
    let owner = TaskSupervisor::default();
    let pool = pool().await;
    let worker_pool = pool.clone();
    let (started, ready) = oneshot::channel();
    let worker = owner
        .spawn("utxo_transaction", async move {
            let mut tx = worker_pool.begin().await.unwrap();
            sqlx::query("CREATE TEMP TABLE lifecycle_canary (id INT) ON COMMIT DROP")
                .execute(&mut *tx)
                .await
                .unwrap();
            started.send(()).unwrap();
            sqlx::query("SELECT pg_sleep(0.05)")
                .execute(&mut *tx)
                .await
                .unwrap();
            sqlx::query("INSERT INTO lifecycle_canary VALUES (1)")
                .execute(&mut *tx)
                .await
                .unwrap();
            tx.commit().await.unwrap();
            assert!(!worker_pool.is_closed());
        })
        .unwrap();
    ready.await.unwrap();
    assert!(
        owner
            .shutdown(Duration::from_secs(1))
            .await
            .unwrap()
            .cancelled
            .is_empty()
    );
    worker.join().await.unwrap();
    pool.close().await;
}

#[tokio::test]
async fn actual_wallet_fanout_remains_owned_after_parent_set_is_dropped() {
    use kaspa_pulse::infrastructure::resilience::runtime::{OwnedTaskSet, task_supervisor};
    let pool = pool().await;
    let events = Arc::new(Mutex::new(Vec::new()));
    let guard = WorkerGuard {
        pool: pool.clone(),
        events: events.clone(),
    };
    let (started, ready) = oneshot::channel();
    let mut scans = OwnedTaskSet::new("utxo_wallet_scan");
    scans.spawn(async move {
        let _guard = guard;
        task_stage("wallet_fanout_pending_db");
        started.send(()).unwrap();
        std::future::pending::<()>().await;
    });
    ready.await.unwrap();
    drop(scans);
    let report = task_supervisor()
        .shutdown(Duration::from_millis(20))
        .await
        .unwrap();
    assert_eq!(report.cancelled[0].name, "utxo_wallet_scan");
    assert_eq!(report.cancelled[0].stage, "wallet_fanout_pending_db");
    assert_eq!(*events.lock().unwrap(), vec!["worker_stopped"]);
    pool.close().await;
}
