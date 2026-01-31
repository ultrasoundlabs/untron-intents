use anyhow::{Context, Result};
use sqlx::{Connection, PgConnection};
use std::time::{Duration, Instant};

pub async fn wait_for_postgres(db_url: &str, timeout: Duration) -> Result<()> {
    let start = Instant::now();
    loop {
        match PgConnection::connect(db_url).await {
            Ok(mut c) => {
                sqlx::query("select 1").execute(&mut c).await?;
                return Ok(());
            }
            Err(e) => {
                if start.elapsed() > timeout {
                    return Err(e).context("postgres not ready before timeout");
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
}

pub async fn configure_postgrest_roles(db_url: &str, pgrst_auth_password: &str) -> Result<()> {
    let mut conn = PgConnection::connect(db_url).await?;
    sqlx::query(
        "do $$ \
         begin \
           if not exists (select 1 from pg_roles where rolname = 'pgrst_authenticator') then \
             create role pgrst_authenticator login inherit; \
           end if; \
           if not exists (select 1 from pg_roles where rolname = 'pgrst_anon') then \
             create role pgrst_anon nologin; \
           end if; \
           grant pgrst_anon to pgrst_authenticator; \
           grant connect on database untron to pgrst_authenticator; \
           grant usage on schema api to pgrst_authenticator; \
           grant usage on schema api to pgrst_anon; \
           grant select on all tables in schema api to pgrst_anon; \
           alter default privileges in schema api grant select on tables to pgrst_anon; \
         end $$;",
    )
    .execute(&mut conn)
    .await
    .context("configure postgrest roles (create/grants)")?;

    // `ALTER ROLE ... PASSWORD` does not accept a parameter placeholder in PostgreSQL.
    let pw = pgrst_auth_password.replace('\'', "''");
    sqlx::query(&format!("alter role pgrst_authenticator password '{pw}'"))
        .execute(&mut conn)
        .await
        .context("set pgrst_authenticator password")?;

    Ok(())
}
