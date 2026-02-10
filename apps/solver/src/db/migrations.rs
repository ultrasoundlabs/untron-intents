use super::*;

const MIGRATIONS: &[(i32, &str)] = &[
    (1, include_str!("../../db/migrations/0001_schema.sql")),
    (2, include_str!("../../db/migrations/0002_jobs.sql")),
    (3, include_str!("../../db/migrations/0003_tron_proofs.sql")),
    (4, include_str!("../../db/migrations/0004_hub_userops.sql")),
    (
        5,
        include_str!("../../db/migrations/0005_circuit_breakers.sql"),
    ),
    (
        6,
        include_str!("../../db/migrations/0006_tron_signed_txs.sql"),
    ),
    (
        7,
        include_str!("../../db/migrations/0007_hub_userop_receipts.sql"),
    ),
    (
        8,
        include_str!("../../db/migrations/0008_hub_userop_gas.sql"),
    ),
    (9, include_str!("../../db/migrations/0009_intent_skips.sql")),
    (
        10,
        include_str!("../../db/migrations/0010_tron_tx_costs.sql"),
    ),
    (
        11,
        include_str!("../../db/migrations/0011_tron_tx_costs_intent_type.sql"),
    ),
    (12, include_str!("../../db/migrations/0012_ops_safety.sql")),
    (
        13,
        include_str!("../../db/migrations/0013_tron_rentals.sql"),
    ),
    (
        14,
        include_str!("../../db/migrations/0014_rental_provider_freeze.sql"),
    ),
    (
        15,
        include_str!("../../db/migrations/0015_claim_window_deadline.sql"),
    ),
];

impl SolverDb {
    pub async fn migrate(&self) -> Result<()> {
        // Prevent concurrent migrations when multiple solver processes start at once.
        //
        // IMPORTANT: advisory locks are per-session/connection. We must run the entire migration
        // sequence on a single connection, otherwise we might:
        // - acquire the lock on connection A
        // - run migrations on connection B
        // - "unlock" on connection C (leading to a warning and leaving the original lock held)
        const MIGRATION_LOCK_KEY: i64 = 0x554E_5452_4F4E_534C; // "UNTRONSL"
        let mut conn = self
            .pool
            .acquire()
            .await
            .context("acquire connection for solver migrations")?;

        sqlx::query("select pg_advisory_lock($1)")
            .bind(MIGRATION_LOCK_KEY)
            .execute(&mut *conn)
            .await
            .context("acquire solver migration lock")?;

        let res: Result<()> = async {
            // Ensure schema and migration table exist before trying to read them.
            exec_sql_batch(&mut *conn, MIGRATIONS[0].1)
                .await
                .context("apply solver schema bootstrap (v1)")?;

            for (version, sql) in MIGRATIONS {
                if *version == 1 {
                    continue;
                }
                let applied: Option<i32> = sqlx::query_scalar(
                    "select version from solver.schema_migrations where version = $1",
                )
                .bind(*version)
                .fetch_optional(&mut *conn)
                .await
                .context("read solver.schema_migrations")?;

                if applied.is_some() {
                    continue;
                }

                let mut tx = conn.begin().await.context("begin migration tx")?;
                exec_sql_batch(&mut *tx, sql)
                    .await
                    .with_context(|| format!("apply solver migration v{version}"))?;
                sqlx::query("insert into solver.schema_migrations(version) values ($1)")
                    .bind(*version)
                    .execute(&mut *tx)
                    .await
                    .context("insert solver.schema_migrations")?;
                tx.commit().await.context("commit migration tx")?;
            }
            Ok(())
        }
        .await;

        // Best-effort unlock (same connection that acquired it).
        let _ = sqlx::query("select pg_advisory_unlock($1)")
            .bind(MIGRATION_LOCK_KEY)
            .execute(&mut *conn)
            .await;

        res
    }
}

async fn exec_sql_batch<E>(exec: &mut E, sql: &str) -> Result<()>
where
    for<'c> &'c mut E: Executor<'c, Database = Postgres>,
{
    for stmt in sql.split(';') {
        let s = stmt.trim();
        if s.is_empty() {
            continue;
        }
        sqlx::query(s).execute(&mut *exec).await.with_context(|| {
            format!(
                "execute migration statement: {}",
                s.lines().next().unwrap_or("")
            )
        })?;
    }
    Ok(())
}
