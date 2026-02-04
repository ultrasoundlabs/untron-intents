create table if not exists solver.rental_provider_freezes (
    provider text primary key,
    frozen_until timestamptz,
    fail_count int not null default 0,
    fail_window_start timestamptz,
    last_error text,
    updated_at timestamptz not null default now()
);

create index if not exists rental_provider_freezes_frozen_until_idx
    on solver.rental_provider_freezes (frozen_until);

