use anyhow::{Context, Result};
use sqlx::Postgres;
use sqlx::postgres::PgPoolOptions;
use std::env;

#[derive(Debug, Clone)]
struct Args {
    notify_pgrst: bool,
    pgrst_channel: String,
    pgrst_payload: String,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            notify_pgrst: true,
            pgrst_channel: "pgrst".to_string(),
            pgrst_payload: "reload schema".to_string(),
        }
    }
}

fn parse_args() -> Result<Args> {
    let mut out = Args::default();
    let mut it = env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--no-notify-pgrst" => out.notify_pgrst = false,
            "--notify-pgrst" => out.notify_pgrst = true,
            "--pgrst-channel" => {
                out.pgrst_channel = it.next().context("missing value for --pgrst-channel")?;
            }
            "--pgrst-payload" => {
                out.pgrst_payload = it.next().context("missing value for --pgrst-payload")?;
            }
            other => anyhow::bail!("unknown arg: {other}"),
        }
    }
    Ok(out)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = parse_args()?;
    let database_url = env::var("DATABASE_URL").context("DATABASE_URL must be set")?;

    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await
        .context("connect to database")?;

    // Apply migrations.
    sqlx::migrate!("./db/migrations")
        .run(&pool)
        .await
        .context("run migrations")?;

    if args.notify_pgrst {
        sqlx::query("select pg_notify($1, $2)")
            .bind(&args.pgrst_channel)
            .bind(&args.pgrst_payload)
            .execute(&pool)
            .await
            .with_context(|| {
                format!(
                    "notify PostgREST channel={} payload={}",
                    args.pgrst_channel, args.pgrst_payload
                )
            })?;
    }

    // Helpful in logs/CI: print the schema migration version.
    let version: i64 = sqlx::query_scalar::<Postgres, i64>(
        "select coalesce(max(version), 0) from _sqlx_migrations",
    )
    .fetch_one(&pool)
    .await
    .context("read migration version")?;

    println!("migrations applied, schema version={version}");

    Ok(())
}
