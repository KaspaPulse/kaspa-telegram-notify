//! Fail-closed UTXO-only source selection. No RPC, Telegram or mining side effects.
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UtxoNode {
    DnsPrimary,
    KasStandby,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    Ready(UtxoNode),
    Pause,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeObservation {
    pub is_mainnet: bool,
    pub is_synced: bool,
    pub has_utxo_index: bool,
    pub connected_peers: usize,
    pub virtual_daa_score: u64,
}

impl NodeObservation {
    fn eligible(self, min_peers: usize, reference_daa: Option<u64>, max_lag: u64) -> bool {
        self.is_mainnet
            && self.is_synced
            && self.has_utxo_index
            && self.connected_peers >= min_peers
            && reference_daa
                .map(|best| best.saturating_sub(self.virtual_daa_score) <= max_lag)
                .unwrap_or(true)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FailoverPolicy {
    pub primary_failures_before_switch: u32,
    pub primary_stable_before_return: Duration,
    pub max_daa_lag: u64,
    pub min_connected_peers: usize,
}

impl Default for FailoverPolicy {
    fn default() -> Self {
        Self {
            primary_failures_before_switch: 3,
            primary_stable_before_return: Duration::from_secs(300),
            max_daa_lag: 64,
            min_connected_peers: 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceDecision {
    pub selection: Selection,
    /// Set only when the active source actually changes.
    pub transition: Option<(UtxoNode, UtxoNode)>,
}

#[derive(Debug)]
pub struct UtxoFailover {
    active: UtxoNode,
    primary_failures: u32,
    primary_healthy_since: Option<Duration>,
    highest_verified_daa: Option<u64>,
    policy: FailoverPolicy,
}

impl UtxoFailover {
    pub fn new(policy: FailoverPolicy) -> Self {
        assert!(policy.primary_failures_before_switch > 0);
        assert!(policy.min_connected_peers > 0);
        Self {
            active: UtxoNode::DnsPrimary,
            primary_failures: 0,
            primary_healthy_since: None,
            highest_verified_daa: None,
            policy,
        }
    }

    #[cfg(test)]
    pub fn active(&self) -> UtxoNode {
        self.active
    }

    /// Check the same DAA watermark used for source selection before deciding
    /// whether the backup needs a fresh health probe. A nominally synced but
    /// stale primary must not suppress standby inspection indefinitely.
    pub fn should_probe_standby(&self, primary: Option<NodeObservation>) -> bool {
        self.active == UtxoNode::KasStandby
            || !primary.is_some_and(|n| {
                n.eligible(
                    self.policy.min_connected_peers,
                    self.highest_verified_daa,
                    self.policy.max_daa_lag,
                )
            })
    }

    /// Evaluates one bounded, fresh health observation per node. Both observations
    /// must be from the same mainnet and prove sync, UTXO index and peer readiness.
    /// This method never creates multiple active readers or sends alerts.
    pub fn choose(
        &mut self,
        now: Duration,
        primary: Option<NodeObservation>,
        standby: Option<NodeObservation>,
    ) -> SourceDecision {
        // Do not allow an untrusted unsynced node to advance our DAA watermark.
        for observation in [primary, standby].into_iter().flatten() {
            if observation.is_mainnet
                && observation.is_synced
                && observation.has_utxo_index
                && observation.connected_peers >= self.policy.min_connected_peers
            {
                self.highest_verified_daa = Some(
                    self.highest_verified_daa
                        .unwrap_or(0)
                        .max(observation.virtual_daa_score),
                );
            }
        }

        let primary_ok = primary.is_some_and(|n| {
            n.eligible(
                self.policy.min_connected_peers,
                self.highest_verified_daa,
                self.policy.max_daa_lag,
            )
        });
        let standby_ok = standby.is_some_and(|n| {
            n.eligible(
                self.policy.min_connected_peers,
                self.highest_verified_daa,
                self.policy.max_daa_lag,
            )
        });

        let mut transition = None;
        match self.active {
            UtxoNode::DnsPrimary => {
                if primary_ok {
                    self.primary_failures = 0;
                    return SourceDecision {
                        selection: Selection::Ready(UtxoNode::DnsPrimary),
                        transition: None,
                    };
                }
                self.primary_failures = self.primary_failures.saturating_add(1);
                if self.primary_failures >= self.policy.primary_failures_before_switch && standby_ok
                {
                    self.active = UtxoNode::KasStandby;
                    self.primary_healthy_since = None;
                    transition = Some((UtxoNode::DnsPrimary, UtxoNode::KasStandby));
                    return SourceDecision {
                        selection: Selection::Ready(self.active),
                        transition,
                    };
                }
            }
            UtxoNode::KasStandby => {
                // Emergency failback: if the standby is down but primary is
                // independently eligible, never pause scans for hysteresis.
                if primary_ok && !standby_ok {
                    self.active = UtxoNode::DnsPrimary;
                    self.primary_failures = 0;
                    self.primary_healthy_since = None;
                    return SourceDecision {
                        selection: Selection::Ready(UtxoNode::DnsPrimary),
                        transition: Some((UtxoNode::KasStandby, UtxoNode::DnsPrimary)),
                    };
                }
                if primary_ok {
                    let since = *self.primary_healthy_since.get_or_insert(now);
                    if now.saturating_sub(since) >= self.policy.primary_stable_before_return {
                        self.active = UtxoNode::DnsPrimary;
                        self.primary_failures = 0;
                        self.primary_healthy_since = None;
                        transition = Some((UtxoNode::KasStandby, UtxoNode::DnsPrimary));
                        return SourceDecision {
                            selection: Selection::Ready(self.active),
                            transition,
                        };
                    }
                } else {
                    self.primary_healthy_since = None;
                }
                if standby_ok {
                    return SourceDecision {
                        selection: Selection::Ready(UtxoNode::KasStandby),
                        transition: None,
                    };
                }
                // No eligible node; fail closed without choosing an out-of-sync node.
            }
        }

        SourceDecision {
            selection: Selection::Pause,
            transition,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn healthy(daa: u64) -> Option<NodeObservation> {
        Some(NodeObservation {
            is_mainnet: true,
            is_synced: true,
            has_utxo_index: true,
            connected_peers: 8,
            virtual_daa_score: daa,
        })
    }
    fn tick(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    #[test]
    fn primary_is_preferred_without_alert_spam() {
        let mut f = UtxoFailover::new(FailoverPolicy::default());
        for s in 0..10 {
            let result = f.choose(tick(s), healthy(1_000), healthy(1_000));
            assert_eq!(result.selection, Selection::Ready(UtxoNode::DnsPrimary));
            assert_eq!(result.transition, None);
        }
    }
    #[test]
    fn switches_only_after_threshold_and_never_dual_reads() {
        let mut f = UtxoFailover::new(FailoverPolicy::default());
        f.choose(tick(0), healthy(1000), healthy(1000));
        for second in 1..=2 {
            assert_eq!(
                f.choose(tick(second), None, healthy(1000)).selection,
                Selection::Pause
            );
        }
        let result = f.choose(tick(3), None, healthy(1000));
        assert_eq!(result.selection, Selection::Ready(UtxoNode::KasStandby));
        assert_eq!(
            result.transition,
            Some((UtxoNode::DnsPrimary, UtxoNode::KasStandby))
        );
        assert_eq!(f.choose(tick(4), None, healthy(1000)).transition, None);
    }
    #[test]
    fn rejects_wrong_network_unsynced_or_missing_utxo_index() {
        for tweak in 0..4 {
            let mut f = UtxoFailover::new(FailoverPolicy::default());
            f.choose(tick(0), healthy(1000), healthy(1000));
            let mut standby = healthy(1000).expect("fixture");
            match tweak {
                0 => standby.is_mainnet = false,
                1 => standby.is_synced = false,
                2 => standby.has_utxo_index = false,
                _ => standby.connected_peers = 0,
            }
            for sec in 1..=3 {
                assert_eq!(
                    f.choose(tick(sec), None, Some(standby)).selection,
                    Selection::Pause
                );
            }
            assert_eq!(f.active(), UtxoNode::DnsPrimary);
        }
    }
    #[test]
    fn stale_but_nominally_synced_primary_triggers_backup_probe() {
        let mut f = UtxoFailover::new(FailoverPolicy::default());
        f.choose(tick(0), healthy(2_000), healthy(2_000));
        assert!(!f.should_probe_standby(healthy(2_000)));
        assert!(f.should_probe_standby(healthy(1_900)));
        assert!(f.should_probe_standby(None));
    }

    #[test]
    fn stale_standby_is_not_promoted() {
        let mut f = UtxoFailover::new(FailoverPolicy::default());
        f.choose(tick(0), healthy(2000), healthy(2000));
        for sec in 1..=3 {
            assert_eq!(
                f.choose(tick(sec), None, healthy(1900)).selection,
                Selection::Pause
            );
        }
    }
    #[test]
    fn waits_for_stable_primary_before_returning() {
        let mut f = UtxoFailover::new(FailoverPolicy::default());
        f.choose(tick(0), healthy(1000), healthy(1000));
        for s in 1..=3 {
            f.choose(tick(s), None, healthy(1000));
        }
        assert_eq!(
            f.choose(tick(30), healthy(1001), healthy(1001)).selection,
            Selection::Ready(UtxoNode::KasStandby)
        );
        assert_eq!(
            f.choose(tick(329), healthy(1001), healthy(1001)).selection,
            Selection::Ready(UtxoNode::KasStandby)
        );
        let result = f.choose(tick(330), healthy(1001), healthy(1001));
        assert_eq!(
            result.transition,
            Some((UtxoNode::KasStandby, UtxoNode::DnsPrimary))
        );
    }
    #[test]
    fn recovery_timer_resets_when_primary_flaps() {
        let mut f = UtxoFailover::new(FailoverPolicy::default());
        f.choose(tick(0), healthy(1000), healthy(1000));
        for s in 1..=3 {
            f.choose(tick(s), None, healthy(1000));
        }
        f.choose(tick(30), healthy(1000), healthy(1000));
        f.choose(tick(150), None, healthy(1000));
        assert_eq!(
            f.choose(tick(331), healthy(1000), healthy(1000)).selection,
            Selection::Ready(UtxoNode::KasStandby)
        );
    }
    #[test]
    fn failed_standby_returns_to_healthy_primary_immediately() {
        let mut f = UtxoFailover::new(FailoverPolicy::default());
        f.choose(tick(0), healthy(1000), healthy(1000));
        for second in 1..=3 {
            f.choose(tick(second), None, healthy(1000));
        }
        assert_eq!(f.active(), UtxoNode::KasStandby);
        let result = f.choose(tick(4), healthy(1001), None);
        assert_eq!(result.selection, Selection::Ready(UtxoNode::DnsPrimary));
        assert_eq!(
            result.transition,
            Some((UtxoNode::KasStandby, UtxoNode::DnsPrimary))
        );
    }

    #[test]
    fn if_both_nodes_fail_stop_utxo_scanning() {
        let mut f = UtxoFailover::new(FailoverPolicy::default());
        f.choose(tick(0), healthy(1000), healthy(1000));
        for s in 1..=3 {
            f.choose(tick(s), None, healthy(1000));
        }
        assert_eq!(f.choose(tick(4), None, None).selection, Selection::Pause);
    }
}
