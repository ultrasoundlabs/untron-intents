/*
Pool projection (UntronIntentsIndex).

Model:
- Versioned tables for "latest state":
  (entity_id, valid_from_seq) PK, with `valid_to_seq is null` = current row.
- Ledger tables for naturally append-only actions.
- Rollback is suffix-only (reorgs), driven by `canonical=false` flips in `chain.event_appended`.

Integrity:
- Apply canonical events strictly in `event_seq` order
- Require `ev.prev_tip == cursor.tip` for every applied event
*/

-- =========================
-- POOL: VERSIONED TABLES (STATE)
-- =========================

-- OwnershipTransferred (singleton)
create table if not exists pool.ownership_versions (
    valid_from_seq bigint primary key,
    valid_to_seq bigint null,
    old_owner evm_address not null,
    new_owner evm_address not null
);
create unique index if not exists pool_ownership_current_unique
on pool.ownership_versions ((1)) where valid_to_seq is null ;

-- RecommendedIntentFeeSet (singleton)
create table if not exists pool.recommended_fee_versions (
valid_from_seq bigint primary key,
valid_to_seq bigint null,
fee_ppm u256 not null,
fee_flat u256 not null
) ;
create unique index if not exists pool_recommended_fee_current_unique
on pool.recommended_fee_versions ((1)) where valid_to_seq is null ;

-- ReceiverIntentParams (KV by id)
create table if not exists pool.receiver_intent_params_versions (
id bytes32_hex not null,
valid_from_seq bigint not null,
valid_to_seq bigint null,

forwarder evm_address not null,
to_tron_evm evm_address not null,
to_tron tron_address not null,
forward_salt bytes32_hex not null,
token evm_address not null,
amount_param u256 not null,

-- Derived: keccak256(abi.encode(forwarder, to_tron_evm)).
intent_hash bytes32_hex not null,

primary key (id, valid_from_seq)
) ;
create unique index if not exists pool_receiver_intent_params_current_unique
on pool.receiver_intent_params_versions (id) where valid_to_seq is null ;

-- ReceiverIntentFeeSnap (KV by id)
create table if not exists pool.receiver_intent_fee_snap_versions (
id bytes32_hex not null,
valid_from_seq bigint not null,
valid_to_seq bigint null,

fee_ppm u256 not null,
fee_flat u256 not null,
tron_payment_amount u256 not null,

primary key (id, valid_from_seq)
) ;
create unique index if not exists pool_receiver_intent_fee_snap_current_unique
on pool.receiver_intent_fee_snap_versions (id) where valid_to_seq is null ;

-- Intent state (KV by id)
create table if not exists pool.intent_versions (
id bytes32_hex not null,
valid_from_seq bigint not null,
valid_to_seq bigint null,

creator evm_address not null,
intent_type smallint not null,
escrow_token evm_address not null,
escrow_amount u256 not null,
refund_beneficiary evm_address not null,
deadline bigint not null,
intent_specs bytes_hex not null,

solver evm_address null,
solver_claimed_at bigint null,

tron_tx_id bytes32_hex null,
tron_block_number bigint null,

solved boolean not null,
funded boolean not null,
settled boolean not null,
closed boolean not null,

primary key (id, valid_from_seq)
) ;
create unique index if not exists pool_intent_current_unique
on pool.intent_versions (id) where valid_to_seq is null ;

-- Helpful indexes for common query patterns.
create index if not exists pool_intent_current_open_by_deadline
on pool.intent_versions (deadline asc)
where valid_to_seq is null and closed = false ;

-- =========================
-- VERSION RANGE / VALUE CHECKS
-- =========================
alter table pool.ownership_versions
add constraint pool_ownership_versions_valid_range_check
check (valid_to_seq is null or valid_to_seq > valid_from_seq) ;

alter table pool.recommended_fee_versions
add constraint pool_recommended_fee_versions_valid_range_check
check (valid_to_seq is null or valid_to_seq > valid_from_seq) ;

alter table pool.receiver_intent_params_versions
add constraint pool_receiver_intent_params_versions_valid_range_check
check (valid_to_seq is null or valid_to_seq > valid_from_seq) ;

alter table pool.receiver_intent_fee_snap_versions
add constraint pool_receiver_intent_fee_snap_versions_valid_range_check
check (valid_to_seq is null or valid_to_seq > valid_from_seq) ;

alter table pool.intent_versions
add constraint pool_intent_versions_valid_range_check
check (valid_to_seq is null or valid_to_seq > valid_from_seq) ;

alter table pool.intent_versions
add constraint pool_intent_type_check
check (intent_type in (0, 1, 2, 3)) ;

-- =========================
-- POOL: LEDGERS (append-only actions)
-- =========================

create table if not exists pool.intent_claimed_ledger (
event_seq bigint primary key,
block_timestamp bigint not null,
id bytes32_hex not null,
solver evm_address not null,
deposit_amount u256 not null
) ;

create table if not exists pool.intent_unclaimed_ledger (
event_seq bigint primary key,
id bytes32_hex not null,
caller evm_address not null,
prev_solver evm_address not null,
funded boolean not null,
deposit_to_caller u256 not null,
deposit_to_refund_beneficiary u256 not null,
deposit_to_prev_solver u256 not null
) ;

create table if not exists pool.intent_solved_ledger (
event_seq bigint primary key,
block_timestamp bigint not null,
id bytes32_hex not null,
solver evm_address not null,
tron_tx_id bytes32_hex not null,
tron_block_number bigint not null
) ;

create table if not exists pool.intent_funded_ledger (
event_seq bigint primary key,
block_timestamp bigint not null,
id bytes32_hex not null,
funder evm_address not null,
token evm_address not null,
amount u256 not null
) ;

create table if not exists pool.intent_settled_ledger (
event_seq bigint primary key,
id bytes32_hex not null,
solver evm_address not null,
escrow_token evm_address not null,
escrow_amount u256 not null,
deposit_token evm_address not null,
deposit_amount u256 not null
) ;

create table if not exists pool.intent_closed_ledger (
event_seq bigint primary key,
id bytes32_hex not null,
caller evm_address not null,
solved boolean not null,
funded boolean not null,
settled boolean not null,
refund_beneficiary evm_address not null,
escrow_token evm_address not null,
escrow_refunded u256 not null,
deposit_token evm_address not null,
deposit_to_caller u256 not null,
deposit_to_refund_beneficiary u256 not null,
deposit_to_solver u256 not null
) ;

-- =========================
-- POOL PATCH HELPERS (versioned updates)
-- =========================

create or replace function pool.ownership_set (
p_seq bigint,
p_old_owner evm_address,
p_new_owner evm_address
) returns void language plpgsql as $$
begin
  update pool.ownership_versions
     set valid_to_seq = p_seq
   where valid_to_seq is null;

  insert into pool.ownership_versions(valid_from_seq, valid_to_seq, old_owner, new_owner)
  values (p_seq, null, p_old_owner, p_new_owner);
end $$ ;

create or replace function pool.recommended_fee_set (
p_seq bigint,
p_fee_ppm u256,
p_fee_flat u256
) returns void language plpgsql as $$
begin
  update pool.recommended_fee_versions
     set valid_to_seq = p_seq
   where valid_to_seq is null;

  insert into pool.recommended_fee_versions(valid_from_seq, valid_to_seq, fee_ppm, fee_flat)
  values (p_seq, null, p_fee_ppm, p_fee_flat);
end $$ ;

create or replace function pool.receiver_intent_params_set (
p_seq bigint,
p_id bytes32_hex,
p_forwarder evm_address,
p_to_tron_evm evm_address,
p_forward_salt bytes32_hex,
p_token evm_address,
p_amount_param u256
) returns void language plpgsql as $$
declare
  v_to_tron tron_address;
  v_intent_hash bytes32_hex;
begin
  v_to_tron := chain.tron_address_from_evm(p_to_tron_evm);
  v_intent_hash := chain.intent_hash_from_receiver_params(p_forwarder, p_to_tron_evm);

  update pool.receiver_intent_params_versions
     set valid_to_seq = p_seq
   where id = p_id and valid_to_seq is null;

  insert into pool.receiver_intent_params_versions(
    id, valid_from_seq, valid_to_seq,
    forwarder, to_tron_evm, to_tron, forward_salt, token, amount_param,
    intent_hash
  ) values (
    p_id, p_seq, null,
    p_forwarder, p_to_tron_evm, v_to_tron, p_forward_salt, p_token, p_amount_param,
    v_intent_hash
  );
end $$ ;

create or replace function pool.receiver_intent_fee_snap_set (
p_seq bigint,
p_id bytes32_hex,
p_fee_ppm u256,
p_fee_flat u256,
p_tron_payment_amount u256
) returns void language plpgsql as $$
begin
  update pool.receiver_intent_fee_snap_versions
     set valid_to_seq = p_seq
   where id = p_id and valid_to_seq is null;

  insert into pool.receiver_intent_fee_snap_versions(
    id, valid_from_seq, valid_to_seq,
    fee_ppm, fee_flat, tron_payment_amount
  ) values (
    p_id, p_seq, null,
    p_fee_ppm, p_fee_flat, p_tron_payment_amount
  );
end $$ ;

create or replace function pool.intent_create (
p_seq bigint,
p_id bytes32_hex,
p_creator evm_address,
p_intent_type smallint,
p_escrow_token evm_address,
p_escrow_amount u256,
p_refund_beneficiary evm_address,
p_deadline bigint,
p_intent_specs bytes_hex
) returns void language plpgsql as $$
begin
  update pool.intent_versions
     set valid_to_seq = p_seq
   where id = p_id and valid_to_seq is null;

  insert into pool.intent_versions(
    id, valid_from_seq, valid_to_seq,
    creator, intent_type, escrow_token, escrow_amount, refund_beneficiary, deadline, intent_specs,
    solver, solver_claimed_at, tron_tx_id, tron_block_number,
    solved, funded, settled, closed
  ) values (
    p_id, p_seq, null,
    p_creator, p_intent_type, p_escrow_token, p_escrow_amount, p_refund_beneficiary, p_deadline, p_intent_specs,
    null, null, null, null,
    false, false, false, false
  );
end $$ ;

create or replace function pool.intent_set_claimed (
p_seq bigint,
p_id bytes32_hex,
p_solver evm_address,
p_solver_claimed_at bigint
) returns void language plpgsql as $$
declare
  cur pool.intent_versions%rowtype;
begin
  select * into cur
    from pool.intent_versions
   where id = p_id and valid_to_seq is null
   limit 1;

  if not found then
    raise exception 'IntentClaimed without existing intent: id %', p_id;
  end if;

  update pool.intent_versions
     set valid_to_seq = p_seq
   where id = p_id and valid_to_seq is null;

  insert into pool.intent_versions(
    id, valid_from_seq, valid_to_seq,
    creator, intent_type, escrow_token, escrow_amount, refund_beneficiary, deadline, intent_specs,
    solver, solver_claimed_at, tron_tx_id, tron_block_number,
    solved, funded, settled, closed
  ) values (
    cur.id, p_seq, null,
    cur.creator, cur.intent_type, cur.escrow_token, cur.escrow_amount, cur.refund_beneficiary, cur.deadline, cur.intent_specs,
    p_solver, p_solver_claimed_at, cur.tron_tx_id, cur.tron_block_number,
    cur.solved, cur.funded, cur.settled, cur.closed
  );
end $$ ;

create or replace function pool.intent_set_unclaimed (
p_seq bigint,
p_id bytes32_hex
) returns void language plpgsql as $$
declare
  cur pool.intent_versions%rowtype;
begin
  select * into cur
    from pool.intent_versions
   where id = p_id and valid_to_seq is null
   limit 1;

  if not found then
    raise exception 'IntentUnclaimed without existing intent: id %', p_id;
  end if;

  update pool.intent_versions
     set valid_to_seq = p_seq
   where id = p_id and valid_to_seq is null;

  insert into pool.intent_versions(
    id, valid_from_seq, valid_to_seq,
    creator, intent_type, escrow_token, escrow_amount, refund_beneficiary, deadline, intent_specs,
    solver, solver_claimed_at, tron_tx_id, tron_block_number,
    solved, funded, settled, closed
  ) values (
    cur.id, p_seq, null,
    cur.creator, cur.intent_type, cur.escrow_token, cur.escrow_amount, cur.refund_beneficiary, cur.deadline, cur.intent_specs,
    null, null, cur.tron_tx_id, cur.tron_block_number,
    cur.solved, cur.funded, cur.settled, cur.closed
  );
end $$ ;

create or replace function pool.intent_set_solved (
p_seq bigint,
p_id bytes32_hex,
p_solver evm_address,
p_solver_claimed_at bigint,
p_tron_tx_id bytes32_hex,
p_tron_block_number bigint
) returns void language plpgsql as $$
declare
  cur pool.intent_versions%rowtype;
begin
  select * into cur
    from pool.intent_versions
   where id = p_id and valid_to_seq is null
   limit 1;

  if not found then
    raise exception 'IntentSolved without existing intent: id %', p_id;
  end if;

  update pool.intent_versions
     set valid_to_seq = p_seq
   where id = p_id and valid_to_seq is null;

  insert into pool.intent_versions(
    id, valid_from_seq, valid_to_seq,
    creator, intent_type, escrow_token, escrow_amount, refund_beneficiary, deadline, intent_specs,
    solver, solver_claimed_at, tron_tx_id, tron_block_number,
    solved, funded, settled, closed
  ) values (
    cur.id, p_seq, null,
    cur.creator, cur.intent_type, cur.escrow_token, cur.escrow_amount, cur.refund_beneficiary, cur.deadline, cur.intent_specs,
    p_solver, p_solver_claimed_at, p_tron_tx_id, p_tron_block_number,
    true, cur.funded, cur.settled, cur.closed
  );
end $$ ;

create or replace function pool.intent_set_funded (
p_seq bigint,
p_id bytes32_hex
) returns void language plpgsql as $$
declare
  cur pool.intent_versions%rowtype;
begin
  select * into cur
    from pool.intent_versions
   where id = p_id and valid_to_seq is null
   limit 1;

  if not found then
    raise exception 'IntentFunded without existing intent: id %', p_id;
  end if;

  if cur.funded then
    -- Idempotent: avoid version churn on duplicate ingests.
    return;
  end if;

  update pool.intent_versions
     set valid_to_seq = p_seq
   where id = p_id and valid_to_seq is null;

  insert into pool.intent_versions(
    id, valid_from_seq, valid_to_seq,
    creator, intent_type, escrow_token, escrow_amount, refund_beneficiary, deadline, intent_specs,
    solver, solver_claimed_at, tron_tx_id, tron_block_number,
    solved, funded, settled, closed
  ) values (
    cur.id, p_seq, null,
    cur.creator, cur.intent_type, cur.escrow_token, cur.escrow_amount, cur.refund_beneficiary, cur.deadline, cur.intent_specs,
    cur.solver, cur.solver_claimed_at, cur.tron_tx_id, cur.tron_block_number,
    cur.solved, true, cur.settled, cur.closed
  );
end $$ ;

create or replace function pool.intent_set_settled (
p_seq bigint,
p_id bytes32_hex
) returns void language plpgsql as $$
declare
  cur pool.intent_versions%rowtype;
begin
  select * into cur
    from pool.intent_versions
   where id = p_id and valid_to_seq is null
   limit 1;

  if not found then
    raise exception 'IntentSettled without existing intent: id %', p_id;
  end if;

  if cur.settled then
    return;
  end if;

  update pool.intent_versions
     set valid_to_seq = p_seq
   where id = p_id and valid_to_seq is null;

  insert into pool.intent_versions(
    id, valid_from_seq, valid_to_seq,
    creator, intent_type, escrow_token, escrow_amount, refund_beneficiary, deadline, intent_specs,
    solver, solver_claimed_at, tron_tx_id, tron_block_number,
    solved, funded, settled, closed
  ) values (
    cur.id, p_seq, null,
    cur.creator, cur.intent_type, cur.escrow_token, cur.escrow_amount, cur.refund_beneficiary, cur.deadline, cur.intent_specs,
    cur.solver, cur.solver_claimed_at, cur.tron_tx_id, cur.tron_block_number,
    cur.solved, cur.funded, true, cur.closed
  );
end $$ ;

create or replace function pool.intent_set_closed (
p_seq bigint,
p_id bytes32_hex,
p_solved boolean,
p_funded boolean,
p_settled boolean
) returns void language plpgsql as $$
declare
  cur pool.intent_versions%rowtype;
begin
  select * into cur
    from pool.intent_versions
   where id = p_id and valid_to_seq is null
   limit 1;

  if not found then
    raise exception 'IntentClosed without existing intent: id %', p_id;
  end if;

  update pool.intent_versions
     set valid_to_seq = p_seq
   where id = p_id and valid_to_seq is null;

  insert into pool.intent_versions(
    id, valid_from_seq, valid_to_seq,
    creator, intent_type, escrow_token, escrow_amount, refund_beneficiary, deadline, intent_specs,
    solver, solver_claimed_at, tron_tx_id, tron_block_number,
    solved, funded, settled, closed
  ) values (
    cur.id, p_seq, null,
    cur.creator, cur.intent_type, cur.escrow_token, cur.escrow_amount, cur.refund_beneficiary, cur.deadline, cur.intent_specs,
    null, null, cur.tron_tx_id, cur.tron_block_number,
    p_solved, p_funded, p_settled, true
  );
end $$ ;

-- =========================
-- POOL APPLY ONE (event interpreter)
-- =========================
create or replace function pool.apply_one (
p_seq bigint,
p_block_timestamp bigint,
p_type text,
p_args jsonb
)
returns void language plpgsql as $$
begin
  if p_type = 'OwnershipTransferred' then
    perform chain.require_json_keys(p_args, array['old_owner','new_owner']);
    perform pool.ownership_set(
      p_seq,
      (p_args->>'old_owner')::evm_address,
      (p_args->>'new_owner')::evm_address
    );

  elsif p_type = 'RecommendedIntentFeeSet' then
    perform chain.require_json_keys(p_args, array['fee_ppm','fee_flat']);
    perform pool.recommended_fee_set(
      p_seq,
      (p_args->>'fee_ppm')::u256,
      (p_args->>'fee_flat')::u256
    );

  elsif p_type = 'ReceiverIntentParams' then
    perform chain.require_json_keys(p_args, array['id','forwarder','to_tron','forward_salt','token','amount']);

    perform pool.receiver_intent_params_set(
      p_seq,
      (p_args->>'id')::bytes32_hex,
      (p_args->>'forwarder')::evm_address,
      (p_args->>'to_tron')::evm_address,
      (p_args->>'forward_salt')::bytes32_hex,
      (p_args->>'token')::evm_address,
      (p_args->>'amount')::u256
    );

  elsif p_type = 'ReceiverIntentFeeSnap' then
    perform chain.require_json_keys(p_args, array['id','fee_ppm','fee_flat','tron_payment_amount']);
    perform pool.receiver_intent_fee_snap_set(
      p_seq,
      (p_args->>'id')::bytes32_hex,
      (p_args->>'fee_ppm')::u256,
      (p_args->>'fee_flat')::u256,
      (p_args->>'tron_payment_amount')::u256
    );

  elsif p_type = 'IntentCreated' then
    perform chain.require_json_keys(p_args, array[
      'id','creator','intent_type','token','amount','refund_beneficiary','deadline','intent_specs'
    ]);
    perform pool.intent_create(
      p_seq,
      (p_args->>'id')::bytes32_hex,
      (p_args->>'creator')::evm_address,
      (p_args->>'intent_type')::smallint,
      (p_args->>'token')::evm_address,
      (p_args->>'amount')::u256,
      (p_args->>'refund_beneficiary')::evm_address,
      (p_args->>'deadline')::bigint,
      (p_args->>'intent_specs')::bytes_hex
    );

  elsif p_type = 'IntentClaimed' then
    perform chain.require_json_keys(p_args, array['id','solver','deposit_amount']);
    insert into pool.intent_claimed_ledger(event_seq, block_timestamp, id, solver, deposit_amount)
    values (
      p_seq,
      p_block_timestamp,
      (p_args->>'id')::bytes32_hex,
      (p_args->>'solver')::evm_address,
      (p_args->>'deposit_amount')::u256
    );
    perform pool.intent_set_claimed(
      p_seq,
      (p_args->>'id')::bytes32_hex,
      (p_args->>'solver')::evm_address,
      p_block_timestamp
    );

  elsif p_type = 'IntentUnclaimed' then
    perform chain.require_json_keys(p_args, array[
      'id','caller','prev_solver','funded','deposit_to_caller','deposit_to_refund_beneficiary','deposit_to_prev_solver'
    ]);
    insert into pool.intent_unclaimed_ledger(
      event_seq, id, caller, prev_solver, funded,
      deposit_to_caller, deposit_to_refund_beneficiary, deposit_to_prev_solver
    ) values (
      p_seq,
      (p_args->>'id')::bytes32_hex,
      (p_args->>'caller')::evm_address,
      (p_args->>'prev_solver')::evm_address,
      (p_args->>'funded')::boolean,
      (p_args->>'deposit_to_caller')::u256,
      (p_args->>'deposit_to_refund_beneficiary')::u256,
      (p_args->>'deposit_to_prev_solver')::u256
    );
    perform pool.intent_set_unclaimed(p_seq, (p_args->>'id')::bytes32_hex);

  elsif p_type = 'IntentSolved' then
    perform chain.require_json_keys(p_args, array['id','solver','tron_tx_id','tron_block_number']);
    insert into pool.intent_solved_ledger(event_seq, block_timestamp, id, solver, tron_tx_id, tron_block_number)
    values (
      p_seq,
      p_block_timestamp,
      (p_args->>'id')::bytes32_hex,
      (p_args->>'solver')::evm_address,
      (p_args->>'tron_tx_id')::bytes32_hex,
      (p_args->>'tron_block_number')::bigint
    );
    perform pool.intent_set_solved(
      p_seq,
      (p_args->>'id')::bytes32_hex,
      (p_args->>'solver')::evm_address,
      p_block_timestamp,
      (p_args->>'tron_tx_id')::bytes32_hex,
      (p_args->>'tron_block_number')::bigint
    );

  elsif p_type = 'IntentFunded' then
    perform chain.require_json_keys(p_args, array['id','funder','token','amount']);
    insert into pool.intent_funded_ledger(event_seq, block_timestamp, id, funder, token, amount)
    values (
      p_seq,
      p_block_timestamp,
      (p_args->>'id')::bytes32_hex,
      (p_args->>'funder')::evm_address,
      (p_args->>'token')::evm_address,
      (p_args->>'amount')::u256
    );
    perform pool.intent_set_funded(p_seq, (p_args->>'id')::bytes32_hex);

  elsif p_type = 'IntentSettled' then
    perform chain.require_json_keys(p_args, array[
      'id','solver','escrow_token','escrow_amount','deposit_token','deposit_amount'
    ]);
    insert into pool.intent_settled_ledger(
      event_seq, id, solver, escrow_token, escrow_amount, deposit_token, deposit_amount
    ) values (
      p_seq,
      (p_args->>'id')::bytes32_hex,
      (p_args->>'solver')::evm_address,
      (p_args->>'escrow_token')::evm_address,
      (p_args->>'escrow_amount')::u256,
      (p_args->>'deposit_token')::evm_address,
      (p_args->>'deposit_amount')::u256
    );
    perform pool.intent_set_settled(p_seq, (p_args->>'id')::bytes32_hex);

  elsif p_type = 'IntentClosed' then
    perform chain.require_json_keys(p_args, array[
      'id','caller','solved','funded','settled',
      'refund_beneficiary','escrow_token','escrow_refunded',
      'deposit_token','deposit_to_caller','deposit_to_refund_beneficiary','deposit_to_solver'
    ]);
    insert into pool.intent_closed_ledger(
      event_seq, id, caller, solved, funded, settled,
      refund_beneficiary, escrow_token, escrow_refunded,
      deposit_token, deposit_to_caller, deposit_to_refund_beneficiary, deposit_to_solver
    ) values (
      p_seq,
      (p_args->>'id')::bytes32_hex,
      (p_args->>'caller')::evm_address,
      (p_args->>'solved')::boolean,
      (p_args->>'funded')::boolean,
      (p_args->>'settled')::boolean,
      (p_args->>'refund_beneficiary')::evm_address,
      (p_args->>'escrow_token')::evm_address,
      (p_args->>'escrow_refunded')::u256,
      (p_args->>'deposit_token')::evm_address,
      (p_args->>'deposit_to_caller')::u256,
      (p_args->>'deposit_to_refund_beneficiary')::u256,
      (p_args->>'deposit_to_solver')::u256
    );
    perform pool.intent_set_closed(
      p_seq,
      (p_args->>'id')::bytes32_hex,
      (p_args->>'solved')::boolean,
      (p_args->>'funded')::boolean,
      (p_args->>'settled')::boolean
    );

  else
    -- Forward-compatibility: ignore unknown event types.
    null;
  end if;
end $$ ;

-- =========================
-- POOL APPLY CATCHUP (contiguous canonical apply)
-- =========================
create or replace function pool.apply_catchup (
p_chain_id bigint,
p_contract_address evm_address
)
returns void language plpgsql as $$
declare
  cur_seq bigint;
  cur_tip bytes32_hex;
  next_seq bigint;
  ev record;
begin
  -- One projector per instance per transaction.
  perform pg_advisory_xact_lock(9201, hashtext(p_chain_id::text || ':' || p_contract_address::text));

  select applied_through_seq, tip
    into cur_seq, cur_tip
    from chain.stream_cursor
   where stream = 'pool'
     and chain_id = p_chain_id
     and contract_address = p_contract_address
   for update;

  if not found then
    raise exception 'stream cursor not initialized for pool instance (chain_id=%, contract=%) (call chain.configure_instance(''pool'', ...))',
      p_chain_id, p_contract_address;
  end if;

  loop
    next_seq := cur_seq + 1;

    select *
      into ev
      from chain.event_appended
     where stream='pool'
       and chain_id = p_chain_id
       and contract_address = p_contract_address
       and canonical
       and event_seq = next_seq
     limit 1;

    exit when not found;

    -- hash-chain link integrity
    if ev.prev_tip <> cur_tip then
      raise exception 'pool tip mismatch at seq %, expected %, got %', next_seq, cur_tip, ev.prev_tip;
    end if;

    perform pool.apply_one(ev.event_seq, ev.block_timestamp, ev.event_type, ev.args);

    cur_seq := next_seq;
    cur_tip := ev.new_tip;
  end loop;

  update chain.stream_cursor
     set applied_through_seq = cur_seq,
         tip = cur_tip,
         updated_at = now()
   where stream = 'pool'
     and chain_id = p_chain_id
     and contract_address = p_contract_address;
end $$ ;

-- =========================
-- POOL ROLLBACK (suffix-only)
-- =========================
create or replace function pool.rollback_from (
p_chain_id bigint,
p_contract_address evm_address,
rollback_seq bigint
)
returns void language plpgsql as $$
begin
  -- ledgers: delete suffix
  delete from pool.intent_closed_ledger where event_seq >= rollback_seq;
  delete from pool.intent_settled_ledger where event_seq >= rollback_seq;
  delete from pool.intent_funded_ledger where event_seq >= rollback_seq;
  delete from pool.intent_solved_ledger where event_seq >= rollback_seq;
  delete from pool.intent_unclaimed_ledger where event_seq >= rollback_seq;
  delete from pool.intent_claimed_ledger where event_seq >= rollback_seq;

  -- versioned: delete suffix + reopen rows closed by suffix
  delete from pool.intent_versions where valid_from_seq >= rollback_seq;
  update pool.intent_versions set valid_to_seq = null where valid_to_seq >= rollback_seq;

  delete from pool.receiver_intent_fee_snap_versions where valid_from_seq >= rollback_seq;
  update pool.receiver_intent_fee_snap_versions set valid_to_seq = null where valid_to_seq >= rollback_seq;

  delete from pool.receiver_intent_params_versions where valid_from_seq >= rollback_seq;
  update pool.receiver_intent_params_versions set valid_to_seq = null where valid_to_seq >= rollback_seq;

  delete from pool.recommended_fee_versions where valid_from_seq >= rollback_seq;
  update pool.recommended_fee_versions set valid_to_seq = null where valid_to_seq >= rollback_seq;

  delete from pool.ownership_versions where valid_from_seq >= rollback_seq;
  update pool.ownership_versions set valid_to_seq = null where valid_to_seq >= rollback_seq;

  -- cursor rewind
  update chain.stream_cursor
     set applied_through_seq = rollback_seq - 1,
         updated_at = now()
   where stream = 'pool'
     and chain_id = p_chain_id
     and contract_address = p_contract_address;

  -- recompute cursor tip (genesis if seq=0 else new_tip at applied seq)
  update chain.stream_cursor c
     set tip =
       case when c.applied_through_seq = 0
            then (select genesis_tip from chain.instance
                   where stream='pool' and chain_id=p_chain_id and contract_address=p_contract_address
                   limit 1)
            else (select e.new_tip from chain.event_appended e
                   where e.stream='pool'
                     and e.chain_id=p_chain_id
                     and e.contract_address=p_contract_address
                     and e.canonical
                     and e.event_seq = c.applied_through_seq
                   limit 1)
       end
   where c.stream='pool'
     and c.chain_id=p_chain_id
     and c.contract_address=p_contract_address;
end $$ ;
