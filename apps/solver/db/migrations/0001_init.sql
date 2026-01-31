create schema if not exists solver;

create table if not exists solver.intent_runs (
    intent_id bytea primary key,
    state text not null,
    claim_tx_hash bytea,
    prove_tx_hash bytea,
    last_error text,
    updated_at timestamptz not null default now(),
    created_at timestamptz not null default now()
);

create index if not exists intent_runs_state_idx on solver.intent_runs(state);
