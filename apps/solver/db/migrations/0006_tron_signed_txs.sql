create table if not exists solver.tron_signed_txs (
    txid bytea primary key,
    job_id bigint not null references solver.jobs(job_id) on delete cascade,
    step text not null,
    tx_bytes bytea not null,
    fee_limit_sun bigint,
    energy_required bigint,
    tx_size_bytes bigint,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),

    constraint tron_signed_txs_txid_len check (octet_length(txid) = 32)
);

create index if not exists tron_signed_txs_job_step_idx
    on solver.tron_signed_txs (job_id, step);

