use super::*;
use crate::types::JobState;

pub(crate) fn expected_previous_states_for(next_state: JobState) -> &'static [JobState] {
    match next_state {
        JobState::Claimed => &[JobState::Ready],
        JobState::TronPrepared => &[JobState::Claimed],
        JobState::TronSent => &[JobState::Claimed, JobState::TronPrepared],
        JobState::ProofBuilt => &[JobState::TronSent, JobState::ProofBuilt],
        JobState::Proved => &[JobState::ProofBuilt],
        JobState::ProvedWaitingFunding => &[JobState::Proved, JobState::ProvedWaitingFunding],
        JobState::ProvedWaitingSettlement => &[
            JobState::Proved,
            JobState::ProvedWaitingFunding,
            JobState::ProvedWaitingSettlement,
        ],
        JobState::Done => &[
            JobState::Proved,
            JobState::ProvedWaitingFunding,
            JobState::ProvedWaitingSettlement,
            JobState::Done,
        ],
        JobState::Ready | JobState::FailedFatal => &[],
    }
}

pub(crate) fn expected_previous_state_names_for(next_state: JobState) -> &'static [&'static str] {
    match next_state {
        JobState::Claimed => &["ready"],
        JobState::TronPrepared => &["claimed"],
        JobState::TronSent => &["claimed", "tron_prepared"],
        JobState::ProofBuilt => &["tron_sent", "proof_built"],
        JobState::Proved => &["proof_built"],
        JobState::ProvedWaitingFunding => &["proved", "proved_waiting_funding"],
        JobState::ProvedWaitingSettlement => &[
            "proved",
            "proved_waiting_funding",
            "proved_waiting_settlement",
        ],
        JobState::Done => &[
            "proved",
            "proved_waiting_funding",
            "proved_waiting_settlement",
            "done",
        ],
        JobState::Ready | JobState::FailedFatal => &[],
    }
}

pub(crate) fn expected_state_binds_for(next_state: JobState) -> Vec<String> {
    expected_previous_states_for(next_state)
        .iter()
        .map(|s| s.as_db_str().to_string())
        .collect()
}

pub(crate) fn expected_previous_states_for_transition(
    next_state: &str,
) -> Result<&'static [&'static str]> {
    let state = JobState::parse(next_state)
        .map_err(|_| anyhow::anyhow!("unsupported record_job_state transition target: {next_state}"))?;
    if matches!(state, JobState::Ready | JobState::FailedFatal) {
        anyhow::bail!("unsupported record_job_state transition target: {next_state}")
    }
    Ok(expected_previous_state_names_for(state))
}

pub(crate) fn expected_state_binds(next_state: &str) -> Result<Vec<String>> {
    Ok(expected_previous_states_for_transition(next_state)?
        .iter()
        .map(|s| (*s).to_string())
        .collect())
}

#[cfg(test)]
fn transition_allowed(from_state: JobState, to_state: JobState) -> bool {
    expected_previous_states_for(to_state).contains(&from_state)
}

#[cfg(test)]
mod job_state_transition_tests {
    use crate::types::JobState;

    use super::{
        expected_previous_states_for_transition, expected_state_binds, transition_allowed,
    };

    #[test]
    fn transition_matrix_allows_expected_forward_edges() {
        assert!(transition_allowed(JobState::Ready, JobState::Claimed));
        assert!(transition_allowed(JobState::Claimed, JobState::TronPrepared));
        assert!(transition_allowed(JobState::Claimed, JobState::TronSent));
        assert!(transition_allowed(
            JobState::TronPrepared,
            JobState::TronSent
        ));
        assert!(transition_allowed(JobState::TronSent, JobState::ProofBuilt));
        assert!(transition_allowed(JobState::ProofBuilt, JobState::ProofBuilt));
        assert!(transition_allowed(JobState::ProofBuilt, JobState::Proved));
        assert!(transition_allowed(
            JobState::Proved,
            JobState::ProvedWaitingFunding
        ));
        assert!(transition_allowed(
            JobState::ProvedWaitingFunding,
            JobState::ProvedWaitingSettlement
        ));
        assert!(transition_allowed(
            JobState::ProvedWaitingSettlement,
            JobState::Done
        ));
        assert!(transition_allowed(JobState::Done, JobState::Done));
    }

    #[test]
    fn transition_matrix_rejects_invalid_or_regressive_edges() {
        assert!(!transition_allowed(JobState::Ready, JobState::Proved));
        assert!(!transition_allowed(JobState::Claimed, JobState::Proved));
        assert!(!transition_allowed(JobState::TronSent, JobState::Claimed));
        assert!(!transition_allowed(JobState::Done, JobState::Proved));
        assert!(!transition_allowed(JobState::FailedFatal, JobState::Done));
    }

    #[test]
    fn transition_target_validation_and_bind_encoding_are_stable() {
        let proved_waiting_settlement =
            expected_previous_states_for_transition("proved_waiting_settlement")
                .expect("known transition target");
        assert_eq!(
            proved_waiting_settlement,
            &[
                "proved",
                "proved_waiting_funding",
                "proved_waiting_settlement"
            ]
        );

        let done_binds = expected_state_binds("done").expect("done bind encoding");
        assert_eq!(
            done_binds,
            vec![
                "proved".to_string(),
                "proved_waiting_funding".to_string(),
                "proved_waiting_settlement".to_string(),
                "done".to_string()
            ]
        );

        assert!(expected_previous_states_for_transition("not_a_real_state").is_err());
        assert!(expected_state_binds("not_a_real_state").is_err());
    }
}
