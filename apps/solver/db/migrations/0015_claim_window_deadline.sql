alter table solver.jobs
    add column if not exists claim_window_expires_at timestamptz;

create index if not exists jobs_claim_window_state_expiry_idx
    on solver.jobs(state, claim_window_expires_at)
    where state in ('claimed', 'tron_prepared', 'tron_sent', 'proof_built');
