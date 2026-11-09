//! OP074: single in-process selection authority shared by the wallet
//! transaction writer and the durable VSPC acceptance indexer.
//!
//! This release does NOT mutate any public PNN endpoint URL. A future hot
//! replacement must first also move the cached per-slot UTXO services,
//! the primary-standby observer reference and the durable VSPC generation.
use super::kaspa_adapter::KaspaRpcAdapter;
use std::sync::Arc;
use tokio::sync::{RwLock, RwLockReadGuard};

struct RegistryState {
    generation: u64,
    selected_index: Option<usize>,
    sources: Vec<Arc<KaspaRpcAdapter>>,
}
pub struct SourceRegistry {
    state: RwLock<RegistryState>,
}
#[derive(Clone)]
pub struct SelectedPnn {
    pub source: Arc<KaspaRpcAdapter>,
    pub index: usize,
    pub generation: u64,
}

/// Holds the exact generation and selected source fixed while a VSPC
/// checkpoint and its proof metadata are committed atomically to PostgreSQL.
pub struct SourceFence<'a> {
    _hold: RwLockReadGuard<'a, RegistryState>,
}
impl SourceRegistry {
    pub fn new(nodes: Vec<Arc<KaspaRpcAdapter>>) -> Self {
        assert_eq!(nodes.len(), 5, "five-source PNN registry expected");
        Self {
            state: RwLock::new(RegistryState {
                generation: 0,
                selected_index: None,
                sources: nodes,
            }),
        }
    }
    pub async fn select(&self, index: Option<usize>) -> Result<u64, &'static str> {
        let mut state = self.state.write().await;
        if index.is_some_and(|i| i >= state.sources.len()) {
            return Err("selected source outside qualified public pool");
        }
        if state.selected_index != index {
            state.selected_index = index;
            state.generation = state.generation.saturating_add(1);
        }
        Ok(state.generation)
    }
    /// One immutable snapshot of ALL five public PNN pointers plus selected
    /// writer/epoch. The wallet scanner owns updating its per-source
    /// UtxoMonitorService cache in the same serial task after a replacement.
    pub async fn snapshot(&self) -> (u64, Option<usize>, Vec<Arc<KaspaRpcAdapter>>) {
        let state = self.state.read().await;
        (
            state.generation,
            state.selected_index,
            state.sources.clone(),
        )
    }

    /// OP075: publish a new PNN node ONLY in an INACTIVE slot, under the
    /// exact expected epoch and previous pointer. Reject duplicate URLs,
    /// stale discoveries, unqualified callers, or selected writer races.
    /// VSPC holds the same registry read-fence across its PG checkpoint
    /// commit, so it cannot observe a split-brain checkpoint during a swap.
    pub async fn replace_inactive(
        &self,
        expected_generation: u64,
        slot: usize,
        expected_old: &Arc<KaspaRpcAdapter>,
        replacement: Arc<KaspaRpcAdapter>,
        independently_qualified: bool,
    ) -> Result<u64, &'static str> {
        if !independently_qualified {
            return Err("candidate not independently qualified by coeval DAA and VSPC");
        }
        let new_url = replacement
            .current_or_pinned_url()
            .ok_or("candidate URL unavailable")?;
        let mut state = self.state.write().await;
        if state.generation != expected_generation {
            return Err("source generation changed during public resolver discovery");
        }
        if slot >= state.sources.len() {
            return Err("replacement slot outside five-source pool");
        }
        if state.selected_index == Some(slot) {
            return Err("replacement of current transactional writer prohibited");
        }
        if !Arc::ptr_eq(&state.sources[slot], expected_old) {
            return Err("replacement target changed while candidate was discovered");
        }
        if state
            .sources
            .iter()
            .any(|source| source.current_or_pinned_url().as_deref() == Some(new_url.as_str()))
        {
            return Err("duplicate public PNN URL refused");
        }
        let next_generation = state
            .generation
            .checked_add(1)
            .ok_or("source generation overflow")?;
        state.sources[slot] = replacement;
        state.generation = next_generation;
        Ok(state.generation)
    }

    pub async fn selected(&self) -> Option<SelectedPnn> {
        let state = self.state.read().await;
        let index = state.selected_index?;
        Some(SelectedPnn {
            source: state.sources.get(index)?.clone(),
            index,
            generation: state.generation,
        })
    }
    pub async fn fence(&self, captured: &SelectedPnn) -> Option<SourceFence<'_>> {
        let state = self.state.read().await;
        if state.generation != captured.generation
            || state.selected_index != Some(captured.index)
            || !Arc::ptr_eq(state.sources.get(captured.index)?, &captured.source)
        {
            return None;
        }
        Some(SourceFence { _hold: state })
    }
    #[cfg(test)]
    pub async fn generation(&self) -> u64 {
        self.state.read().await.generation
    }
}

#[cfg(test)]
mod op074_shared_source_generation_tests {
    use super::*;
    use kaspa_consensus_core::network::NetworkId;
    use kaspa_wrpc_client::{KaspaRpcClient, WrpcEncoding};
    use std::str::FromStr;
    use std::time::Duration;

    fn registry() -> Arc<SourceRegistry> {
        let id = NetworkId::from_str("mainnet").unwrap();
        let sources = (0u16..5)
            .map(|i| {
                let url = format!("ws://127.0.0.1:{}", 19501 + i);
                let client =
                    KaspaRpcClient::new(WrpcEncoding::Borsh, Some(&url), None, Some(id), None)
                        .expect("synthetic unconnected local-only adapter");
                Arc::new(KaspaRpcAdapter::new_pinned(Arc::new(client), url))
            })
            .collect();
        Arc::new(SourceRegistry::new(sources))
    }
    #[tokio::test]
    async fn indexer_fence_prevents_change_of_selected_source_during_checkpoint() {
        let registry = registry();
        assert!(registry.selected().await.is_none());
        let first = registry.select(Some(2)).await.unwrap();
        let captured = registry.selected().await.unwrap();
        assert_eq!(captured.index, 2);
        assert_eq!(captured.generation, first);
        let hold = registry
            .fence(&captured)
            .await
            .expect("generation remains valid");
        let for_task = registry.clone();
        let task = tokio::spawn(async move { for_task.select(Some(3)).await.unwrap() });
        // The scanner must WAIT while indexer owns an immutable VSPC commit
        // fence, ensuring no wrong-source acceptance cursor is committed.
        tokio::task::yield_now().await;
        assert!(
            !task.is_finished(),
            "source changed while VSPC read-fence held"
        );
        drop(hold);
        let next = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(next, first + 1);
        assert!(registry.fence(&captured).await.is_none());
        let new_source = registry.selected().await.unwrap();
        assert_eq!(new_source.index, 3);
        assert_eq!(new_source.generation, next);
    }
    #[tokio::test]
    async fn invalid_selection_does_not_mutate_epoch_and_reselect_is_idempotent() {
        let registry = registry();
        assert!(registry.select(Some(9)).await.is_err());
        assert_eq!(registry.generation().await, 0);
        let first = registry.select(Some(1)).await.unwrap();
        assert_eq!(first, registry.select(Some(1)).await.unwrap());
        assert_eq!(registry.generation().await, first);
        let captured = registry.selected().await.unwrap();
        assert!(registry.fence(&captured).await.is_some());
        assert_eq!(registry.select(None).await.unwrap(), first + 1);
        assert!(registry.selected().await.is_none());
        assert!(registry.fence(&captured).await.is_none());
    }

    fn synthetic_adapter(port: u16) -> Arc<KaspaRpcAdapter> {
        let id = NetworkId::from_str("mainnet").unwrap();
        let url = format!("ws://127.0.0.1:{port}");
        let client =
            KaspaRpcClient::new(WrpcEncoding::Borsh, Some(&url), None, Some(id), None).unwrap();
        Arc::new(KaspaRpcAdapter::new_pinned(Arc::new(client), url))
    }

    #[tokio::test]
    async fn inactive_source_zero_can_recover_while_selected_writer_stays_untouched() {
        let registry = registry();
        let writer_epoch = registry.select(Some(2)).await.unwrap();
        let (_, selected, pinned) = registry.snapshot().await;
        assert_eq!(selected, Some(2));
        let writer = pinned[2].clone();
        let old = pinned[0].clone();
        let candidate = synthetic_adapter(19999);
        let next = registry
            .replace_inactive(writer_epoch, 0, &old, candidate.clone(), true)
            .await
            .unwrap();
        assert_eq!(next, writer_epoch + 1);
        let (_, selected_after, now) = registry.snapshot().await;
        assert_eq!(selected_after, Some(2));
        assert!(Arc::ptr_eq(&now[2], &writer));
        assert!(Arc::ptr_eq(&now[0], &candidate));
        assert!(!Arc::ptr_eq(&now[0], &old));
    }

    #[tokio::test]
    async fn replacement_refuses_active_writer_duplicate_unqualified_and_stale_epoch() {
        let registry = registry();
        let writer_epoch = registry.select(Some(3)).await.unwrap();
        let (_, _, pinned) = registry.snapshot().await;
        let candidate = synthetic_adapter(19888);
        assert!(
            registry
                .replace_inactive(writer_epoch, 3, &pinned[3], candidate.clone(), true)
                .await
                .is_err()
        );
        assert!(
            registry
                .replace_inactive(writer_epoch, 1, &pinned[1], candidate.clone(), false)
                .await
                .is_err()
        );
        assert!(
            registry
                .replace_inactive(writer_epoch, 1, &pinned[1], pinned[2].clone(), true)
                .await
                .is_err()
        );
        assert!(
            registry
                .replace_inactive(writer_epoch - 1, 1, &pinned[1], candidate.clone(), true)
                .await
                .is_err()
        );
        let success_epoch = registry
            .replace_inactive(writer_epoch, 1, &pinned[1], candidate.clone(), true)
            .await
            .unwrap();
        assert!(
            registry
                .replace_inactive(writer_epoch, 2, &pinned[2], synthetic_adapter(19889), true)
                .await
                .is_err()
        );
        assert!(
            registry
                .replace_inactive(success_epoch, 1, &pinned[1], synthetic_adapter(19889), true)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn vspc_commit_fence_prevents_background_public_url_swap() {
        let registry = registry();
        let epoch = registry.select(Some(2)).await.unwrap();
        let selection = registry.selected().await.unwrap();
        let fenced = registry.fence(&selection).await.unwrap();
        let (_, _, old) = registry.snapshot().await;
        let old = old[0].clone();
        let remote = registry.clone();
        let pending = tokio::spawn(async move {
            remote
                .replace_inactive(epoch, 0, &old, synthetic_adapter(19666), true)
                .await
                .unwrap()
        });
        tokio::task::yield_now().await;
        assert!(
            !pending.is_finished(),
            "VSPC commit fence must block URL replacement"
        );
        drop(fenced);
        let new_epoch = tokio::time::timeout(Duration::from_secs(2), pending)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(new_epoch, epoch + 1);
        assert!(
            registry.fence(&selection).await.is_none(),
            "old VSPC response must be rejected after rotation"
        );
    }
}
