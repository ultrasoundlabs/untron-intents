/*
Foundation:
- Schemas: chain/pool/forwarder are internal; api is exposed via PostgREST.
- Domains/types: constrain addresses/hashes/bytes and large integers.
- Helpers: JSON key presence enforcement.

Design choice: store addresses/hashes as TEXT with strict regex constraints.
This keeps data human-readable and PostgREST outputs friendly.
*/

-- =========================
-- SCHEMAS
-- =========================
create schema if not exists chain;
create schema if not exists pool;
create schema if not exists forwarder;
create schema if not exists api;
create schema if not exists extensions;

-- =========================
-- EXTENSIONS
-- =========================
-- digest() for checksums
create extension if not exists pgcrypto with schema extensions ;

-- =========================
-- DOMAINS (canonical text representations)
-- =========================

-- EVM addresses must be stored in a checksummed form.
-- All other byte types must be stored in all lowercase hex.
create domain public.evm_address as text
check (value ~ '^0x[0-9a-fA-F]{40}$') ;

-- Tron mainnet base58check addresses (0x41 || addr20).
-- Note: regex validation only (checksum not verified here).
create domain public.tron_address as text
check (value ~ '^T[1-9A-HJ-NP-Za-km-z]{33}$') ;

-- Multi-chain "address string" for tables that can contain either chain.
create domain public.chain_address as text
check (
value ~ '^0x[0-9a-fA-F]{40}$'
or value ~ '^T[1-9A-HJ-NP-Za-km-z]{33}$'
) ;

create domain public.bytes32_hex as text
check (value ~ '^0x[0-9a-f]{64}$') ;

create domain public.txhash_hex as text
check (value ~ '^0x[0-9a-f]{64}$') ;

-- Variable-length bytes as 0x-prefixed even-length hex.
-- (Used for ABI blobs / payloads.)
create domain public.bytes_hex as text
check (value ~ '^0x([0-9a-f]{2})*$') ;

-- uint256 fits within 78 decimal digits (2^256 ~ 1.15e77)
create domain public.u256 as numeric(78, 0)
check (value >= 0);

-- int256 fits in numeric(78,0) comfortably
create domain public.i256 as numeric(78, 0);

-- =========================
-- TRON ADDRESS HELPERS
-- =========================
/*
Tron "base58check" address encoding:
- payload = 0x41 || addr20  (21 bytes)
- checksum = first 4 bytes of SHA256(SHA256(payload))
- b58 = base58(payload || checksum)

We keep this in the DB mainly for deterministic projection + API output.
If performance becomes an issue, do the conversion in the worker instead.
*/
create or replace function chain.tron_b58check_encode (addr20 bytea)
returns public.tron_address
language plpgsql
set search_path = ''
immutable strict
as $$
declare
    alphabet constant text[] := array[
      '1','2','3','4','5','6','7','8','9',
      'A','B','C','D','E','F','G','H','J','K','L','M','N','P','Q','R','S','T','U','V','W','X','Y','Z',
      'a','b','c','d','e','f','g','h','i','j','k','m','n','o','p','q','r','s','t','u','v','w','x','y','z'
    ];
    payload   bytea;          -- 21-byte (41 || addr20)
    checksum  bytea;          -- 4-byte double-SHA256
    num       numeric := 0;   -- big-int accumulator
    digit     int;
    enc       text   := '';
    i         int;
begin
    if length(addr20) <> 20 then
        raise exception 'Expected 20-byte raw address, got % bytes', length(addr20);
    end if;

    payload  := E'\\x41'::bytea || addr20;
    checksum := substring(extensions.digest(extensions.digest(payload,'sha256'),'sha256') for 4);
    payload  := payload || checksum; -- 25 bytes total

    -- binary -> numeric (base-256)
    for i in 0 .. length(payload)-1 loop
        num := num * 256 + get_byte(payload,i);
    end loop;

    -- numeric -> base-58 string
    while num > 0 loop
        digit := mod(num, 58)::int;
        num   := (num - digit) / 58;
        enc   := alphabet[digit+1] || enc;
    end loop;

    -- leading zero bytes -> leading '1'
    i := 0;
    while i < length(payload) and get_byte(payload,i) = 0 loop
        enc := '1' || enc;
        i   := i + 1;
    end loop;

    return enc::public.tron_address;
end;
$$ ;

create or replace function chain.evm_address_to_bytes20 (
p_addr public.evm_address
)
returns bytea
language sql
immutable strict
set search_path = ''
as $$
  select decode(substr(p_addr, 3), 'hex')
$$ ;

create or replace function chain.tron_address_from_evm (
p_addr public.evm_address
)
returns public.tron_address
language sql
immutable strict
set search_path = ''
as $$
  select chain.tron_b58check_encode(chain.evm_address_to_bytes20(p_addr))
$$ ;

create or replace function chain.tron_address_from_text (p_addr text)
returns public.tron_address
language plpgsql
immutable strict
set search_path = ''
as $$
begin
  if p_addr ~ '^T[1-9A-HJ-NP-Za-km-z]{33}$' then
    return p_addr::public.tron_address;
  elsif p_addr ~ '^0x[0-9a-fA-F]{40}$' then
    return chain.tron_address_from_evm(p_addr::public.evm_address);
  end if;

  raise exception 'Invalid Tron address text: %', p_addr;
end;
$$ ;

-- =========================
-- HASHING / ABI HELPERS
-- =========================

create or replace function chain.bytes32_hex_to_bytea(p public.bytes32_hex)
returns bytea
language sql
immutable strict
set search_path = ''
as $$
  select decode(substr(p, 3), 'hex')
$$;

create or replace function chain.abi_encode_evm_address(p_addr public.evm_address)
returns bytea
language sql
immutable strict
set search_path = ''
as $$
  select decode(repeat('00', 12), 'hex') || chain.evm_address_to_bytes20(p_addr)
$$;

create or replace function chain.keccak256_bytes32_hex(p bytea)
returns public.bytes32_hex
language plpgsql
immutable strict
set search_path = ''
as $$
declare
  out bytea;
begin
  -- pgcrypto's digest() delegates to OpenSSL digests (when built with OpenSSL).
  -- Try a few common names across OpenSSL builds.
  begin
    out := extensions.digest(p, 'keccak256');
    return ('0x' || encode(out, 'hex'))::public.bytes32_hex;
  exception when others then
    null;
  end;

  begin
    out := extensions.digest(p, 'keccak-256');
    return ('0x' || encode(out, 'hex'))::public.bytes32_hex;
  exception when others then
    null;
  end;

  begin
    out := extensions.digest(p, 'KECCAK-256');
    return ('0x' || encode(out, 'hex'))::public.bytes32_hex;
  exception when others then
    null;
  end;

  raise exception 'keccak256 digest not available (pgcrypto/OpenSSL build missing KECCAK-256)';
end;
$$;

create or replace function chain.intent_hash_from_receiver_params(
    p_forwarder public.evm_address,
    p_to_tron_evm public.evm_address
)
returns public.bytes32_hex
language sql
immutable strict
set search_path = ''
as $$
  -- Solidity: keccak256(abi.encode(forwarder, toTron))
  select chain.keccak256_bytes32_hex(
    chain.abi_encode_evm_address(p_forwarder)
    || chain.abi_encode_evm_address(p_to_tron_evm)
  )
$$;

-- =========================
-- STREAM ENUM
-- =========================
create type chain.stream as enum ('pool', 'forwarder') ;

-- =========================
-- JSON VALIDATION HELPERS
-- =========================
/*
Projection depends on worker-decoded JSON being correct.
We fail loudly on missing keys to avoid silent state corruption.
*/
create or replace function chain.require_json_keys (
p_args jsonb,
p_keys text []
)
returns void language plpgsql as $$
declare
  k text;
begin
  foreach k in array p_keys loop
    if not (p_args ? k) then
      raise exception 'missing required json key: % in %', k, p_args;
    end if;
  end loop;
end $$ ;

-- Note: PostgREST roles/passwords/grants are intentionally managed out-of-band
-- (e.g. via docker init scripts / IaC), not in migrations.

-- =============================================================================
-- POSTGREST / OPENAPI DOC COMMENTS
-- =============================================================================
-- PostgREST includes SQL comments in its OpenAPI output as `description`
-- fields.
-- We keep these comments extremely explicit because most API consumers will see
-- only the generated OpenAPI (not the underlying protocol code).

-- Schemas
comment on schema chain is
$$Untron Intents indexer: internal ingestion + reorg engine (not public)

Contains the canonical event streams (`chain.event_appended`) for both Untron Intents pool
(EVM, `UntronIntentsIndex`) and forwarders on EVM chains (`IntentsForwarder`), plus the
projector cursor machinery that applies events to the `pool.*` and `forwarder.*` derived tables.

This schema is intentionally NOT exposed directly via PostgREST; only `api.*` views
are exposed.$$ ;

comment on schema pool is
$$Untron Intents indexer: pool projection (derived state, not public)

Materialized tables derived from the canonical `pool` event stream, i.e. events emitted
by the EVM-side `UntronIntentsIndex` contract (the UntronIntents pool). Tables here represent
either "current state" (versioned tables with `valid_to_seq is null`) or append-only
ledgers for actions.$$ ;

comment on schema forwarder is
$$Untron Intents indexer: forwarders projection (derived state, not public)

Materialized tables derived from the canonical `forwarder` event stream, i.e. events
emitted by the EVM `IntentsForwarderIndex` contracts (receiver sweeps + bridging).
Tables here represent either "current state" (versioned tables) or append-only ledgers.$$ ;

comment on schema api is
$$Untron V3 Indexer API

Read-only PostgREST schema.

All objects in this schema are views over internal tables in `chain`, `pool`, and `forwarder`.
The intent is to expose a stable HTTP API surface while allowing internal refactors of
storage/projection logic.$$ ;

comment on schema extensions is
$$Untron Intents indexer: extensions schema (internal)

Holds PostgreSQL extensions (e.g. `pgcrypto`) used by the ingestion/projection logic.$$ ;

-- Domains (types)
comment on domain public.evm_address is
$$EVM address (0x + 20 bytes) as text

Used for all EVM-chain addresses in the indexer. Regex validation is applied.
Note: we do NOT enforce EIP-55 checksum at the database layer; upstream components
should normalize addresses.$$ ;

comment on domain public.tron_address is
$$Tron mainnet address (base58check) as text

Used for all Tron addresses in the indexer. Regex validation is applied.
Note: we do NOT verify base58check checksums at the database layer.$$ ;

comment on domain public.chain_address is
$$Multi-chain address string (EVM or Tron)

Used when a column may contain either an EVM address (0x...) or a Tron base58 address (T...).
This is primarily used for cross-chain "origin token/address" fields that come from protocol
event metadata.$$ ;

comment on domain public.bytes32_hex is
$$0x-prefixed lowercase hex bytes32 string

Used for hash-chain tips and other 32-byte values that are represented canonically as lowercase hex.$$ ;

comment on domain public.txhash_hex is
$$0x-prefixed lowercase hex transaction hash string

Used for transaction hashes in the ingestion layer. Always stored as lowercase hex.$$ ;

comment on domain public.bytes_hex is
$$0x-prefixed lowercase hex bytes (variable length)

Used for ABI blobs and payloads, including `abi_encoded_event_data` which must match exactly the
bytes that were hashed onchain into Untron's event hash-chains.$$ ;

comment on domain public.u256 is
$$Unsigned 256-bit integer stored as NUMERIC(78,0)

Solidity `uint256` does not fit in Postgres BIGINT, so we store it as NUMERIC with sufficient precision.
This domain is used for onchain amounts, ids, nonces, and other uint256-valued fields.$$ ;

comment on domain public.i256 is
$$Signed 256-bit integer stored as NUMERIC(78,0)

-- Enum types
comment on type chain.stream is
$$ Untron event stream identifier

- `pool` = UntronIntents pool stream (events emitted via `UntronIntentsIndex`)
- `forwarder` = Forwarders stream (events emitted via `UntronControllerIndex`) $$;

-- Helpers
comment on function chain.tron_b58check_encode(bytea) is
$$ Encode a 20 - byte raw EVM address into a Tron base58check address

Input: 20 - byte address (no 0x41 prefix).
Process: prepend Tron prefix (0x41), compute checksum, then base58 encode.

This is used to deterministically project Tron receiver addresses or event metadata into the DB
in a human - readable format.$$;

comment on function chain.evm_address_to_bytes20(public.evm_address) is
$$ Convert an EVM address text (0x...) into 20 raw bytes

This is a helper for Tron address conversion and for deterministic receiver address computations.$$;

comment on function chain.tron_address_from_evm(public.evm_address) is
$$ Convert an EVM address into the corresponding Tron base58check address
(0x41 prefix)

This is used when a protocol event or configuration value is stored in EVM form but must be
represented as a Tron address in the indexed data.$$;

comment on function chain.tron_address_from_text(text) is
$$ Parse a text value as either Tron base58 (T...) or EVM 0x...and return a
Tron base58 address

- If the input is already a Tron base58 address, returns it.
- If the input is an EVM address, converts it to the Tron base58 form.

This is used by projectors when they receive address strings from the worker in JSON form.$$;

comment on function chain.require_json_keys(jsonb, text []) is
$$ Projection safety: require a set of keys to exist in a JSON args blob

The worker ingests onchain events into `chain.event_appended.args` (JSON).
Projection functions (`pool.apply_one`,
`forwarder.apply_one`) call this helper to fail loudly if the
ingestion layer changed and a required key is missing,
preventing silent state corruption.
$$;
