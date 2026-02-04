create table if not exists solver.tron_rentals (
    job_id bigint primary key references solver.jobs(job_id) on delete cascade,
    provider text not null,
    resource text not null,
    receiver_evm bytea not null,
    balance_sun bigint not null,
    lock_period bigint not null,
    order_id text,
    txid bytea,
    request_json jsonb,
    response_json jsonb,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),

    constraint tron_rentals_receiver_len check (octet_length(receiver_evm) = 20),
    constraint tron_rentals_txid_len check (txid is null or octet_length(txid) = 32)
);

create index if not exists tron_rentals_txid_idx on solver.tron_rentals (txid);

