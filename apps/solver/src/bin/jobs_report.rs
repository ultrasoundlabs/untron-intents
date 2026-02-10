use anyhow::{Context, Result};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let args: Vec<String> = std::env::args().collect();
    let mut limit: i64 = 50;
    let mut job_id: Option<i64> = None;
    let mut db_url: Option<String> = None;

    let mut i = 1usize;
    while i < args.len() {
        match args[i].as_str() {
            "--limit" => {
                i += 1;
                let v = args.get(i).context("missing value for --limit")?;
                limit = v.parse::<i64>().context("parse --limit")?;
            }
            "--job-id" => {
                i += 1;
                let v = args.get(i).context("missing value for --job-id")?;
                job_id = Some(v.parse::<i64>().context("parse --job-id")?);
            }
            "--db-url" => {
                i += 1;
                let v = args.get(i).context("missing value for --db-url")?;
                db_url = Some(v.clone());
            }
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            other => anyhow::bail!("unknown arg: {other}"),
        }
        i += 1;
    }

    let db_url = db_url
        .or_else(|| std::env::var("SOLVER_DB_URL").ok())
        .context("missing db url: pass --db-url or set SOLVER_DB_URL")?;

    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&db_url)
        .await
        .context("connect db")?;

    print_state_summary(&pool).await?;
    println!();
    print_recent_jobs(&pool, limit).await?;

    if let Some(job_id) = job_id {
        println!();
        print_job_detail(&pool, job_id).await?;
    }

    Ok(())
}

fn print_help() {
    println!("jobs_report");
    println!("  --limit <N>         Number of recent jobs to print (default: 50)");
    println!("  --job-id <ID>       Print detailed related rows for one job");
    println!("  --db-url <URL>      Postgres URL (fallback: SOLVER_DB_URL env)");
}

async fn print_state_summary(pool: &PgPool) -> Result<()> {
    println!("== solver.jobs state summary ==");
    let rows = sqlx::query(
        "select state, count(*)::bigint as n \
         from solver.jobs \
         group by state \
         order by n desc, state asc",
    )
    .fetch_all(pool)
    .await
    .context("state summary query")?;

    if rows.is_empty() {
        println!("(no jobs)");
        return Ok(());
    }

    for r in rows {
        let state: String = r.try_get("state")?;
        let n: i64 = r.try_get("n")?;
        println!("{:>7}  {}", n, state);
    }
    Ok(())
}

async fn print_recent_jobs(pool: &PgPool, limit: i64) -> Result<()> {
    println!("== recent jobs (limit={}) ==", limit);
    let rows = sqlx::query(
        "select \
            job_id, \
            encode(intent_id, 'hex') as intent_id_hex, \
            intent_type, \
            state, \
            attempts, \
            leased_by, \
            coalesce(extract(epoch from (lease_until - now()))::bigint, 0) as lease_secs_left, \
            coalesce(extract(epoch from (next_retry_at - now()))::bigint, 0) as retry_secs_left, \
            left(coalesce(last_error, ''), 120) as last_error_short, \
            updated_at::text as updated_at \
         from solver.jobs \
         order by job_id desc \
         limit $1",
    )
    .bind(limit.max(1))
    .fetch_all(pool)
    .await
    .context("recent jobs query")?;

    if rows.is_empty() {
        println!("(no jobs)");
        return Ok(());
    }

    println!(
        "{:>6}  {:<10}  {:<24}  {:>4}  {:>5}  {:>5}  {:<20}  {}",
        "job_id", "state", "intent_id", "type", "att", "lease", "updated_at", "last_error"
    );
    for r in rows {
        let job_id: i64 = r.try_get("job_id")?;
        let state: String = r.try_get("state")?;
        let intent: String = r.try_get("intent_id_hex")?;
        let intent_short = format!("{}..{}", &intent[..8], &intent[intent.len() - 8..]);
        let intent_type: i16 = r.try_get("intent_type")?;
        let attempts: i32 = r.try_get("attempts")?;
        let lease_secs_left: i64 = r.try_get("lease_secs_left")?;
        let updated_at: String = r.try_get("updated_at")?;
        let last_error_short: String = r.try_get("last_error_short")?;

        println!(
            "{:>6}  {:<10}  {:<24}  {:>4}  {:>5}  {:>5}  {:<20}  {}",
            job_id,
            state,
            intent_short,
            intent_type,
            attempts,
            lease_secs_left.max(0),
            updated_at,
            last_error_short
        );
    }
    Ok(())
}

async fn print_job_detail(pool: &PgPool, job_id: i64) -> Result<()> {
    println!("== job detail (job_id={}) ==", job_id);

    let hub_rows = sqlx::query(
        "select kind::text as kind, state::text as state, attempts, userop_hash, tx_hash, block_number, success, updated_at::text as updated_at \
         from solver.hub_userops \
         where job_id = $1 \
         order by userop_id asc",
    )
    .bind(job_id)
    .fetch_all(pool)
    .await
    .context("hub_userops detail query")?;
    println!("hub_userops: {}", hub_rows.len());
    for r in hub_rows {
        let kind: String = r.try_get("kind")?;
        let state: String = r.try_get("state")?;
        let attempts: i32 = r.try_get("attempts")?;
        let userop_hash: Option<String> = r.try_get("userop_hash")?;
        let block_number: Option<i64> = r.try_get("block_number")?;
        let success: Option<bool> = r.try_get("success")?;
        let updated_at: String = r.try_get("updated_at")?;
        println!(
            "  kind={} state={} attempts={} userop_hash={:?} block={:?} success={:?} updated_at={}",
            kind, state, attempts, userop_hash, block_number, success, updated_at
        );
    }

    let tx_rows = sqlx::query(
        "select step, encode(txid, 'hex') as txid_hex, fee_limit_sun, energy_required, tx_size_bytes, updated_at::text as updated_at \
         from solver.tron_signed_txs \
         where job_id = $1 \
         order by step asc",
    )
    .bind(job_id)
    .fetch_all(pool)
    .await
    .context("tron_signed_txs detail query")?;
    println!("tron_signed_txs: {}", tx_rows.len());
    for r in tx_rows {
        let step: String = r.try_get("step")?;
        let txid_hex: String = r.try_get("txid_hex")?;
        let fee_limit_sun: Option<i64> = r.try_get("fee_limit_sun")?;
        let energy_required: Option<i64> = r.try_get("energy_required")?;
        let tx_size_bytes: Option<i64> = r.try_get("tx_size_bytes")?;
        let updated_at: String = r.try_get("updated_at")?;
        println!(
            "  step={} txid={} fee_limit_sun={:?} energy_required={:?} tx_size_bytes={:?} updated_at={}",
            step, txid_hex, fee_limit_sun, energy_required, tx_size_bytes, updated_at
        );
    }

    let rental_rows = sqlx::query(
        "select provider, resource, balance_sun, lock_period, order_id, txid, updated_at::text as updated_at \
         from solver.tron_rentals \
         where job_id = $1 \
         order by rental_id asc",
    )
    .bind(job_id)
    .fetch_all(pool)
    .await
    .context("tron_rentals detail query")?;
    println!("tron_rentals: {}", rental_rows.len());
    for r in rental_rows {
        let provider: String = r.try_get("provider")?;
        let resource: String = r.try_get("resource")?;
        let balance_sun: i64 = r.try_get("balance_sun")?;
        let lock_period: i64 = r.try_get("lock_period")?;
        let order_id: Option<String> = r.try_get("order_id")?;
        let txid: Option<Vec<u8>> = r.try_get("txid")?;
        let txid_hex = txid.map(hex::encode);
        let updated_at: String = r.try_get("updated_at")?;
        println!(
            "  provider={} resource={} balance_sun={} lock_period={} order_id={:?} txid={:?} updated_at={}",
            provider, resource, balance_sun, lock_period, order_id, txid_hex, updated_at
        );
    }

    Ok(())
}
