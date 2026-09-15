use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use async_channel::Sender;
use async_trait::async_trait;
use kaspa_wrpc_client::prelude::{
    ChannelConnection, ChannelType, KaspaRpcClient, ListenerId, Notification, RpcState, Scope,
    VirtualDaaScoreChangedScope,
};
use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitoringMode {
    PollingOnly,
    SubscriptionPreferred,
}

impl MonitoringMode {
    pub fn from_env() -> Self {
        match std::env::var("KASPA_MONITOR_MODE")
            .unwrap_or_else(|_| "subscription_preferred".to_string())
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "polling" | "polling_only" => Self::PollingOnly,
            _ => Self::SubscriptionPreferred,
        }
    }

    pub const fn requires_subscription(self) -> bool {
        matches!(self, Self::SubscriptionPreferred)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PollingOnly => "polling_only",
            Self::SubscriptionPreferred => "subscription_preferred",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriptionState {
    Disabled,
    Disconnected,
    RegistrationRequired,
    Active,
    Degraded,
}

impl SubscriptionState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Disconnected => "disconnected",
            Self::RegistrationRequired => "registration_required",
            Self::Active => "active",
            Self::Degraded => "degraded",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorTrigger {
    Startup,
    SubscriptionNotification,
    PollFallback,
    PeriodicReconciliation,
}

impl MonitorTrigger {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::SubscriptionNotification => "subscription_notification",
            Self::PollFallback => "poll_fallback",
            Self::PeriodicReconciliation => "periodic_reconciliation",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectionTransition {
    Disabled,
    NoChange,
    InitialConnect,
    Reconnect,
    RegistrationRetry,
}

impl ConnectionTransition {
    const fn requires_registration(self) -> bool {
        matches!(
            self,
            Self::InitialConnect | Self::Reconnect | Self::RegistrationRetry
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubscriptionLifecycle {
    mode: MonitoringMode,
    state: SubscriptionState,
    connection_generation: u64,
    registration_generation: Option<u64>,
}

impl SubscriptionLifecycle {
    fn new(mode: MonitoringMode) -> Self {
        Self {
            mode,
            state: if mode.requires_subscription() {
                SubscriptionState::Disconnected
            } else {
                SubscriptionState::Disabled
            },
            connection_generation: 0,
            registration_generation: None,
        }
    }

    fn on_connected(&mut self) -> ConnectionTransition {
        if !self.mode.requires_subscription() {
            self.state = SubscriptionState::Disabled;
            return ConnectionTransition::Disabled;
        }

        match self.state {
            SubscriptionState::Disconnected => {
                let transition = if self.connection_generation == 0 {
                    ConnectionTransition::InitialConnect
                } else {
                    ConnectionTransition::Reconnect
                };
                self.connection_generation = self.connection_generation.saturating_add(1);
                self.registration_generation = None;
                self.state = SubscriptionState::RegistrationRequired;
                transition
            }
            SubscriptionState::Degraded => {
                self.state = SubscriptionState::RegistrationRequired;
                ConnectionTransition::RegistrationRetry
            }
            SubscriptionState::RegistrationRequired | SubscriptionState::Active => {
                ConnectionTransition::NoChange
            }
            SubscriptionState::Disabled => ConnectionTransition::Disabled,
        }
    }

    fn on_registration_succeeded(&mut self) -> bool {
        if !self.mode.requires_subscription() {
            return false;
        }

        self.registration_generation = Some(self.connection_generation);
        self.state = SubscriptionState::Active;
        self.connection_generation > 1
    }

    fn on_registration_failed(&mut self) {
        if self.mode.requires_subscription() {
            self.registration_generation = None;
            self.state = SubscriptionState::Degraded;
        }
    }

    fn on_disconnected(&mut self) {
        self.on_runtime_failed(false);
    }

    fn on_runtime_failed(&mut self, connected: bool) {
        if self.mode.requires_subscription() {
            self.registration_generation = None;
            self.state = if connected {
                SubscriptionState::Degraded
            } else {
                SubscriptionState::Disconnected
            };
        }
    }

    fn should_poll_fallback(self) -> bool {
        !self.mode.requires_subscription()
            || self.state != SubscriptionState::Active
            || self.registration_generation != Some(self.connection_generation)
    }

    fn should_recover_registration(self, connected: bool) -> bool {
        connected
            && self.mode.requires_subscription()
            && matches!(
                self.state,
                SubscriptionState::Disconnected | SubscriptionState::Degraded
            )
    }
}

#[derive(Debug, Clone)]
pub struct SubscriptionLifecycleHandle {
    inner: Arc<Mutex<SubscriptionLifecycle>>,
}

impl SubscriptionLifecycleHandle {
    fn new(mode: MonitoringMode) -> Self {
        Self {
            inner: Arc::new(Mutex::new(SubscriptionLifecycle::new(mode))),
        }
    }

    pub fn snapshot(&self) -> SubscriptionLifecycle {
        *lock_unpoisoned(&self.inner)
    }

    fn on_connected(&self) -> ConnectionTransition {
        lock_unpoisoned(&self.inner).on_connected()
    }

    fn on_registration_succeeded(&self) -> bool {
        lock_unpoisoned(&self.inner).on_registration_succeeded()
    }

    fn on_registration_failed(&self) {
        lock_unpoisoned(&self.inner).on_registration_failed();
    }

    fn on_disconnected(&self) {
        lock_unpoisoned(&self.inner).on_disconnected();
    }

    fn on_runtime_failed(&self, connected: bool) {
        lock_unpoisoned(&self.inner).on_runtime_failed(connected);
    }

    fn should_poll_fallback(&self) -> bool {
        lock_unpoisoned(&self.inner).should_poll_fallback()
    }
}

#[derive(Debug, Clone)]
pub struct SubscriptionSignalSender {
    sender: mpsc::Sender<()>,
    last_signal: Arc<Mutex<Option<Instant>>>,
    minimum_interval: Duration,
}

impl SubscriptionSignalSender {
    pub fn signal(&self) {
        let mut last_signal = lock_unpoisoned(&self.last_signal);
        if last_signal
            .as_ref()
            .is_some_and(|instant| instant.elapsed() < self.minimum_interval)
        {
            return;
        }

        if self.sender.try_send(()).is_ok() {
            *last_signal = Some(Instant::now());
        }
    }
}

pub struct MonitoringSchedule {
    lifecycle: SubscriptionLifecycleHandle,
    signals: mpsc::Receiver<()>,
    poll_interval: tokio::time::Interval,
    reconciliation_interval: tokio::time::Interval,
    startup_pending: bool,
    signals_open: bool,
}

impl MonitoringSchedule {
    pub fn new(
        mode: MonitoringMode,
        poll_interval: Duration,
        reconciliation_interval: Duration,
        subscription_minimum_interval: Duration,
    ) -> (Self, SubscriptionSignalSender, SubscriptionLifecycleHandle) {
        assert!(!poll_interval.is_zero(), "poll interval must be non-zero");
        assert!(
            !reconciliation_interval.is_zero(),
            "reconciliation interval must be non-zero"
        );

        let (sender, signals) = mpsc::channel(1);
        let mut poll_timer = tokio::time::interval(poll_interval);
        let mut reconciliation_timer = tokio::time::interval(reconciliation_interval);
        poll_timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
        reconciliation_timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let lifecycle = SubscriptionLifecycleHandle::new(mode);

        (
            Self {
                lifecycle: lifecycle.clone(),
                signals,
                poll_interval: poll_timer,
                reconciliation_interval: reconciliation_timer,
                startup_pending: true,
                signals_open: true,
            },
            SubscriptionSignalSender {
                sender,
                last_signal: Arc::new(Mutex::new(None)),
                minimum_interval: subscription_minimum_interval,
            },
            lifecycle,
        )
    }

    pub async fn next_trigger(
        &mut self,
        cancellation: &CancellationToken,
    ) -> Option<MonitorTrigger> {
        if cancellation.is_cancelled() {
            return None;
        }

        if self.startup_pending {
            self.startup_pending = false;
            self.poll_interval.tick().await;
            self.reconciliation_interval.tick().await;
            return Some(MonitorTrigger::Startup);
        }

        loop {
            tokio::select! {
                _ = cancellation.cancelled() => return None,
                signal = self.signals.recv(), if self.signals_open => {
                    match signal {
                        Some(()) => return Some(MonitorTrigger::SubscriptionNotification),
                        None => self.signals_open = false,
                    }
                }
                _ = self.poll_interval.tick() => {
                    if self.lifecycle.should_poll_fallback() {
                        return Some(MonitorTrigger::PollFallback);
                    }
                }
                _ = self.reconciliation_interval.tick() => {
                    return Some(MonitorTrigger::PeriodicReconciliation);
                }
            }
        }
    }
}

pub fn spawn_subscription_runtime(
    client: Arc<KaspaRpcClient>,
    signal_sender: SubscriptionSignalSender,
    lifecycle: SubscriptionLifecycleHandle,
    cancellation: CancellationToken,
) {
    let backend = Arc::new(KaspaSubscriptionRuntimeBackend { client });
    let restart_delay = Duration::from_secs(crate::infrastructure::resilience::runtime::env_u64(
        "KASPA_SUBSCRIPTION_SUPERVISOR_RESTART_SECS",
        5,
    ));

    crate::infrastructure::resilience::runtime::spawn_resilient(
        "kaspa_subscription_runtime",
        run_subscription_supervisor(
            backend,
            signal_sender,
            lifecycle,
            cancellation,
            restart_delay,
        ),
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubscriptionSessionExit {
    Cancelled,
    RpcControlChannelFailed,
    NotificationChannelFailed,
}

#[async_trait]
trait SubscriptionRuntimeBackend: Send + Sync {
    async fn run_session(
        &self,
        signal_sender: &SubscriptionSignalSender,
        lifecycle: &SubscriptionLifecycleHandle,
        cancellation: &CancellationToken,
    ) -> SubscriptionSessionExit;

    fn is_connected(&self) -> bool;
}

struct KaspaSubscriptionRuntimeBackend {
    client: Arc<KaspaRpcClient>,
}

#[async_trait]
impl SubscriptionRuntimeBackend for KaspaSubscriptionRuntimeBackend {
    async fn run_session(
        &self,
        signal_sender: &SubscriptionSignalSender,
        lifecycle: &SubscriptionLifecycleHandle,
        cancellation: &CancellationToken,
    ) -> SubscriptionSessionExit {
        let rpc_control = self.client.rpc_ctl().multiplexer().channel();
        let (notification_sender, notification_receiver) =
            async_channel::unbounded::<Notification>();
        let retry_seconds = crate::infrastructure::resilience::runtime::env_u64(
            "KASPA_SUBSCRIPTION_RETRY_SECS",
            30,
        );
        let mut retry_timer = tokio::time::interval(Duration::from_secs(retry_seconds));
        retry_timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
        retry_timer.tick().await;

        let mut listener_id: Option<ListenerId> = None;

        if self.client.is_connected() {
            handle_connected(
                &self.client,
                &notification_sender,
                lifecycle,
                &mut listener_id,
            )
            .await;
        }

        let exit = loop {
            tokio::select! {
                _ = cancellation.cancelled() => break SubscriptionSessionExit::Cancelled,
                state = rpc_control.receiver.recv() => {
                    match state {
                        Ok(RpcState::Connected) => {
                            handle_connected(
                                &self.client,
                                &notification_sender,
                                lifecycle,
                                &mut listener_id,
                            )
                            .await;
                        }
                        Ok(RpcState::Disconnected) => {
                            crate::infrastructure::observability::set_node_connected(false);
                            crate::infrastructure::observability::set_subscription_active(false);
                            lifecycle.on_disconnected();
                            unregister_listener(&self.client, &mut listener_id).await;
                            tracing::warn!("[KASPA SUBSCRIPTION] RPC disconnected; polling fallback enabled.");
                        }
                        Err(error) => {
                            tracing::error!("[KASPA SUBSCRIPTION] RPC control channel failed: {}", error);
                            break SubscriptionSessionExit::RpcControlChannelFailed;
                        }
                    }
                }
                notification = notification_receiver.recv() => {
                    match notification {
                        Ok(_) => {
                            crate::infrastructure::observability::mark_subscription_event(
                                crate::infrastructure::metrics::now_unix_secs(),
                            );
                            signal_sender.signal();
                        }
                        Err(error) => {
                            tracing::error!("[KASPA SUBSCRIPTION] Notification channel failed: {}", error);
                            break SubscriptionSessionExit::NotificationChannelFailed;
                        }
                    }
                }
                _ = retry_timer.tick() => {
                    if lifecycle
                        .snapshot()
                        .should_recover_registration(self.client.is_connected())
                    {
                        handle_connected(
                            &self.client,
                            &notification_sender,
                            lifecycle,
                            &mut listener_id,
                        )
                        .await;
                    }
                }
            }
        };

        unregister_listener(&self.client, &mut listener_id).await;
        crate::infrastructure::observability::set_subscription_active(false);
        exit
    }

    fn is_connected(&self) -> bool {
        self.client.is_connected()
    }
}

async fn run_subscription_supervisor<B>(
    backend: Arc<B>,
    signal_sender: SubscriptionSignalSender,
    lifecycle: SubscriptionLifecycleHandle,
    cancellation: CancellationToken,
    restart_delay: Duration,
) where
    B: SubscriptionRuntimeBackend + 'static,
{
    loop {
        if cancellation.is_cancelled() {
            return;
        }

        let exit = backend
            .run_session(&signal_sender, &lifecycle, &cancellation)
            .await;
        if exit == SubscriptionSessionExit::Cancelled || cancellation.is_cancelled() {
            return;
        }

        let connected = backend.is_connected();
        lifecycle.on_runtime_failed(connected);
        crate::infrastructure::observability::set_node_connected(connected);
        crate::infrastructure::observability::set_subscription_active(false);
        crate::infrastructure::observability::increment_subscription_runtime_restarts();
        tracing::warn!(
            "[KASPA SUBSCRIPTION] Runtime session ended ({:?}); polling fallback enabled and supervisor will restart it.",
            exit
        );

        tokio::select! {
            _ = cancellation.cancelled() => return,
            _ = tokio::time::sleep(restart_delay) => {}
        }
    }
}

async fn handle_connected(
    client: &Arc<KaspaRpcClient>,
    notification_sender: &Sender<Notification>,
    lifecycle: &SubscriptionLifecycleHandle,
    listener_id: &mut Option<ListenerId>,
) {
    crate::infrastructure::observability::set_node_connected(true);

    let transition = lifecycle.on_connected();
    if transition == ConnectionTransition::Reconnect {
        crate::infrastructure::observability::increment_node_reconnects();
    }
    if !transition.requires_registration() {
        return;
    }

    unregister_listener(client, listener_id).await;

    match register_listener(client, notification_sender.clone()).await {
        Ok(id) => {
            *listener_id = Some(id);
            let is_reregistration = lifecycle.on_registration_succeeded();
            crate::infrastructure::observability::set_subscription_active(true);
            if is_reregistration {
                crate::infrastructure::observability::increment_subscription_reregistrations();
            }
            let snapshot = lifecycle.snapshot();
            tracing::info!(
                "[KASPA SUBSCRIPTION] Registered generation={} state={} transition={:?}.",
                snapshot.connection_generation,
                snapshot.state.as_str(),
                transition
            );
        }
        Err(error) => {
            lifecycle.on_registration_failed();
            crate::infrastructure::observability::set_subscription_active(false);
            crate::infrastructure::observability::increment_subscription_registration_failures();
            tracing::warn!(
                "[KASPA SUBSCRIPTION] Registration failed; polling fallback remains active: {}",
                error
            );
        }
    }
}

async fn register_listener(
    client: &Arc<KaspaRpcClient>,
    notification_sender: Sender<Notification>,
) -> Result<ListenerId, String> {
    let listener_id = client
        .rpc_api()
        .register_new_listener(ChannelConnection::new(
            "kaspa-pulse-subscription",
            notification_sender,
            ChannelType::Persistent,
        ));

    if let Err(error) = client
        .rpc_api()
        .start_notify(
            listener_id,
            Scope::VirtualDaaScoreChanged(VirtualDaaScoreChangedScope {}),
        )
        .await
    {
        let _ = client.rpc_api().unregister_listener(listener_id).await;
        return Err(error.to_string());
    }

    Ok(listener_id)
}

async fn unregister_listener(client: &Arc<KaspaRpcClient>, listener_id: &mut Option<ListenerId>) {
    if let Some(id) = listener_id.take()
        && let Err(error) = client.rpc_api().unregister_listener(id).await
    {
        tracing::debug!("[KASPA SUBSCRIPTION] Listener cleanup failed: {}", error);
    }
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use async_trait::async_trait;
    use tokio::sync::Notify;

    use super::*;

    #[test]
    fn connection_transitions_distinguish_initial_retry_and_reconnect() {
        let lifecycle = SubscriptionLifecycleHandle::new(MonitoringMode::SubscriptionPreferred);

        assert_eq!(
            lifecycle.on_connected(),
            ConnectionTransition::InitialConnect
        );
        assert_eq!(lifecycle.snapshot().connection_generation, 1);
        assert_eq!(lifecycle.on_connected(), ConnectionTransition::NoChange);
        assert!(!lifecycle.on_registration_succeeded());

        lifecycle.on_registration_failed();
        assert_eq!(
            lifecycle.on_connected(),
            ConnectionTransition::RegistrationRetry
        );
        assert_eq!(lifecycle.snapshot().connection_generation, 1);
        assert!(!lifecycle.on_registration_succeeded());

        lifecycle.on_disconnected();
        assert_eq!(lifecycle.on_connected(), ConnectionTransition::Reconnect);
        assert_eq!(lifecycle.snapshot().connection_generation, 2);
        assert!(lifecycle.on_registration_succeeded());
    }

    #[test]
    fn runtime_failure_enables_polling_fallback_for_connected_client() {
        let lifecycle = SubscriptionLifecycleHandle::new(MonitoringMode::SubscriptionPreferred);
        lifecycle.on_connected();
        lifecycle.on_registration_succeeded();
        assert!(!lifecycle.should_poll_fallback());

        lifecycle.on_runtime_failed(true);

        assert!(lifecycle.should_poll_fallback());
        assert_eq!(lifecycle.snapshot().state, SubscriptionState::Degraded);
        assert_eq!(lifecycle.snapshot().registration_generation, None);
    }

    #[test]
    fn polling_only_never_requests_registration() {
        let lifecycle = SubscriptionLifecycleHandle::new(MonitoringMode::PollingOnly);

        assert_eq!(lifecycle.on_connected(), ConnectionTransition::Disabled);
        assert_eq!(lifecycle.snapshot().state, SubscriptionState::Disabled);
        assert!(lifecycle.should_poll_fallback());
    }

    #[test]
    fn disconnected_subscription_recovers_when_rpc_is_already_connected() {
        let lifecycle = SubscriptionLifecycleHandle::new(MonitoringMode::SubscriptionPreferred);

        assert!(lifecycle.snapshot().should_recover_registration(true));
        assert!(!lifecycle.snapshot().should_recover_registration(false));

        lifecycle.on_connected();
        lifecycle.on_registration_succeeded();
        assert!(!lifecycle.snapshot().should_recover_registration(true));

        lifecycle.on_disconnected();
        assert!(lifecycle.snapshot().should_recover_registration(true));

        let polling = SubscriptionLifecycleHandle::new(MonitoringMode::PollingOnly);
        assert!(!polling.snapshot().should_recover_registration(true));
    }

    struct FakeRuntimeBackend {
        attempts: AtomicUsize,
        connected: AtomicBool,
        restarted: Notify,
    }

    #[async_trait]
    impl SubscriptionRuntimeBackend for FakeRuntimeBackend {
        async fn run_session(
            &self,
            _signal_sender: &SubscriptionSignalSender,
            _lifecycle: &SubscriptionLifecycleHandle,
            cancellation: &CancellationToken,
        ) -> SubscriptionSessionExit {
            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
            if attempt == 0 {
                return SubscriptionSessionExit::RpcControlChannelFailed;
            }

            self.restarted.notify_one();
            cancellation.cancelled().await;
            SubscriptionSessionExit::Cancelled
        }

        fn is_connected(&self) -> bool {
            self.connected.load(Ordering::SeqCst)
        }
    }

    #[tokio::test]
    async fn supervisor_restarts_failed_session_and_keeps_polling_fallback_active() {
        let cancellation = CancellationToken::new();
        let (_schedule, sender, lifecycle) = MonitoringSchedule::new(
            MonitoringMode::SubscriptionPreferred,
            Duration::from_secs(5),
            Duration::from_secs(30),
            Duration::ZERO,
        );
        lifecycle.on_connected();
        lifecycle.on_registration_succeeded();
        assert!(!lifecycle.should_poll_fallback());

        let backend = Arc::new(FakeRuntimeBackend {
            attempts: AtomicUsize::new(0),
            connected: AtomicBool::new(true),
            restarted: Notify::new(),
        });
        let task = tokio::spawn(run_subscription_supervisor(
            backend.clone(),
            sender,
            lifecycle.clone(),
            cancellation.clone(),
            Duration::ZERO,
        ));

        tokio::time::timeout(Duration::from_secs(1), backend.restarted.notified())
            .await
            .expect("supervisor did not restart the failed session");

        assert!(backend.attempts.load(Ordering::SeqCst) >= 2);
        assert!(lifecycle.should_poll_fallback());
        assert_eq!(lifecycle.snapshot().state, SubscriptionState::Degraded);

        cancellation.cancel();
        task.await.expect("supervisor task join failed");
    }

    #[tokio::test(start_paused = true)]
    async fn active_subscription_suppresses_fast_polling_but_not_reconciliation() {
        let cancellation = CancellationToken::new();
        let (mut schedule, _sender, lifecycle) = MonitoringSchedule::new(
            MonitoringMode::SubscriptionPreferred,
            Duration::from_secs(5),
            Duration::from_secs(30),
            Duration::ZERO,
        );

        assert_eq!(
            schedule.next_trigger(&cancellation).await,
            Some(MonitorTrigger::Startup)
        );
        lifecycle.on_connected();
        lifecycle.on_registration_succeeded();

        tokio::time::advance(Duration::from_secs(5)).await;
        let mut pending = Box::pin(schedule.next_trigger(&cancellation));
        assert!(
            tokio::time::timeout(Duration::ZERO, &mut pending)
                .await
                .is_err()
        );

        tokio::time::advance(Duration::from_secs(25)).await;
        assert_eq!(pending.await, Some(MonitorTrigger::PeriodicReconciliation));
    }

    #[tokio::test]
    async fn subscription_notifications_are_coalesced_and_delivered() {
        let cancellation = CancellationToken::new();
        let (mut schedule, sender, _lifecycle) = MonitoringSchedule::new(
            MonitoringMode::SubscriptionPreferred,
            Duration::from_secs(60),
            Duration::from_secs(120),
            Duration::ZERO,
        );

        assert_eq!(
            schedule.next_trigger(&cancellation).await,
            Some(MonitorTrigger::Startup)
        );

        sender.signal();
        sender.signal();
        sender.signal();

        assert_eq!(
            schedule.next_trigger(&cancellation).await,
            Some(MonitorTrigger::SubscriptionNotification)
        );
    }

    #[tokio::test]
    async fn cancellation_stops_schedule_without_external_services() {
        let cancellation = CancellationToken::new();
        let (mut schedule, _sender, _lifecycle) = MonitoringSchedule::new(
            MonitoringMode::PollingOnly,
            Duration::from_secs(60),
            Duration::from_secs(120),
            Duration::ZERO,
        );

        assert_eq!(
            schedule.next_trigger(&cancellation).await,
            Some(MonitorTrigger::Startup)
        );
        cancellation.cancel();
        assert_eq!(schedule.next_trigger(&cancellation).await, None);
    }
}
