-- Operational safety primitives for solver instances sharing a DB:
-- - global pause
-- - rate limit windows
-- - intent emulation results (for mismatch classification)
-- - delegate resource capacity reservations

create table if not exists solver.global_pause (
    id smallint primary key,
    pause_until timestamptz not null default now(),
    reason text,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

insert into solver.global_pause(id, pause_until, reason)
values (1, now(), null)
on conflict (id) do nothing;

create table if not exists solver.rate_limits (
    key text not null,
    window_start timestamptz not null,
    count bigint not null default 0,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    primary key (key, window_start)
);

create index if not exists rate_limits_window_start_idx
    on solver.rate_limits(window_start desc);

create table if not exists solver.intent_emulations (
    intent_id bytea primary key,
    intent_type smallint not null,
    ok boolean not null,
    reason text,
    contract bytea,
    selector bytea,
    checked_at timestamptz not null default now(),
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    constraint intent_emulations_intent_id_len check (octet_length(intent_id) = 32)
);

create index if not exists intent_emulations_checked_at_idx
    on solver.intent_emulations(checked_at desc);

create table if not exists solver.delegate_reservations (
    job_id bigint primary key references solver.jobs(job_id) on delete cascade,
    owner_address bytea not null,
    resource smallint not null,
    amount_sun bigint not null,
    expires_at timestamptz not null,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create index if not exists delegate_reservations_expires_at_idx
    on solver.delegate_reservations(expires_at);

create index if not exists delegate_reservations_owner_resource_idx
    on solver.delegate_reservations(owner_address, resource);

