use crate::config::Stream;
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use sqlx::{
    ConnectOptions, PgPool, Postgres, QueryBuilder,
    postgres::{PgConnectOptions, PgPoolOptions},
    query_scalar,
    types::Json,
};
use std::str::FromStr;
use std::time::Duration;

#[derive(Clone)]
pub struct Db {
    pub pool: PgPool,
}

impl Db {
    pub async fn connect(database_url: &str, max_connections: u32) -> Result<Self> {
        let opts = PgConnectOptions::from_str(database_url)
            .context("parse DATABASE_URL")?
            .log_statements(tracing::log::LevelFilter::Trace)
            .log_slow_statements(tracing::log::LevelFilter::Warn, Duration::from_millis(200));

        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .connect_with(opts)
            .await
            .context("connect to database")?;

        Ok(Self { pool })
    }
}

pub async fn ensure_schema_version(db: &Db, min_version: i64) -> Result<i64> {
    let version: i64 = sqlx::query_scalar::<Postgres, i64>(
        "select coalesce(max(version), 0) from _sqlx_migrations",
    )
    .fetch_one(&db.pool)
    .await
    .context("read _sqlx_migrations version")?;

    if version < min_version {
        anyhow::bail!(
            "database schema version is {version}, but indexer expects >= {min_version} (run `cargo run -p indexer --bin migrate` against the same DATABASE_URL)"
        );
    }

    Ok(version)
}

const THE_DECLARATION: &str = "Justin Sun is responsible for setting back the inevitable global stablecoin revolution by years through exploiting Tron USDT's network effects and imposing vendor lock-in on hundreds of millions of people in the Third World, who rely on stablecoins for remittances and to store their savings in unstable, overregulated economies. Let's Untron the People.";

pub fn compute_event_chain_genesis(index_name: &str) -> alloy::primitives::B256 {
    let mut hasher = Sha256::new();
    hasher.update(index_name.as_bytes());
    hasher.update(b"\n");
    hasher.update(THE_DECLARATION.as_bytes());
    let out: [u8; 32] = hasher.finalize().into();
    alloy::primitives::B256::from(out)
}

pub async fn ensure_instance_config(
    db: &Db,
    stream: Stream,
    chain_id: u64,
    contract_address: &str,
) -> Result<()> {
    let genesis_tip = compute_event_chain_genesis(stream.index_name());
    let genesis_tip_hex = format!("0x{}", hex::encode(genesis_tip));

    contract_address
        .parse::<alloy::primitives::Address>()
        .with_context(|| {
            format!(
                "invalid {} contract address: {contract_address}",
                stream.as_str()
            )
        })?;

    let chain_id_db = i64::try_from(chain_id).context("chain_id out of range for bigint")?;

    let existing: Option<String> = sqlx::query_scalar::<Postgres, String>(
        "select genesis_tip::text from chain.instance where stream = $1::chain.stream and chain_id = $2 and contract_address = $3::evm_address",
    )
    .bind(stream.as_str())
    .bind(chain_id_db)
    .bind(contract_address)
    .fetch_optional(&db.pool)
    .await
    .map_err(|e| {
        if let Some(db_err) = e.as_database_error() && db_err.code().as_deref() == Some("42P01") {
            return anyhow::anyhow!(
                "missing required table chain.instance (did you run `cargo run -p indexer --bin migrate` against the same DATABASE_URL?)"
            );
        }
        anyhow::Error::new(e).context("read chain.instance")
    })?;

    if let Some(db_genesis_tip) = existing {
        if db_genesis_tip != genesis_tip_hex {
            anyhow::bail!(
                "chain.instance mismatch for {} (chain_id={}, contract={}): genesis_tip db={} env={}",
                stream.as_str(),
                chain_id,
                contract_address,
                db_genesis_tip,
                genesis_tip_hex
            );
        }

        // Ensure cursor row exists (manual DB setups may forget it).
        let cursor_exists: Option<i64> = query_scalar(
            "select applied_through_seq from chain.stream_cursor where stream = $1::chain.stream and chain_id = $2 and contract_address = $3::evm_address",
        )
        .bind(stream.as_str())
        .bind(chain_id_db)
        .bind(contract_address)
        .fetch_optional(&db.pool)
        .await
        .context("read chain.stream_cursor")?;

        if cursor_exists.is_none() {
            configure_instance(db, stream, chain_id, contract_address, &genesis_tip_hex).await?;
        }

        return Ok(());
    }

    configure_instance(db, stream, chain_id, contract_address, &genesis_tip_hex).await
}

async fn configure_instance(
    db: &Db,
    stream: Stream,
    chain_id: u64,
    contract_address: &str,
    genesis_tip_hex: &str,
) -> Result<()> {
    let chain_id_db = i64::try_from(chain_id).context("chain_id out of range for bigint")?;
    sqlx::query(
        "select chain.configure_instance($1::chain.stream, $2::bigint, $3::evm_address, $4::bytes32_hex)",
    )
    .bind(stream.as_str())
    .bind(chain_id_db)
    .bind(contract_address)
    .bind(genesis_tip_hex)
    .execute(&db.pool)
    .await
    .with_context(|| format!("chain.configure_instance({} chain_id={} contract={})", stream.as_str(), chain_id, contract_address))?;
    Ok(())
}

pub async fn resume_from_block(
    db: &Db,
    stream: Stream,
    chain_id: u64,
    contract_address: &str,
    deployment_block: u64,
) -> Result<u64> {
    let chain_id_db = i64::try_from(chain_id).context("chain_id out of range for bigint")?;
    let max_block: Option<i64> = query_scalar(
        "select max(block_number) from chain.event_appended where stream = $1::chain.stream and chain_id = $2 and contract_address = $3::evm_address and canonical",
    )
    .bind(stream.as_str())
    .bind(chain_id_db)
    .bind(contract_address)
    .fetch_one(&db.pool)
    .await
    .context("read max(block_number)")?;

    Ok(match max_block {
        None => deployment_block,
        Some(b) => u64::try_from(b)
            .ok()
            .unwrap_or(deployment_block)
            .saturating_add(1)
            .max(deployment_block),
    })
}

#[derive(Debug, Clone)]
pub struct StoredBlockHash {
    pub block_number: u64,
    pub block_hash_hex: String,
}

pub async fn latest_canonical_block_hash(
    db: &Db,
    stream: Stream,
    chain_id: u64,
    contract_address: &str,
) -> Result<Option<StoredBlockHash>> {
    let chain_id_db = i64::try_from(chain_id).context("chain_id out of range for bigint")?;
    let row = sqlx::query_as::<Postgres, (i64, String)>(
        r#"
        select
          block_number,
          block_hash::text as block_hash
        from chain.event_appended
        where stream = $1::chain.stream
          and chain_id = $2
          and contract_address = $3::evm_address
          and canonical
        order by block_number desc, log_index desc
        limit 1
        "#,
    )
    .bind(stream.as_str())
    .bind(chain_id_db)
    .bind(contract_address)
    .fetch_optional(&db.pool)
    .await
    .context("read latest canonical block hash")?;

    row.map(|(n, h)| {
        Ok(StoredBlockHash {
            block_number: u64::try_from(n)
                .with_context(|| format!("db block_number out of range: {n}"))?,
            block_hash_hex: h,
        })
    })
    .transpose()
}

pub async fn recent_canonical_block_hashes(
    db: &Db,
    stream: Stream,
    chain_id: u64,
    contract_address: &str,
    limit: u64,
) -> Result<Vec<StoredBlockHash>> {
    let chain_id_db = i64::try_from(chain_id).context("chain_id out of range for bigint")?;
    let rows = sqlx::query_as::<Postgres, (i64, String)>(
        r#"
        select distinct on (block_number)
          block_number,
          block_hash::text as block_hash
        from chain.event_appended
        where stream = $1::chain.stream
          and chain_id = $2
          and contract_address = $3::evm_address
          and canonical
        order by block_number desc, log_index desc
        limit $4
        "#,
    )
    .bind(stream.as_str())
    .bind(chain_id_db)
    .bind(contract_address)
    .bind(i64::try_from(limit).context("limit out of range")?)
    .fetch_all(&db.pool)
    .await
    .context("read recent canonical block hashes")?;

    rows.into_iter()
        .map(|(n, h)| {
            Ok(StoredBlockHash {
                block_number: u64::try_from(n)
                    .with_context(|| format!("db block_number out of range: {n}"))?,
                block_hash_hex: h,
            })
        })
        .collect()
}

pub async fn invalidate_from_block(
    db: &Db,
    stream: Stream,
    chain_id: u64,
    contract_address: &str,
    from_block: u64,
) -> Result<()> {
    let chain_id_db = i64::try_from(chain_id).context("chain_id out of range")?;
    let from_block_db = i64::try_from(from_block).context("from_block out of range")?;
    sqlx::query(
        "update chain.event_appended set canonical = false \
         where stream = $1::chain.stream and chain_id = $2 and contract_address = $3::evm_address \
           and canonical and block_number >= $4",
    )
    .bind(stream.as_str())
    .bind(chain_id_db)
    .bind(contract_address)
    .bind(from_block_db)
    .execute(&db.pool)
    .await
    .context("invalidate chain.event_appended")?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct EventAppendedRow {
    pub stream: Stream,
    pub chain_id: i64,
    pub contract_address: String,

    pub block_number: i64,
    pub block_timestamp: i64,
    pub block_hash: String,

    pub tx_hash: String,
    pub log_index: i32,

    pub event_seq: i64,
    pub prev_tip: String,
    pub new_tip: String,
    pub event_signature: String,
    pub abi_encoded_event_data: String,

    pub event_type: String,
    pub args_json: serde_json::Value,
}

pub async fn insert_event_appended_batch(db: &Db, rows: &[EventAppendedRow]) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    let mut qb = QueryBuilder::new(
        "insert into chain.event_appended (\
         stream, chain_id, contract_address, \
         block_number, block_timestamp, block_hash, \
         tx_hash, log_index, canonical, \
         event_seq, prev_tip, new_tip, event_signature, abi_encoded_event_data, \
         event_type, args\
         ) ",
    );

    qb.push_values(rows, |mut b, row| {
        b.push_bind(row.stream.as_str())
            .push_unseparated("::chain.stream");
        b.push_bind(row.chain_id);
        b.push_bind(&row.contract_address)
            .push_unseparated("::evm_address");

        b.push_bind(row.block_number);
        b.push_bind(row.block_timestamp);
        b.push_bind(&row.block_hash)
            .push_unseparated("::bytes32_hex");

        b.push_bind(&row.tx_hash).push_unseparated("::txhash_hex");
        b.push_bind(row.log_index);
        b.push_bind(true);

        b.push_bind(row.event_seq);
        b.push_bind(&row.prev_tip).push_unseparated("::bytes32_hex");
        b.push_bind(&row.new_tip).push_unseparated("::bytes32_hex");
        b.push_bind(&row.event_signature)
            .push_unseparated("::bytes32_hex");
        b.push_bind(&row.abi_encoded_event_data)
            .push_unseparated("::bytes_hex");

        b.push_bind(&row.event_type);
        b.push_bind(Json(&row.args_json));
    });

    qb.push(
        " on conflict (chain_id, tx_hash, log_index) do update set \
          stream = excluded.stream, \
          contract_address = excluded.contract_address, \
          block_number = excluded.block_number, \
          block_timestamp = excluded.block_timestamp, \
          block_hash = excluded.block_hash, \
          canonical = excluded.canonical, \
          event_seq = excluded.event_seq, \
          prev_tip = excluded.prev_tip, \
          new_tip = excluded.new_tip, \
          event_signature = excluded.event_signature, \
          abi_encoded_event_data = excluded.abi_encoded_event_data, \
          event_type = excluded.event_type, \
          args = excluded.args \
          where \
            chain.event_appended.stream is distinct from excluded.stream \
            or chain.event_appended.contract_address is distinct from excluded.contract_address \
            or chain.event_appended.block_number is distinct from excluded.block_number \
            or chain.event_appended.block_timestamp is distinct from excluded.block_timestamp \
            or chain.event_appended.block_hash is distinct from excluded.block_hash \
            or chain.event_appended.canonical is distinct from excluded.canonical \
            or chain.event_appended.event_seq is distinct from excluded.event_seq \
            or chain.event_appended.prev_tip is distinct from excluded.prev_tip \
            or chain.event_appended.new_tip is distinct from excluded.new_tip \
            or chain.event_appended.event_signature is distinct from excluded.event_signature \
            or chain.event_appended.abi_encoded_event_data is distinct from excluded.abi_encoded_event_data \
            or chain.event_appended.event_type is distinct from excluded.event_type \
            or chain.event_appended.args is distinct from excluded.args",
    );

    qb.build()
        .execute(&db.pool)
        .await
        .context("insert chain.event_appended")?;

    Ok(())
}
