use anyhow::{Context, Result};
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};

pub struct PostgresInstance {
    pub container: ContainerAsync<GenericImage>,
    pub host_port: u16,
    pub db_url: String,
    /// The docker network the container joined (if any).
    pub network: Option<String>,
    /// The container name (if any).
    pub container_name: Option<String>,
}

pub struct PostgresOptions {
    pub image_tag: String,
    pub db: String,
    pub user: String,
    pub password: String,
    pub network: Option<String>,
    pub container_name: Option<String>,
}

impl Default for PostgresOptions {
    fn default() -> Self {
        Self {
            image_tag: "18.1".to_string(),
            db: "untron".to_string(),
            user: "postgres".to_string(),
            password: "postgres".to_string(),
            network: None,
            container_name: None,
        }
    }
}

pub async fn start_postgres(opts: PostgresOptions) -> Result<PostgresInstance> {
    let mut pg = GenericImage::new("postgres".to_string(), opts.image_tag)
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stdout(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_DB", opts.db.clone())
        .with_env_var("POSTGRES_USER", opts.user.clone())
        .with_env_var("POSTGRES_PASSWORD", opts.password.clone());

    if let Some(network) = opts.network.clone() {
        pg = pg.with_network(network);
    }
    if let Some(name) = opts.container_name.clone() {
        pg = pg.with_container_name(name);
    }

    let container = pg.start().await.context("start postgres container")?;
    let host_port = container.get_host_port_ipv4(5432).await?;
    let db_url = format!(
        "postgres://{}:{}@127.0.0.1:{}/{}",
        opts.user, opts.password, host_port, opts.db
    );

    Ok(PostgresInstance {
        container,
        host_port,
        db_url,
        network: opts.network,
        container_name: opts.container_name,
    })
}

pub struct PostgrestInstance {
    pub container: ContainerAsync<GenericImage>,
    pub host_port: u16,
    pub base_url: String,
}

pub struct PostgrestOptions {
    pub image_tag: String,
    pub network: String,
    pub db_uri: String,
    pub db_schema: String,
    pub anon_role: String,
}

impl Default for PostgrestOptions {
    fn default() -> Self {
        Self {
            image_tag: "v14.2".to_string(),
            network: "bridge".to_string(),
            db_uri: "postgres://pgrst_authenticator:pw@postgres:5432/untron".to_string(),
            db_schema: "api".to_string(),
            anon_role: "pgrst_anon".to_string(),
        }
    }
}

pub async fn start_postgrest(opts: PostgrestOptions) -> Result<PostgrestInstance> {
    let pgrst = GenericImage::new("postgrest/postgrest".to_string(), opts.image_tag)
        .with_exposed_port(3000.tcp())
        .with_wait_for(WaitFor::Nothing)
        .with_env_var("PGRST_DB_URI", opts.db_uri)
        .with_env_var("PGRST_DB_SCHEMA", opts.db_schema)
        .with_env_var("PGRST_DB_ANON_ROLE", opts.anon_role)
        .with_network(opts.network)
        .start()
        .await
        .context("start postgrest container")?;

    let host_port = pgrst.get_host_port_ipv4(3000).await?;
    let base_url = format!("http://127.0.0.1:{host_port}");

    Ok(PostgrestInstance {
        container: pgrst,
        host_port,
        base_url,
    })
}
