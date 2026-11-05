#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RewardConfirmationStatus {
    pub confirmations: u64,
    pub is_confirmed: bool,
}

pub fn reward_confirmation_status(
    is_coinbase: bool,
    reward_daa_score: u64,
    virtual_daa_score: u64,
    required_confirmations: u64,
) -> RewardConfirmationStatus {
    let required_confirmations = required_confirmations.clamp(1, 10_000);
    let confirmations = virtual_daa_score.saturating_sub(reward_daa_score);

    RewardConfirmationStatus {
        confirmations,
        is_confirmed: !is_coinbase || confirmations >= required_confirmations,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewardProcessingDecision {
    PersistPending,
    ProcessNow,
    AlreadySeen,
    FirstRunSnapshot,
}

pub fn reward_processing_decision(
    is_first_run: bool,
    seen_before: bool,
    is_coinbase: bool,
    reward_daa_score: u64,
    virtual_daa_score: u64,
    required_confirmations: u64,
) -> RewardProcessingDecision {
    if is_first_run {
        return RewardProcessingDecision::FirstRunSnapshot;
    }

    if seen_before {
        return RewardProcessingDecision::AlreadySeen;
    }

    let status = reward_confirmation_status(
        is_coinbase,
        reward_daa_score,
        virtual_daa_score,
        required_confirmations,
    );

    if status.is_confirmed {
        RewardProcessingDecision::ProcessNow
    } else {
        RewardProcessingDecision::PersistPending
    }
}
