create table if not exists solver.jobs (
    job_id bigserial primary key,
    intent_id bytea not null,
    intent_type smallint not null,
    intent_specs bytea not null,
    deadline bigint not null,

    state text not null,
    attempts int not null default 0,
    next_retry_at timestamptz not null default now(),
    last_error text,

    leased_by text,
    lease_until timestamptz,

    claim_tx_hash bytea,
    prove_tx_hash bytea,
    tron_txid bytea,

    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),

    constraint jobs_intent_id_len check (octet_length(intent_id) = 32),
    constraint jobs_unique_intent unique (intent_id)
);

create index if not exists jobs_state_idx on solver.jobs(state);
create index if not exists jobs_next_retry_idx on solver.jobs(next_retry_at);
create index if not exists jobs_lease_until_idx on solver.jobs(lease_until);

