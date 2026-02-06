use super::*;

pub(crate) fn expected_previous_states_for_transition(
    next_state: &str,
) -> Result<&'static [&'static str]> {
    match next_state {
        "claimed" => Ok(&["ready"]),
        "tron_prepared" => Ok(&["claimed"]),
        "tron_sent" => Ok(&["claimed", "tron_prepared"]),
        "proof_built" => Ok(&["tron_sent", "proof_built"]),
        "proved" => Ok(&["proof_built"]),
        "proved_waiting_funding" => Ok(&["proved", "proved_waiting_funding"]),
        "proved_waiting_settlement" => Ok(&[
            "proved",
            "proved_waiting_funding",
            "proved_waiting_settlement",
        ]),
        "done" => Ok(&[
            "proved",
            "proved_waiting_funding",
            "proved_waiting_settlement",
            "done",
        ]),
        _ => anyhow::bail!("unsupported record_job_state transition target: {next_state}"),
    }
}

pub(crate) fn expected_state_binds(next_state: &str) -> Result<Vec<String>> {
    Ok(expected_previous_states_for_transition(next_state)?
        .iter()
        .map(|s| (*s).to_string())
        .collect())
}

#[cfg(test)]
fn transition_allowed(from_state: &str, to_state: &str) -> bool {
    expected_previous_states_for_transition(to_state)
        .map(|expected| expected.contains(&from_state))
        .unwrap_or(false)
}

#[cfg(test)]
mod job_state_transition_tests {
    use super::{
        expected_previous_states_for_transition, expected_state_binds, transition_allowed,
    };

    #[test]
    fn transition_matrix_allows_expected_forward_edges() {
        assert!(transition_allowed("ready", "claimed"));
        assert!(transition_allowed("claimed", "tron_prepared"));
        assert!(transition_allowed("claimed", "tron_sent"));
        assert!(transition_allowed("tron_prepared", "tron_sent"));
        assert!(transition_allowed("tron_sent", "proof_built"));
        assert!(transition_allowed("proof_built", "proof_built"));
        assert!(transition_allowed("proof_built", "proved"));
        assert!(transition_allowed("proved", "proved_waiting_funding"));
        assert!(transition_allowed(
            "proved_waiting_funding",
            "proved_waiting_settlement"
        ));
        assert!(transition_allowed("proved_waiting_settlement", "done"));
        assert!(transition_allowed("done", "done"));
    }

    #[test]
    fn transition_matrix_rejects_invalid_or_regressive_edges() {
        assert!(!transition_allowed("ready", "proved"));
        assert!(!transition_allowed("claimed", "proved"));
        assert!(!transition_allowed("tron_sent", "claimed"));
        assert!(!transition_allowed("done", "proved"));
        assert!(!transition_allowed("failed_fatal", "done"));
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
