/*
Public read API (PostgREST schema).

We expose only views in api schema, sourced from internal tables.
This lets us refactor internal storage without breaking API consumers.
*/

-- =========================
-- BASIC
-- =========================
create or replace view api.health as
select 'ok'::text as status;

create or replace view api.instances as
select stream, chain_id, contract_address, genesis_tip, inserted_at
from chain.instance;

create or replace view api.stream_cursor as
select stream, chain_id, contract_address, applied_through_seq, tip, updated_at
from chain.stream_cursor;

create or replace view api.pool_instance as
select chain_id, contract_address
from chain.instance
where stream = 'pool'
limit 1;

create or replace view api.forwarder_instances as
select chain_id, contract_address
from chain.instance
where stream = 'forwarder';

-- =========================
-- RAW EVENTS (canonical)
-- =========================
create or replace view api.event_appended as
select
    stream,
    chain_id,
    contract_address,
    event_seq,
    prev_tip,
    new_tip,
    event_signature,
    abi_encoded_event_data,
    event_type,
    args,
    block_number,
    block_timestamp,
    to_timestamp(block_timestamp) as block_time,
    block_hash,
    tx_hash,
    log_index
from chain.event_appended
where canonical;

-- =========================
-- POOL: CURRENT STATE
-- =========================

create or replace view api.pool_ownership as
select valid_from_seq, old_owner, new_owner
from pool.ownership_versions
where valid_to_seq is null;

create or replace view api.pool_recommended_fee as
select valid_from_seq, fee_ppm, fee_flat
from pool.recommended_fee_versions
where valid_to_seq is null;

create or replace view api.pool_intents as
select
    id,
    valid_from_seq,
    creator,
    intent_type,
    escrow_token,
    escrow_amount,
    refund_beneficiary,
    deadline,
    to_timestamp(deadline) as deadline_time,
    intent_specs,
    solver,
    solver_claimed_at,
    case
        when solver_claimed_at is null then null
        else to_timestamp(solver_claimed_at)
    end as solver_claimed_time,
    tron_tx_id,
    tron_block_number,
    solved,
    funded,
    settled,
    closed
from pool.intent_versions
where valid_to_seq is null;

create or replace view api.pool_receiver_intents as
select
    i.id,
    i.creator,
    i.intent_type,
    i.escrow_token,
    i.escrow_amount,
    i.refund_beneficiary,
    i.deadline,
    to_timestamp(i.deadline) as deadline_time,
    i.solver,
    i.solver_claimed_at,
    i.solved,
    i.funded,
    i.settled,
    i.closed,

    p.forwarder,
    p.to_tron_evm,
    p.to_tron,
    p.forward_salt,
    p.token as receiver_token,
    p.amount_param,
    p.intent_hash,

    f.fee_ppm,
    f.fee_flat,
    f.tron_payment_amount
from pool.intent_versions i
join pool.receiver_intent_params_versions p
    on p.id = i.id and p.valid_to_seq is null
left join pool.receiver_intent_fee_snap_versions f
    on f.id = i.id and f.valid_to_seq is null
where i.valid_to_seq is null;

-- =========================
-- POOL: RELAYER/SOLVER QUERIES
-- =========================

create or replace view api.pool_open_intents as
select *
from api.pool_intents
where
    closed = false
    and deadline > extract(epoch from now())::bigint;

-- Unclaimable = claimed, unsolved, and `now >= solver_claimed_at + TIME_TO_FILL`.
-- TIME_TO_FILL is a protocol constant: 2 minutes (120 seconds).
create or replace view api.pool_unclaimable_intents as
select *
from api.pool_intents
where
    closed = false
    and solved = false
    and solver is not null
    and solver_claimed_at is not null
    and extract(epoch from now())::bigint >= solver_claimed_at + 120;

-- Settleable = solved + funded but not yet settled.
create or replace view api.pool_settleable_intents as
select *
from api.pool_intents
where
    closed = false
    and solved = true
    and funded = true
    and settled = false;

-- Closable = deadline passed and `closeIntent` would not immediately settle.
create or replace view api.pool_closable_intents as
select *
from api.pool_intents
where
    closed = false
    and deadline <= extract(epoch from now())::bigint
    and not (solved = true and funded = true and settled = false);

-- Virtual (waiting funding): solved but not funded yet.
create or replace view api.pool_virtual_waiting_funding as
select *
from api.pool_intents
where
    closed = false
    and solved = true
    and funded = false;

-- =========================
-- FORWARDER: CURRENT STATE
-- =========================

create or replace view api.forwarder_ownership as
select chain_id, contract_address, valid_from_seq, old_owner, new_owner
from forwarder.ownership_versions
where valid_to_seq is null;

create or replace view api.forwarder_bridgers as
select chain_id, contract_address, valid_from_seq, usdt_bridger, usdc_bridger
from forwarder.bridgers_versions
where valid_to_seq is null;

create or replace view api.forwarder_quoters as
select chain_id, contract_address, token_in, valid_from_seq, quoter
from forwarder.quoter_versions
where valid_to_seq is null;

create or replace view api.forwarder_receivers as
select
    chain_id,
    contract_address,
    receiver_salt,
    valid_from_seq,
    receiver,
    implementation
from forwarder.receiver_versions
where valid_to_seq is null;

create or replace view api.forwarder_forwards as
select
    chain_id,
    contract_address,
    forward_id,
    valid_from_seq,
    base_receiver_salt,
    forward_salt,
    intent_hash,
    target_chain,
    beneficiary,
    beneficiary_claim_only,
    balance_param,
    token_in,
    token_out,
    receiver_used,
    ephemeral_receiver,
    started_at,
    to_timestamp(started_at) as started_time,
    completed_at,
    case
        when completed_at is null then null else to_timestamp(completed_at)
    end as completed_time,
    ephemeral,
    amount_pulled,
    amount_forwarded,
    relayer_rebate,
    msg_value_refunded,
    settled_locally,
    bridger,
    expected_bridge_out,
    bridge_data_hash
from forwarder.forward_versions
where valid_to_seq is null;

create or replace view api.forwarder_swap_executed as
select *
from forwarder.swap_executed_ledger;

create or replace view api.forwarder_bridge_initiated as
select *
from forwarder.bridge_initiated_ledger;

-- Cross-chain forwards that target the pool chain (best-effort signal that a pool-side
-- funding action may occur later).
create or replace view api.forwarder_forwards_to_pool as
with pool as (
    select chain_id as pool_chain_id
    from api.pool_instance
    limit 1
)

select
    f.*,
    (f.target_chain = p.pool_chain_id) as targets_pool_chain
from api.forwarder_forwards f
cross join pool p
where f.target_chain = p.pool_chain_id;

-- Best-effort "pending virtual receiver intent" signals:
-- - completed cross-chain forward to pool chain
-- - matches an existing receiver intent params row by (intent_hash, forward_salt, token_out, balance_param)
-- This assumes:
-- - the worker populates `pool.receiver_intent_params_versions.intent_hash`, and
-- - forwarders are deployed at the same address across chains (so `contract_address` matches).
create or replace view api.forwarder_expected_receiver_intents as
with pool as (
    select chain_id as pool_chain_id
    from api.pool_instance
    limit 1
),

receiver_params as (
    select
        id,
        forwarder,
        forward_salt,
        token,
        amount_param,
        intent_hash
    from pool.receiver_intent_params_versions
    where valid_to_seq is null
)

select
    f.chain_id as origin_chain_id,
    f.contract_address as origin_forwarder,
    f.forward_id,
    f.intent_hash,
    f.forward_salt,
    f.token_out,
    f.balance_param,
    f.amount_forwarded,
    f.expected_bridge_out,
    f.bridge_data_hash,
    rp.id as pool_intent_id
from api.forwarder_forwards f
cross join pool p
left join receiver_params rp
    on
        rp.forwarder = f.contract_address
        and rp.intent_hash = f.intent_hash
        and rp.forward_salt = f.forward_salt
        and rp.token = f.token_out
        and rp.amount_param = f.balance_param
where
    f.target_chain = p.pool_chain_id
    and f.settled_locally is false
    and f.completed_at is not null;

-- =========================
-- POSTGREST GRANTS
-- Safe to run in all environments; no-ops if roles don't exist.
-- =========================
do $$
begin
  if exists (select 1 from pg_roles where rolname = 'pgrst_anon') then
    grant usage on schema api to pgrst_anon;
    grant select on all tables in schema api to pgrst_anon;

    revoke all on schema chain from pgrst_anon;
    revoke all on schema pool from pgrst_anon;
    revoke all on schema forwarder from pgrst_anon;

    alter default privileges in schema api grant select on tables to pgrst_anon;
  end if;
end $$ ;
