//! OP075: a cautiously corroborated public PNN DAA reference.
//! Used only to qualify proposed INACTIVE replacement with coeval RPC observations.
use std::collections::HashSet;

pub const MAX_DAA_SPREAD: u64 = 64;
pub const MAX_SAMPLING_SKEW_MS: u64 = 5_000;
pub const MIN_REFERENCE_QUORUM: usize = 3;

#[derive(Debug, Clone, Copy)]
pub struct DaaSample {
    /// The caller verifies genuinely distinct pinned resolver URLs.
    pub source_id: usize,
    pub observed_at_mono_ms: u64,
    pub daa: u64,
    pub mainnet: bool,
    pub synced: bool,
    pub indexed: bool,
    pub peers: usize,
}
impl DaaSample {
    fn qualified(self) -> bool {
        self.daa > 0 && self.mainnet && self.synced && self.indexed && self.peers > 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceError {
    DuplicateEndpoint,
    NotEnoughIndependentSources,
    SequentialMeasurementsNotComparable,
    NoCorroboratedTip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reference {
    /// Highest DAA backed by three different eligible URL observations
    /// within 64 DAA. This is only cross-provider agreement, NOT a direct
    /// chain-wide absolute truth anchor or physical operator diversity.
    pub corroborated_daa: u64,
    pub corroborating_sources: usize,
    pub sample_cohort_span_ms: u64,
}

pub fn find_corroborated_reference(samples: &[DaaSample]) -> Result<Reference, ReferenceError> {
    let mut urls = HashSet::new();
    for sample in samples {
        if !urls.insert(sample.source_id) {
            return Err(ReferenceError::DuplicateEndpoint);
        }
    }
    let good: Vec<_> = samples.iter().copied().filter(|s| s.qualified()).collect();
    if good.len() < MIN_REFERENCE_QUORUM {
        return Err(ReferenceError::NotEnoughIndependentSources);
    }
    let start = good.iter().map(|x| x.observed_at_mono_ms).min().unwrap();
    let end = good.iter().map(|x| x.observed_at_mono_ms).max().unwrap();
    let cohort_span = end.saturating_sub(start);
    if cohort_span > MAX_SAMPLING_SKEW_MS {
        return Err(ReferenceError::SequentialMeasurementsNotComparable);
    }
    let mut candidates: Vec<_> = good.iter().map(|s| s.daa).collect();
    candidates.sort_unstable_by(|a, b| b.cmp(a));
    candidates.dedup();
    for tip in candidates {
        let support = good
            .iter()
            .filter(|s| tip >= s.daa && tip.saturating_sub(s.daa) <= MAX_DAA_SPREAD)
            .count();
        if support >= MIN_REFERENCE_QUORUM {
            return Ok(Reference {
                corroborated_daa: tip,
                corroborating_sources: support,
                sample_cohort_span_ms: cohort_span,
            });
        }
    }
    Err(ReferenceError::NoCorroboratedTip)
}

pub fn within_verified_daa_window(reference: &Reference, candidate: DaaSample) -> bool {
    candidate.qualified() && reference.corroborated_daa.abs_diff(candidate.daa) <= MAX_DAA_SPREAD
}

#[cfg(test)]
mod tests {
    use super::*;
    fn s(id: usize, daa: u64, t: u64) -> DaaSample {
        DaaSample {
            source_id: id,
            daa,
            observed_at_mono_ms: t,
            mainnet: true,
            synced: true,
            indexed: true,
            peers: 8,
        }
    }
    #[test]
    fn rejects_raw_daa_collected_during_hundred_second_resolver_sweep() {
        let samples = [s(0, 2_000, 0), s(1, 2_300, 30_000), s(2, 2_800, 80_000)];
        assert_eq!(
            find_corroborated_reference(&samples),
            Err(ReferenceError::SequentialMeasurementsNotComparable)
        );
    }
    #[test]
    fn accepts_parallel_independent_three_source_cohort() {
        let samples = [
            s(0, 2_000, 50),
            s(1, 2_018, 70),
            s(2, 2_030, 90),
            s(3, 1700, 75),
        ];
        let r = find_corroborated_reference(&samples).unwrap();
        assert_eq!(r.corroborated_daa, 2_030);
        assert_eq!(r.corroborating_sources, 3);
        assert!(within_verified_daa_window(&r, s(0, 2_000, 52)));
        assert!(!within_verified_daa_window(&r, s(3, 1700, 75)));
    }
    #[test]
    fn rejects_single_unjustified_high_daa_claim_from_an_outlier() {
        let samples = [
            s(0, 2_000, 30),
            s(1, 2_015, 40),
            s(2, 2_021, 42),
            s(3, 30_000, 35),
        ];
        let r = find_corroborated_reference(&samples).unwrap();
        assert_eq!(r.corroborated_daa, 2_021);
        assert_eq!(r.corroborating_sources, 3);
        assert!(!within_verified_daa_window(&r, s(3, 30_000, 35)));
    }
    #[test]
    fn refuses_false_quorum_of_identical_urls_or_two_sources() {
        assert_eq!(
            find_corroborated_reference(&[s(0, 100, 10), s(0, 100, 10), s(1, 100, 10)]),
            Err(ReferenceError::DuplicateEndpoint)
        );
        assert_eq!(
            find_corroborated_reference(&[s(0, 100, 10), s(1, 100, 10)]),
            Err(ReferenceError::NotEnoughIndependentSources)
        );
    }
    #[test]
    fn rejects_three_disagreeing_sources_or_unsynced_samples() {
        assert_eq!(
            find_corroborated_reference(&[s(0, 100, 10), s(1, 300, 20), s(2, 500, 30)]),
            Err(ReferenceError::NoCorroboratedTip)
        );
        let mut stale = s(2, 100, 10);
        stale.synced = false;
        assert_eq!(
            find_corroborated_reference(&[s(0, 100, 10), s(1, 100, 10), stale]),
            Err(ReferenceError::NotEnoughIndependentSources)
        );
    }
    #[test]
    fn clock_skew_boundary_is_closed_not_silently_accepted() {
        assert!(
            find_corroborated_reference(&[s(0, 100, 0), s(1, 100, 5000), s(2, 100, 2500)]).is_ok()
        );
        assert_eq!(
            find_corroborated_reference(&[s(0, 100, 0), s(1, 100, 5001), s(2, 100, 2500)]),
            Err(ReferenceError::SequentialMeasurementsNotComparable)
        );
    }
}
