use std::sync::atomic::{AtomicI64, AtomicU8, Ordering};
use tracing::warn;

const STATE_CLOSED: u8 = 0;
const STATE_OPEN: u8 = 1;
const STATE_HALF_OPEN: u8 = 2;

/// Protects the system from hanging APIs (e.g., CoinGecko, Groq)
pub struct CircuitBreaker {
    state: AtomicU8,
    failure_count: AtomicU8,
    last_failure_time: AtomicI64,
    threshold: u8,
    reset_timeout_secs: i64,
}

impl CircuitBreaker {
    pub fn new(threshold: u8, reset_timeout_secs: i64) -> Self {
        Self {
            state: AtomicU8::new(STATE_CLOSED),
            failure_count: AtomicU8::new(0),
            last_failure_time: AtomicI64::new(0),
            threshold,
            reset_timeout_secs,
        }
    }

    pub fn is_allowed(&self) -> bool {
        let state = self.state.load(Ordering::SeqCst);
        if state == STATE_CLOSED {
            return true;
        }

        let now = chrono::Utc::now().timestamp();
        let last_fail = self.last_failure_time.load(Ordering::SeqCst);

        if state == STATE_OPEN && (now - last_fail) > self.reset_timeout_secs {
            self.state.store(STATE_HALF_OPEN, Ordering::SeqCst);
            return true;
        }
        false
    }

    pub fn record_success(&self) {
        self.failure_count.store(0, Ordering::SeqCst);
        self.state.store(STATE_CLOSED, Ordering::SeqCst);
    }

    pub fn record_failure(&self) {
        let count = self.failure_count.fetch_add(1, Ordering::SeqCst) + 1;
        self.last_failure_time
            .store(chrono::Utc::now().timestamp(), Ordering::SeqCst);

        if count >= self.threshold {
            warn!("🛑 [CIRCUIT BREAKER] Tripped! External API is failing. Blocking requests...");
            self.state.store(STATE_OPEN, Ordering::SeqCst);
        }
    }
}
