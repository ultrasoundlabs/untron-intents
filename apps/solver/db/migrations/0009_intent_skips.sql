create table if not exists solver.intent_skips (
    intent_id bytea primary key,
    intent_type smallint not null,
    reason text not null,
    details jsonb,
    skip_count bigint not null default 0,
    first_seen_at timestamptz not null default now(),
    last_seen_at timestamptz not null default now()
);

create index if not exists intent_skips_last_seen_idx
    on solver.intent_skips(last_seen_at desc);
