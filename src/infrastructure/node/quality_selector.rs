//! Adaptive public PNN source selection for the ONE transactional UTXO reader.
//! A source must be independently synced, indexed and fresh. Quality includes
//! latency, peers, chain tip freshness and recent failures. No network I/O here.
use std::time::Duration;

pub const MAX_PUBLIC_PNN_SOURCES: usize = 5;
const MAX_LAG_DAA: u64 = 64;
const MIN_GAIN: i64 = 45;
const MIN_STABILITY: Duration = Duration::from_secs(90);
const STRONGER_OBSERVATIONS: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quality {
    pub is_mainnet: bool,
    pub is_synced: bool,
    pub has_utxo_index: bool,
    pub peers: usize,
    pub virtual_daa: u64,
    pub latency_ms: u64,
}

impl Quality {
    fn prelim_eligible(self) -> bool {
        self.is_mainnet
            && self.is_synced
            && self.has_utxo_index
            && self.peers > 0
            && self.virtual_daa > 0
    }
    fn score(self, best_daa: u64, consecutive_failures: u32) -> Option<i64> {
        if !self.prelim_eligible() || best_daa.saturating_sub(self.virtual_daa) > MAX_LAG_DAA {
            return None;
        }
        let lag = best_daa.saturating_sub(self.virtual_daa);
        Some(
            (self.peers.min(80) as i64) * 3
                - ((self.latency_ms.min(5000) / 20) as i64)
                - (lag as i64) * 4
                - (consecutive_failures.min(5) as i64) * 20,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choice {
    pub active_index: Option<usize>,
    pub transition: Option<(usize, usize)>,
    pub eligible_count: usize,
    /// Bit i reflects eligibility of pinned independent source i, including
    /// mainnet/index/sync/peer and the same DAA freshness as selection.
    /// No IPs or public resolver URLs are exported.
    pub eligible_slots_mask: u8,
}

/// All methods are called serially by the one UTXO scanner. No hidden tasks.
pub struct QualitySelector {
    active: Option<usize>,
    failures: Vec<u32>,
    stronger_candidate: Option<usize>,
    stronger_streak: u32,
    last_transition: Duration,
}

impl QualitySelector {
    pub fn new(source_count: usize) -> Self {
        assert!((2..=MAX_PUBLIC_PNN_SOURCES).contains(&source_count));
        Self {
            active: None,
            failures: vec![0; source_count],
            stronger_candidate: None,
            stronger_streak: 0,
            last_transition: Duration::ZERO,
        }
    }

    pub fn choose(&mut self, now: Duration, samples: &[Option<Quality>]) -> Choice {
        assert_eq!(samples.len(), self.failures.len());
        let best_daa = samples
            .iter()
            .flatten()
            .filter(|s| s.prelim_eligible())
            .map(|s| s.virtual_daa)
            .max()
            .unwrap_or(0);
        let scored: Vec<Option<i64>> = samples
            .iter()
            .enumerate()
            .map(|(i, sample)| {
                match sample.and_then(|s| s.score(best_daa, self.failures[i])) {
                    Some(score) => {
                        self.failures[i] = 0; // Fresh probe was successful.
                        Some(score)
                    }
                    None => {
                        self.failures[i] = self.failures[i].saturating_add(1);
                        None
                    }
                }
            })
            .collect();
        let eligible_count = scored.iter().flatten().count();
        let eligible_slots_mask = scored.iter().enumerate().fold(0u8, |bits, (slot, score)| {
            if score.is_some() {
                bits | (1u8 << slot)
            } else {
                bits
            }
        });
        debug_assert_eq!(eligible_count, eligible_slots_mask.count_ones() as usize);
        let best = scored
            .iter()
            .enumerate()
            .filter_map(|(i, score)| score.map(|v| (i, v)))
            .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)));
        let Some((best_index, best_score)) = best else {
            self.stronger_candidate = None;
            self.stronger_streak = 0;
            return Choice {
                active_index: None,
                transition: None,
                eligible_count,
                eligible_slots_mask,
            };
        };
        let current_ok = self
            .active
            .and_then(|idx| scored[idx].map(|score| (idx, score)));
        let proposed = if let Some((current_idx, current_score)) = current_ok {
            if best_index == current_idx
                || best_score < current_score.saturating_add(MIN_GAIN)
                || now.saturating_sub(self.last_transition) < MIN_STABILITY
            {
                self.stronger_candidate = None;
                self.stronger_streak = 0;
                current_idx
            } else {
                if self.stronger_candidate == Some(best_index) {
                    self.stronger_streak = self.stronger_streak.saturating_add(1);
                } else {
                    self.stronger_candidate = Some(best_index);
                    self.stronger_streak = 1;
                }
                if self.stronger_streak >= STRONGER_OBSERVATIONS {
                    best_index
                } else {
                    current_idx
                }
            }
        } else {
            // The current source failed or is stale: promote the strongest
            // independently qualified source immediately, never retry stale UTXOs.
            self.stronger_candidate = None;
            self.stronger_streak = 0;
            best_index
        };
        let transition = self
            .active
            .filter(|old| *old != proposed)
            .map(|old| (old, proposed));
        if transition.is_some() || self.active.is_none() {
            self.last_transition = now;
            self.stronger_candidate = None;
            self.stronger_streak = 0;
        }
        self.active = Some(proposed);
        Choice {
            active_index: Some(proposed),
            transition,
            eligible_count,
            eligible_slots_mask,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn q(peers: usize, daa: u64, latency_ms: u64) -> Option<Quality> {
        Some(Quality {
            is_mainnet: true,
            is_synced: true,
            has_utxo_index: true,
            peers,
            virtual_daa: daa,
            latency_ms,
        })
    }
    #[test]
    fn public_source_slots_mask_uses_real_fresh_qualification() {
        let mut s = QualitySelector::new(5);
        let choice = s.choose(
            Duration::ZERO,
            &[q(10, 1000, 10), None, q(30, 1000, 15), None, q(20, 800, 10)],
        );
        assert_eq!(choice.eligible_count, 2);
        assert_eq!(choice.eligible_slots_mask, 0b00101);
        assert_eq!(choice.active_index, Some(2));
        let no_sources = s.choose(Duration::from_secs(2), &[None; 5]);
        assert_eq!(no_sources.eligible_count, 0);
        assert_eq!(no_sources.eligible_slots_mask, 0);
        assert_eq!(no_sources.active_index, None);
    }
    #[test]
    fn picks_strongest_fresh_public_source_not_arbitrary_primary() {
        let mut s = QualitySelector::new(5);
        let choice = s.choose(
            Duration::ZERO,
            &[
                q(10, 1000, 300),
                q(45, 1000, 60),
                q(25, 1000, 80),
                q(5, 1000, 40),
                q(8, 1000, 300),
            ],
        );
        assert_eq!(choice.active_index, Some(1));
        assert_eq!(choice.eligible_count, 5);
    }
    #[test]
    fn advances_through_all_five_after_failures_without_dual_reader() {
        let mut s = QualitySelector::new(5);
        let mut data = [
            q(80, 1000, 20),
            q(70, 1000, 20),
            q(60, 1000, 20),
            q(50, 1000, 20),
            q(40, 1000, 20),
        ];
        for i in 0..5 {
            let d = s.choose(Duration::from_secs(i as u64 * 10), &data);
            assert_eq!(d.active_index, Some(i));
            if i < 4 {
                data[i] = None;
            }
        }
        assert_eq!(
            s.choose(Duration::from_secs(55), &[None; 5]).active_index,
            None
        );
    }
    #[test]
    fn rejects_stale_wrong_network_unindexed_and_unsynced_sources() {
        let mut s = QualitySelector::new(5);
        let mut wrong = q(100, 1000, 1).unwrap();
        wrong.is_mainnet = false;
        let mut unsynced = q(100, 1000, 1).unwrap();
        unsynced.is_synced = false;
        let mut unindexed = q(100, 1000, 1).unwrap();
        unindexed.has_utxo_index = false;
        assert_eq!(
            s.choose(
                Duration::ZERO,
                &[
                    Some(wrong),
                    Some(unsynced),
                    Some(unindexed),
                    q(20, 900, 4),
                    q(30, 1000, 80)
                ]
            )
            .active_index,
            Some(4)
        );
    }
    #[test]
    fn never_flaps_to_marginally_better_or_transient_endpoint() {
        let mut s = QualitySelector::new(5);
        let mut data = [
            q(60, 1000, 40),
            q(20, 1000, 50),
            q(20, 1000, 50),
            q(20, 1000, 50),
            q(20, 1000, 50),
        ];
        assert_eq!(s.choose(Duration::ZERO, &data).active_index, Some(0));
        data[1] = q(80, 1000, 1);
        for sec in [1, 90, 100] {
            assert_eq!(
                s.choose(Duration::from_secs(sec), &data).active_index,
                Some(0)
            );
        }
        assert_eq!(
            s.choose(Duration::from_secs(110), &data).active_index,
            Some(1)
        );
        data[0] = q(80, 1000, 1);
        assert_eq!(
            s.choose(Duration::from_secs(120), &data).active_index,
            Some(1)
        );
    }
    #[test]
    fn high_peer_count_cannot_override_lag_and_no_eligible_pauses() {
        let mut s = QualitySelector::new(5);
        assert_eq!(
            s.choose(
                Duration::ZERO,
                &[q(1000, 900, 1), q(2, 1000, 70), None, None, None]
            )
            .active_index,
            Some(1)
        );
        assert_eq!(
            s.choose(Duration::from_secs(10), &[None; 5]).active_index,
            None
        );
    }
}
