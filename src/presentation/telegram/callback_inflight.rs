use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CallbackExecutionKey {
    pub chat_id: i64,
    pub message_id: i32,
}

impl CallbackExecutionKey {
    pub const fn new(chat_id: i64, message_id: i32) -> Self {
        Self {
            chat_id,
            message_id,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CallbackExecutionRegistry {
    active: Arc<Mutex<HashSet<CallbackExecutionKey>>>,
}

impl CallbackExecutionRegistry {
    pub fn try_acquire(&self, key: CallbackExecutionKey) -> Option<CallbackExecutionGuard> {
        let mut active = lock_unpoisoned(&self.active);
        if !active.insert(key.clone()) {
            return None;
        }

        Some(CallbackExecutionGuard {
            active: Arc::clone(&self.active),
            key: Some(key),
        })
    }
}

#[derive(Debug)]
pub struct CallbackExecutionGuard {
    active: Arc<Mutex<HashSet<CallbackExecutionKey>>>,
    key: Option<CallbackExecutionKey>,
}

impl Drop for CallbackExecutionGuard {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            lock_unpoisoned(&self.active).remove(&key);
        }
    }
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> CallbackExecutionKey {
        CallbackExecutionKey::new(-100, 55)
    }

    #[test]
    fn duplicate_callback_is_rejected_while_first_execution_is_active() {
        let registry = CallbackExecutionRegistry::default();
        let _first = registry
            .try_acquire(key())
            .expect("first acquisition must succeed");
        assert!(registry.try_acquire(key()).is_none());
    }

    #[test]
    fn callback_is_available_again_after_guard_is_dropped() {
        let registry = CallbackExecutionRegistry::default();
        {
            let _guard = registry
                .try_acquire(key())
                .expect("first acquisition must succeed");
        }
        assert!(registry.try_acquire(key()).is_some());
    }

    #[test]
    fn another_button_on_the_same_message_is_blocked() {
        let registry = CallbackExecutionRegistry::default();
        let _first = registry
            .try_acquire(CallbackExecutionKey::new(-100, 55))
            .expect("first acquisition must succeed");
        assert!(
            registry
                .try_acquire(CallbackExecutionKey::new(-100, 55))
                .is_none()
        );
    }

    #[test]
    fn same_action_from_different_message_is_not_blocked() {
        let registry = CallbackExecutionRegistry::default();
        let _first = registry
            .try_acquire(CallbackExecutionKey::new(-100, 55))
            .expect("first acquisition must succeed");
        assert!(
            registry
                .try_acquire(CallbackExecutionKey::new(-100, 56))
                .is_some()
        );
    }
}
