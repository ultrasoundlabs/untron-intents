create type solver.userop_kind as enum ('claim', 'prove');

create type solver.userop_state as enum ('prepared', 'submitted', 'included', 'failed_fatal');

create table if not exists solver.hub_userops (
    userop_id bigserial primary key,

    job_id bigint not null references solver.jobs(job_id) on delete cascade,
    kind solver.userop_kind not null,

    state solver.userop_state not null default 'prepared',

    -- The fully-formed signed PackedUserOperation (JSON) so we can re-submit the exact op after a restart.
    userop jsonb not null,

    -- eth_sendUserOperation return value (0x hex string).
    userop_hash text,

    -- Once included, we discover the underlying onchain tx hash.
    tx_hash bytea,
    block_number bigint,
    success boolean,

    attempts int not null default 0,
    next_retry_at timestamptz not null default now(),
    last_error text,

    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create unique index if not exists hub_userops_job_kind_uq
    on solver.hub_userops(job_id, kind);

create index if not exists hub_userops_state_idx
    on solver.hub_userops(state, next_retry_at, updated_at);

