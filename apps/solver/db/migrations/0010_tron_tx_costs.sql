create table if not exists solver.tron_tx_costs (
    txid bytea primary key,
    job_id bigint references solver.jobs(job_id) on delete set null,

    fee_sun bigint,
    energy_usage_total bigint,
    net_usage bigint,
    energy_fee_sun bigint,
    net_fee_sun bigint,

    block_number bigint,
    block_timestamp bigint,
    result_code int,
    result_message text,

    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create index if not exists tron_tx_costs_job_id_idx
    on solver.tron_tx_costs(job_id);

create index if not exists tron_tx_costs_updated_at_idx
    on solver.tron_tx_costs(updated_at desc);
