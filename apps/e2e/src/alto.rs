use anyhow::{Context, Result};
use testcontainers::core::Host;
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};

pub struct AnvilContainerInstance {
    pub container: ContainerAsync<GenericImage>,
    pub host_port: u16,
    pub rpc_url: String,
}

pub struct AnvilContainerOptions {
    pub image_tag: String,
    pub network: Option<String>,
    pub container_name: Option<String>,
    pub chain_id: u64,
    pub block_time_secs: u64,
    pub mnemonic: String,
}

impl Default for AnvilContainerOptions {
    fn default() -> Self {
        Self {
            image_tag: "stable".to_string(),
            network: None,
            container_name: None,
            chain_id: 31337,
            block_time_secs: 0,
            mnemonic: "test test test test test test test test test test test junk".to_string(),
        }
    }
}

pub async fn start_anvil_container(opts: AnvilContainerOptions) -> Result<AnvilContainerInstance> {
    let mut args = vec![
        "anvil".to_string(),
        "--host".to_string(),
        "0.0.0.0".to_string(),
        "--port".to_string(),
        "8545".to_string(),
        "--chain-id".to_string(),
        opts.chain_id.to_string(),
        "--mnemonic".to_string(),
        opts.mnemonic,
    ];
    if opts.block_time_secs > 0 {
        args.push("--block-time".to_string());
        args.push(opts.block_time_secs.to_string());
    }

    let mut anvil = GenericImage::new("ghcr.io/foundry-rs/foundry".to_string(), opts.image_tag)
        .with_exposed_port(8545.tcp())
        .with_wait_for(WaitFor::message_on_stdout("Listening on"))
        .with_cmd(args);

    if let Some(network) = opts.network {
        anvil = anvil.with_network(network);
    }
    if let Some(name) = opts.container_name {
        anvil = anvil.with_container_name(name);
    }

    let container = anvil.start().await.context("start anvil container")?;
    let host_port = container.get_host_port_ipv4(8545).await?;
    let rpc_url = format!("http://127.0.0.1:{host_port}");
    Ok(AnvilContainerInstance {
        container,
        host_port,
        rpc_url,
    })
}

pub struct AltoInstance {
    pub container: ContainerAsync<GenericImage>,
    pub host_port: u16,
    pub base_url: String,
}

pub struct AltoOptions {
    pub image_tag: String,
    pub network: Option<String>,
    pub container_name: Option<String>,
    pub rpc_url: String,
    pub entrypoints_csv: String,
    pub executor_private_keys_csv: String,
    pub utility_private_key_hex: String,
    pub safe_mode: bool,
    pub log_level: String,
    pub deploy_simulations_contract: bool,
}

impl Default for AltoOptions {
    fn default() -> Self {
        Self {
            image_tag: "latest".to_string(),
            network: None,
            container_name: None,
            rpc_url: "http://anvil:8545".to_string(),
            entrypoints_csv: String::new(),
            executor_private_keys_csv: String::new(),
            utility_private_key_hex: String::new(),
            safe_mode: false,
            log_level: "info".to_string(),
            deploy_simulations_contract: true,
        }
    }
}

pub async fn start_alto(opts: AltoOptions) -> Result<AltoInstance> {
    if opts.entrypoints_csv.trim().is_empty() {
        anyhow::bail!("AltoOptions.entrypoints_csv must be set");
    }
    if opts.executor_private_keys_csv.trim().is_empty() {
        anyhow::bail!("AltoOptions.executor_private_keys_csv must be set");
    }
    if opts.utility_private_key_hex.trim().is_empty() {
        anyhow::bail!("AltoOptions.utility_private_key_hex must be set");
    }

    let args = vec![
        "--rpc-url".to_string(),
        opts.rpc_url,
        "--entrypoints".to_string(),
        opts.entrypoints_csv,
        "--executor-private-keys".to_string(),
        opts.executor_private_keys_csv,
        "--utility-private-key".to_string(),
        opts.utility_private_key_hex,
        "--safe-mode".to_string(),
        (if opts.safe_mode { "true" } else { "false" }).to_string(),
        // Keep this permissive for local chains; production configs should use staking checks.
        "--min-entity-stake".to_string(),
        "0".to_string(),
        "--min-entity-unstake-delay".to_string(),
        "0".to_string(),
        "--port".to_string(),
        "3000".to_string(),
        "--log-level".to_string(),
        opts.log_level,
        "--deploy-simulations-contract".to_string(),
        (if opts.deploy_simulations_contract {
            "true"
        } else {
            "false"
        })
        .to_string(),
    ];

    let mut alto = GenericImage::new("ghcr.io/pimlicolabs/alto".to_string(), opts.image_tag)
        .with_exposed_port(3000.tcp())
        .with_wait_for(WaitFor::Nothing)
        .with_cmd(args)
        // Ensure containers can reach a host-run Anvil in Linux CI when using host.docker.internal.
        .with_host("host.docker.internal", Host::HostGateway);

    if let Some(network) = opts.network {
        alto = alto.with_network(network);
    }
    if let Some(name) = opts.container_name {
        alto = alto.with_container_name(name);
    }

    let container = alto.start().await.context("start alto container")?;
    let host_port = container.get_host_port_ipv4(3000).await?;
    let base_url = format!("http://127.0.0.1:{host_port}");

    Ok(AltoInstance {
        container,
        host_port,
        base_url,
    })
}
