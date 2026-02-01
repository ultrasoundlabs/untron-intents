create table if not exists solver.tron_proofs (
    txid bytea primary key,
    blocks bytea[] not null,
    encoded_tx bytea not null,
    proof bytea[] not null,
    index_dec text not null,
    created_at timestamptz not null default now(),

    constraint tron_proofs_txid_len check (octet_length(txid) = 32),
    constraint tron_proofs_blocks_len check (array_length(blocks, 1) = 20)
);

