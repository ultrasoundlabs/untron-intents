create table if not exists solver.circuit_breakers (
    breaker_id bigserial primary key,
    contract bytea not null,
    selector bytea,
    fail_count integer not null default 0,
    cooldown_until timestamptz not null default now(),
    last_error text,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    unique (contract, selector)
);

create index if not exists circuit_breakers_cooldown_until_idx
    on solver.circuit_breakers (cooldown_until);

