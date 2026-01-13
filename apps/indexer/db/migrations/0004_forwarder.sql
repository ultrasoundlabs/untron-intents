/*
Forwarder projection (IntentsForwarderIndex).

Same model as pool:
- versioned tables for current singleton/KV state
- ledgers for actions
- apply_catchup + rollback_from driven by event_seq
- strict prev_tip == cursor.tip

Also introduces ingestion triggers to:
- apply catchup on inserts
- rollback and re-apply on canonical flips (reorgs)
*/

-- =========================
-- FORWARDER: VERSIONED TABLES
-- =========================

-- OwnershipTransferred (singleton per forwarder instance)
create table if not exists forwarder.ownership_versions (
    chain_id bigint not null,
    contract_address evm_address not null,

    valid_from_seq bigint not null,
    valid_to_seq bigint null,

    old_owner evm_address not null,
    new_owner evm_address not null,

    primary key (chain_id, contract_address, valid_from_seq)
);
create unique index if not exists forwarder_ownership_current_unique
on forwarder.ownership_versions (chain_id, contract_address)
where valid_to_seq is null ;

-- BridgersSet (singleton per forwarder instance)
create table if not exists forwarder.bridgers_versions (
chain_id bigint not null,
contract_address evm_address not null,

valid_from_seq bigint not null,
valid_to_seq bigint null,

usdt_bridger evm_address not null,
usdc_bridger evm_address not null,

primary key (chain_id, contract_address, valid_from_seq)
) ;
create unique index if not exists forwarder_bridgers_current_unique
on forwarder.bridgers_versions (chain_id, contract_address)
where valid_to_seq is null ;

-- QuoterSet (KV by token_in per forwarder instance)
create table if not exists forwarder.quoter_versions (
chain_id bigint not null,
contract_address evm_address not null,

token_in evm_address not null,

valid_from_seq bigint not null,
valid_to_seq bigint null,

quoter evm_address not null,

primary key (chain_id, contract_address, token_in, valid_from_seq)
) ;
create unique index if not exists forwarder_quoter_current_unique
on forwarder.quoter_versions (chain_id, contract_address, token_in)
where valid_to_seq is null ;

-- ReceiverDeployed (KV by receiver_salt per forwarder instance)
create table if not exists forwarder.receiver_versions (
chain_id bigint not null,
contract_address evm_address not null,

receiver_salt bytes32_hex not null,

valid_from_seq bigint not null,
valid_to_seq bigint null,

receiver evm_address not null,
implementation evm_address not null,

primary key (chain_id, contract_address, receiver_salt, valid_from_seq)
) ;
create unique index if not exists forwarder_receiver_current_unique
on forwarder.receiver_versions (chain_id, contract_address, receiver_salt)
where valid_to_seq is null ;

-- Forward attempt state (KV by forward_id per forwarder instance)
create table if not exists forwarder.forward_versions (
chain_id bigint not null,
contract_address evm_address not null,

forward_id bytes32_hex not null,

valid_from_seq bigint not null,
valid_to_seq bigint null,

-- ForwardStarted payload
base_receiver_salt bytes32_hex not null,
forward_salt bytes32_hex not null,
intent_hash bytes32_hex not null,
target_chain bigint not null,
beneficiary evm_address not null,
beneficiary_claim_only boolean not null,
balance_param u256 not null,
token_in evm_address not null,
token_out evm_address not null,
receiver_used evm_address not null,
ephemeral_receiver evm_address not null,
started_at bigint not null,

-- ForwardCompleted payload (nullable until completed)
completed_at bigint null,
ephemeral boolean null,
amount_pulled u256 null,
amount_forwarded u256 null,
relayer_rebate u256 null,
msg_value_refunded u256 null,
settled_locally boolean null,
bridger evm_address null,
expected_bridge_out u256 null,
bridge_data_hash bytes32_hex null,

primary key (chain_id, contract_address, forward_id, valid_from_seq)
) ;
create unique index if not exists forwarder_forward_current_unique
on forwarder.forward_versions (chain_id, contract_address, forward_id)
where valid_to_seq is null ;

-- =========================
-- VERSION RANGE CHECKS
-- =========================
alter table forwarder.ownership_versions
add constraint forwarder_ownership_versions_valid_range_check
check (valid_to_seq is null or valid_to_seq > valid_from_seq) ;

alter table forwarder.bridgers_versions
add constraint forwarder_bridgers_versions_valid_range_check
check (valid_to_seq is null or valid_to_seq > valid_from_seq) ;

alter table forwarder.quoter_versions
add constraint forwarder_quoter_versions_valid_range_check
check (valid_to_seq is null or valid_to_seq > valid_from_seq) ;

alter table forwarder.receiver_versions
add constraint forwarder_receiver_versions_valid_range_check
check (valid_to_seq is null or valid_to_seq > valid_from_seq) ;

alter table forwarder.forward_versions
add constraint forwarder_forward_versions_valid_range_check
check (valid_to_seq is null or valid_to_seq > valid_from_seq) ;

-- =========================
-- FORWARDER: LEDGERS
-- =========================

create table if not exists forwarder.swap_executed_ledger (
event_seq bigint primary key,

chain_id bigint not null,
contract_address evm_address not null,

forward_id bytes32_hex not null,
token_in evm_address not null,
token_out evm_address not null,
min_out u256 not null,
actual_out u256 not null
) ;

create table if not exists forwarder.bridge_initiated_ledger (
event_seq bigint primary key,

chain_id bigint not null,
contract_address evm_address not null,

forward_id bytes32_hex not null,
bridger evm_address not null,
token_out evm_address not null,
amount_in u256 not null,
target_chain bigint not null
) ;

-- =========================
-- FORWARDER PATCH HELPERS
-- =========================

create or replace function forwarder.ownership_set (
p_seq bigint,
p_chain_id bigint,
p_contract_address evm_address,
p_old_owner evm_address,
p_new_owner evm_address
) returns void language plpgsql as $$
begin
  update forwarder.ownership_versions
     set valid_to_seq = p_seq
   where chain_id = p_chain_id
     and contract_address = p_contract_address
     and valid_to_seq is null;

  insert into forwarder.ownership_versions(
    chain_id, contract_address,
    valid_from_seq, valid_to_seq,
    old_owner, new_owner
  ) values (
    p_chain_id, p_contract_address,
    p_seq, null,
    p_old_owner, p_new_owner
  );
end $$ ;

create or replace function forwarder.bridgers_set (
p_seq bigint,
p_chain_id bigint,
p_contract_address evm_address,
p_usdt_bridger evm_address,
p_usdc_bridger evm_address
) returns void language plpgsql as $$
begin
  update forwarder.bridgers_versions
     set valid_to_seq = p_seq
   where chain_id = p_chain_id
     and contract_address = p_contract_address
     and valid_to_seq is null;

  insert into forwarder.bridgers_versions(
    chain_id, contract_address,
    valid_from_seq, valid_to_seq,
    usdt_bridger, usdc_bridger
  ) values (
    p_chain_id, p_contract_address,
    p_seq, null,
    p_usdt_bridger, p_usdc_bridger
  );
end $$ ;

create or replace function forwarder.quoter_set (
p_seq bigint,
p_chain_id bigint,
p_contract_address evm_address,
p_token_in evm_address,
p_quoter evm_address
) returns void language plpgsql as $$
begin
  update forwarder.quoter_versions
     set valid_to_seq = p_seq
   where chain_id = p_chain_id
     and contract_address = p_contract_address
     and token_in = p_token_in
     and valid_to_seq is null;

  insert into forwarder.quoter_versions(
    chain_id, contract_address, token_in,
    valid_from_seq, valid_to_seq,
    quoter
  ) values (
    p_chain_id, p_contract_address, p_token_in,
    p_seq, null,
    p_quoter
  );
end $$ ;

create or replace function forwarder.receiver_deployed_set (
p_seq bigint,
p_chain_id bigint,
p_contract_address evm_address,
p_receiver_salt bytes32_hex,
p_receiver evm_address,
p_implementation evm_address
) returns void language plpgsql as $$
begin
  update forwarder.receiver_versions
     set valid_to_seq = p_seq
   where chain_id = p_chain_id
     and contract_address = p_contract_address
     and receiver_salt = p_receiver_salt
     and valid_to_seq is null;

  insert into forwarder.receiver_versions(
    chain_id, contract_address, receiver_salt,
    valid_from_seq, valid_to_seq,
    receiver, implementation
  ) values (
    p_chain_id, p_contract_address, p_receiver_salt,
    p_seq, null,
    p_receiver, p_implementation
  );
end $$ ;

create or replace function forwarder.forward_started_set (
p_seq bigint,
p_block_timestamp bigint,
p_chain_id bigint,
p_contract_address evm_address,
p_forward_id bytes32_hex,
p_base_receiver_salt bytes32_hex,
p_forward_salt bytes32_hex,
p_intent_hash bytes32_hex,
p_target_chain bigint,
p_beneficiary evm_address,
p_beneficiary_claim_only boolean,
p_balance_param u256,
p_token_in evm_address,
p_token_out evm_address,
p_receiver_used evm_address,
p_ephemeral_receiver evm_address
) returns void language plpgsql as $$
begin
  update forwarder.forward_versions
     set valid_to_seq = p_seq
   where chain_id = p_chain_id
     and contract_address = p_contract_address
     and forward_id = p_forward_id
     and valid_to_seq is null;

  insert into forwarder.forward_versions(
    chain_id, contract_address, forward_id,
    valid_from_seq, valid_to_seq,
    base_receiver_salt, forward_salt, intent_hash, target_chain,
    beneficiary, beneficiary_claim_only, balance_param,
    token_in, token_out, receiver_used, ephemeral_receiver,
    started_at,
    completed_at, ephemeral, amount_pulled, amount_forwarded, relayer_rebate, msg_value_refunded,
    settled_locally, bridger, expected_bridge_out, bridge_data_hash
  ) values (
    p_chain_id, p_contract_address, p_forward_id,
    p_seq, null,
    p_base_receiver_salt, p_forward_salt, p_intent_hash, p_target_chain,
    p_beneficiary, p_beneficiary_claim_only, p_balance_param,
    p_token_in, p_token_out, p_receiver_used, p_ephemeral_receiver,
    p_block_timestamp,
    null, null, null, null, null, null,
    null, null, null, null
  );
end $$ ;

create or replace function forwarder.forward_completed_set (
p_seq bigint,
p_block_timestamp bigint,
p_chain_id bigint,
p_contract_address evm_address,
p_forward_id bytes32_hex,
p_ephemeral boolean,
p_amount_pulled u256,
p_amount_forwarded u256,
p_relayer_rebate u256,
p_msg_value_refunded u256,
p_settled_locally boolean,
p_bridger evm_address,
p_expected_bridge_out u256,
p_bridge_data_hash bytes32_hex
) returns void language plpgsql as $$
declare
  cur forwarder.forward_versions%rowtype;
begin
  select * into cur
    from forwarder.forward_versions
   where chain_id = p_chain_id
     and contract_address = p_contract_address
     and forward_id = p_forward_id
     and valid_to_seq is null
   limit 1;

  if not found then
    raise exception 'ForwardCompleted without existing ForwardStarted: chain_id %, contract %, forward_id %',
      p_chain_id, p_contract_address, p_forward_id;
  end if;

  update forwarder.forward_versions
     set valid_to_seq = p_seq
   where chain_id = p_chain_id
     and contract_address = p_contract_address
     and forward_id = p_forward_id
     and valid_to_seq is null;

  insert into forwarder.forward_versions(
    chain_id, contract_address, forward_id,
    valid_from_seq, valid_to_seq,
    base_receiver_salt, forward_salt, intent_hash, target_chain,
    beneficiary, beneficiary_claim_only, balance_param,
    token_in, token_out, receiver_used, ephemeral_receiver,
    started_at,
    completed_at, ephemeral, amount_pulled, amount_forwarded, relayer_rebate, msg_value_refunded,
    settled_locally, bridger, expected_bridge_out, bridge_data_hash
  ) values (
    cur.chain_id, cur.contract_address, cur.forward_id,
    p_seq, null,
    cur.base_receiver_salt, cur.forward_salt, cur.intent_hash, cur.target_chain,
    cur.beneficiary, cur.beneficiary_claim_only, cur.balance_param,
    cur.token_in, cur.token_out, cur.receiver_used, cur.ephemeral_receiver,
    cur.started_at,
    p_block_timestamp, p_ephemeral, p_amount_pulled, p_amount_forwarded, p_relayer_rebate, p_msg_value_refunded,
    p_settled_locally, p_bridger, p_expected_bridge_out, p_bridge_data_hash
  );
end $$ ;

-- =========================
-- FORWARDER APPLY ONE
-- =========================
create or replace function forwarder.apply_one (
p_seq bigint,
p_block_timestamp bigint,
p_chain_id bigint,
p_contract_address evm_address,
p_type text,
p_args jsonb
)
returns void language plpgsql as $$
begin
  if p_type = 'OwnershipTransferred' then
    perform chain.require_json_keys(p_args, array['old_owner','new_owner']);
    perform forwarder.ownership_set(
      p_seq,
      p_chain_id,
      p_contract_address,
      (p_args->>'old_owner')::evm_address,
      (p_args->>'new_owner')::evm_address
    );

  elsif p_type = 'BridgersSet' then
    perform chain.require_json_keys(p_args, array['usdt_bridger','usdc_bridger']);
    perform forwarder.bridgers_set(
      p_seq,
      p_chain_id,
      p_contract_address,
      (p_args->>'usdt_bridger')::evm_address,
      (p_args->>'usdc_bridger')::evm_address
    );

  elsif p_type = 'QuoterSet' then
    perform chain.require_json_keys(p_args, array['token_in','quoter']);
    perform forwarder.quoter_set(
      p_seq,
      p_chain_id,
      p_contract_address,
      (p_args->>'token_in')::evm_address,
      (p_args->>'quoter')::evm_address
    );

  elsif p_type = 'ReceiverDeployed' then
    perform chain.require_json_keys(p_args, array['receiver_salt','receiver','implementation']);
    perform forwarder.receiver_deployed_set(
      p_seq,
      p_chain_id,
      p_contract_address,
      (p_args->>'receiver_salt')::bytes32_hex,
      (p_args->>'receiver')::evm_address,
      (p_args->>'implementation')::evm_address
    );

  elsif p_type = 'ForwardStarted' then
    perform chain.require_json_keys(p_args, array[
      'forward_id','base_receiver_salt','forward_salt','intent_hash',
      'target_chain','beneficiary','beneficiary_claim_only','balance_param',
      'token_in','token_out','receiver_used','ephemeral_receiver'
    ]);
    perform forwarder.forward_started_set(
      p_seq,
      p_block_timestamp,
      p_chain_id,
      p_contract_address,
      (p_args->>'forward_id')::bytes32_hex,
      (p_args->>'base_receiver_salt')::bytes32_hex,
      (p_args->>'forward_salt')::bytes32_hex,
      (p_args->>'intent_hash')::bytes32_hex,
      (p_args->>'target_chain')::bigint,
      (p_args->>'beneficiary')::evm_address,
      (p_args->>'beneficiary_claim_only')::boolean,
      (p_args->>'balance_param')::u256,
      (p_args->>'token_in')::evm_address,
      (p_args->>'token_out')::evm_address,
      (p_args->>'receiver_used')::evm_address,
      (p_args->>'ephemeral_receiver')::evm_address
    );

  elsif p_type = 'ForwardCompleted' then
    perform chain.require_json_keys(p_args, array[
      'forward_id','ephemeral','amount_pulled','amount_forwarded','relayer_rebate',
      'msg_value_refunded','settled_locally','bridger','expected_bridge_out','bridge_data_hash'
    ]);
    perform forwarder.forward_completed_set(
      p_seq,
      p_block_timestamp,
      p_chain_id,
      p_contract_address,
      (p_args->>'forward_id')::bytes32_hex,
      (p_args->>'ephemeral')::boolean,
      (p_args->>'amount_pulled')::u256,
      (p_args->>'amount_forwarded')::u256,
      (p_args->>'relayer_rebate')::u256,
      (p_args->>'msg_value_refunded')::u256,
      (p_args->>'settled_locally')::boolean,
      (p_args->>'bridger')::evm_address,
      (p_args->>'expected_bridge_out')::u256,
      (p_args->>'bridge_data_hash')::bytes32_hex
    );

  elsif p_type = 'SwapExecuted' then
    perform chain.require_json_keys(p_args, array['forward_id','token_in','token_out','min_out','actual_out']);
    insert into forwarder.swap_executed_ledger(
      event_seq, chain_id, contract_address, forward_id, token_in, token_out, min_out, actual_out
    ) values (
      p_seq,
      p_chain_id,
      p_contract_address,
      (p_args->>'forward_id')::bytes32_hex,
      (p_args->>'token_in')::evm_address,
      (p_args->>'token_out')::evm_address,
      (p_args->>'min_out')::u256,
      (p_args->>'actual_out')::u256
    );

  elsif p_type = 'BridgeInitiated' then
    perform chain.require_json_keys(p_args, array['forward_id','bridger','token_out','amount_in','target_chain']);
    insert into forwarder.bridge_initiated_ledger(
      event_seq, chain_id, contract_address, forward_id, bridger, token_out, amount_in, target_chain
    ) values (
      p_seq,
      p_chain_id,
      p_contract_address,
      (p_args->>'forward_id')::bytes32_hex,
      (p_args->>'bridger')::evm_address,
      (p_args->>'token_out')::evm_address,
      (p_args->>'amount_in')::u256,
      (p_args->>'target_chain')::bigint
    );

  else
    null;
  end if;
end $$ ;

-- =========================
-- FORWARDER APPLY CATCHUP
-- =========================
create or replace function forwarder.apply_catchup (
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
  perform pg_advisory_xact_lock(9202, hashtext(p_chain_id::text || ':' || p_contract_address::text));

  select applied_through_seq, tip
    into cur_seq, cur_tip
    from chain.stream_cursor
   where stream='forwarder'
     and chain_id = p_chain_id
     and contract_address = p_contract_address
   for update;

  if not found then
    raise exception 'stream cursor not initialized for forwarder instance (chain_id=%, contract=%) (call chain.configure_instance(''forwarder'', ...))',
      p_chain_id, p_contract_address;
  end if;

  loop
    next_seq := cur_seq + 1;

    select *
      into ev
      from chain.event_appended
     where stream='forwarder'
       and chain_id = p_chain_id
       and contract_address = p_contract_address
       and canonical
       and event_seq = next_seq
     limit 1;

    exit when not found;

    if ev.prev_tip <> cur_tip then
      raise exception 'forwarder tip mismatch at seq %, expected %, got %', next_seq, cur_tip, ev.prev_tip;
    end if;

    perform forwarder.apply_one(ev.event_seq, ev.block_timestamp, p_chain_id, p_contract_address, ev.event_type, ev.args);

    cur_seq := next_seq;
    cur_tip := ev.new_tip;
  end loop;

  update chain.stream_cursor
     set applied_through_seq = cur_seq,
         tip = cur_tip,
         updated_at = now()
   where stream='forwarder'
     and chain_id = p_chain_id
     and contract_address = p_contract_address;
end $$ ;

-- =========================
-- FORWARDER ROLLBACK
-- =========================
create or replace function forwarder.rollback_from (
p_chain_id bigint,
p_contract_address evm_address,
rollback_seq bigint
)
returns void language plpgsql as $$
begin
  -- ledgers
  delete from forwarder.bridge_initiated_ledger
   where chain_id = p_chain_id and contract_address = p_contract_address and event_seq >= rollback_seq;
  delete from forwarder.swap_executed_ledger
   where chain_id = p_chain_id and contract_address = p_contract_address and event_seq >= rollback_seq;

  -- versioned
  delete from forwarder.forward_versions
   where chain_id = p_chain_id and contract_address = p_contract_address and valid_from_seq >= rollback_seq;
  update forwarder.forward_versions
     set valid_to_seq = null
   where chain_id = p_chain_id and contract_address = p_contract_address and valid_to_seq >= rollback_seq;

  delete from forwarder.receiver_versions
   where chain_id = p_chain_id and contract_address = p_contract_address and valid_from_seq >= rollback_seq;
  update forwarder.receiver_versions
     set valid_to_seq = null
   where chain_id = p_chain_id and contract_address = p_contract_address and valid_to_seq >= rollback_seq;

  delete from forwarder.quoter_versions
   where chain_id = p_chain_id and contract_address = p_contract_address and valid_from_seq >= rollback_seq;
  update forwarder.quoter_versions
     set valid_to_seq = null
   where chain_id = p_chain_id and contract_address = p_contract_address and valid_to_seq >= rollback_seq;

  delete from forwarder.bridgers_versions
   where chain_id = p_chain_id and contract_address = p_contract_address and valid_from_seq >= rollback_seq;
  update forwarder.bridgers_versions
     set valid_to_seq = null
   where chain_id = p_chain_id and contract_address = p_contract_address and valid_to_seq >= rollback_seq;

  delete from forwarder.ownership_versions
   where chain_id = p_chain_id and contract_address = p_contract_address and valid_from_seq >= rollback_seq;
  update forwarder.ownership_versions
     set valid_to_seq = null
   where chain_id = p_chain_id and contract_address = p_contract_address and valid_to_seq >= rollback_seq;

  -- cursor rewind
  update chain.stream_cursor
     set applied_through_seq = rollback_seq - 1,
         updated_at = now()
   where stream = 'forwarder'
     and chain_id = p_chain_id
     and contract_address = p_contract_address;

  update chain.stream_cursor c
     set tip =
       case when c.applied_through_seq = 0
            then (select genesis_tip from chain.instance
                   where stream='forwarder' and chain_id=p_chain_id and contract_address=p_contract_address
                   limit 1)
            else (select e.new_tip from chain.event_appended e
                   where e.stream='forwarder'
                     and e.chain_id=p_chain_id
                     and e.contract_address=p_contract_address
                     and e.canonical
                     and e.event_seq = c.applied_through_seq
                   limit 1)
       end
   where c.stream='forwarder'
     and c.chain_id=p_chain_id
     and c.contract_address=p_contract_address;
end $$ ;

-- =========================
-- INGEST TRIGGERS (pool + forwarder)
-- =========================

create or replace function chain.on_event_appended_insert ()
returns trigger language plpgsql as $$
declare
  inst record;
begin
  for inst in (
    select distinct stream, chain_id, contract_address
    from new_rows
    where canonical
  ) loop
    if inst.stream = 'pool' then
      perform pool.apply_catchup(inst.chain_id, inst.contract_address);
    elsif inst.stream = 'forwarder' then
      perform forwarder.apply_catchup(inst.chain_id, inst.contract_address);
    end if;
  end loop;

  return null;
end $$ ;

create or replace function chain.on_event_appended_canonical_update ()
returns trigger language plpgsql as $$
declare
  rb record;
  inst record;
begin
  -- Compute per-instance rollback points for canonical TRUE -> FALSE flips.
  for rb in (
    select
      o.stream,
      o.chain_id,
      o.contract_address,
      min(o.event_seq) as rollback_seq
    from old_rows o
    join new_rows n using (id)
    where o.canonical is true and n.canonical is false
    group by o.stream, o.chain_id, o.contract_address
  ) loop
    if rb.stream = 'pool' then
      perform pool.rollback_from(rb.chain_id, rb.contract_address, rb.rollback_seq);
    elsif rb.stream = 'forwarder' then
      perform forwarder.rollback_from(rb.chain_id, rb.contract_address, rb.rollback_seq);
    end if;
  end loop;

  -- Re-apply catchup if anything changed (true->false OR false->true).
  for inst in (
    select distinct o.stream, o.chain_id, o.contract_address
    from old_rows o
    join new_rows n using (id)
    where o.canonical is distinct from n.canonical
  ) loop
    if inst.stream = 'pool' then
      perform pool.apply_catchup(inst.chain_id, inst.contract_address);
    elsif inst.stream = 'forwarder' then
      perform forwarder.apply_catchup(inst.chain_id, inst.contract_address);
    end if;
  end loop;

  return null;
end $$ ;

drop trigger if exists trg_event_appended_insert on chain.event_appended ;
create trigger trg_event_appended_insert
after insert on chain.event_appended
referencing new table as new_rows
for each statement execute function chain.on_event_appended_insert () ;

drop trigger if exists trg_event_appended_canonical_update
on chain.event_appended ;
create trigger trg_event_appended_canonical_update
after update on chain.event_appended
referencing old table as old_rows new table as new_rows
for each statement execute function chain.on_event_appended_canonical_update () ;
