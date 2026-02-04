use crate::util::repo_root;
use anyhow::{Context, Result};
use std::process::{Command, Stdio};

fn parse_forge_deployed_address(stdout: &str, stderr: &str) -> Result<String> {
    let combined = format!("{stdout}\n{stderr}");
    if let Some(pos) = combined.find("Deployed to:") {
        let tail = &combined[pos + "Deployed to:".len()..];
        if let Some(addr_pos) = tail.find("0x") {
            let addr_tail = &tail[addr_pos..];
            let addr = addr_tail
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            if addr.len() == 42 {
                return Ok(addr);
            }
        }
    }

    Err(anyhow::anyhow!(
        "failed to parse deployed address from forge output: {combined}"
    ))
}

pub fn run_forge_build() -> Result<()> {
    let root = repo_root();
    // Ensure any local patches to vendored deps are applied before compilation.
    let status = Command::new("bash")
        .args(["packages/contracts/scripts/apply_patches.sh"])
        .current_dir(&root)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("apply contract patches")?;
    if !status.success() {
        anyhow::bail!("apply contract patches failed");
    }

    let status = Command::new("forge")
        .args(["build", "--root", "packages/contracts"])
        .current_dir(&root)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("forge build")?;
    if !status.success() {
        anyhow::bail!("forge build failed");
    }
    Ok(())
}

pub fn run_forge_create_untron_intents(
    rpc_url: &str,
    private_key: &str,
    owner: &str,
) -> Result<String> {
    let root = repo_root();
    // Deploy with v3=address(0) and usdt=address(0) for indexer-only tests: createIntent doesn't call V3.
    let out = Command::new("forge")
        .args([
            "create",
            "--root",
            "packages/contracts",
            "--rpc-url",
            rpc_url,
            "--private-key",
            private_key,
            "--broadcast",
            "src/UntronIntents.sol:UntronIntents",
            "--constructor-args",
            owner,
            "0x0000000000000000000000000000000000000000",
            "0x0000000000000000000000000000000000000000",
        ])
        .current_dir(&root)
        .stdin(Stdio::null())
        .output()
        .context("forge create UntronIntents")?;

    if !out.status.success() {
        anyhow::bail!(
            "forge create failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    parse_forge_deployed_address(
        &String::from_utf8_lossy(&out.stdout),
        &String::from_utf8_lossy(&out.stderr),
    )
}

pub fn run_forge_create_untron_intents_with_args(
    rpc_url: &str,
    private_key: &str,
    owner: &str,
    v3: &str,
    usdt: &str,
) -> Result<String> {
    let root = repo_root();
    let out = Command::new("forge")
        .args([
            "create",
            "--root",
            "packages/contracts",
            "--rpc-url",
            rpc_url,
            "--private-key",
            private_key,
            "--broadcast",
            "src/UntronIntents.sol:UntronIntents",
            "--constructor-args",
            owner,
            v3,
            usdt,
        ])
        .current_dir(&root)
        .stdin(Stdio::null())
        .output()
        .context("forge create UntronIntents (args)")?;

    if !out.status.success() {
        anyhow::bail!(
            "forge create failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    parse_forge_deployed_address(
        &String::from_utf8_lossy(&out.stdout),
        &String::from_utf8_lossy(&out.stderr),
    )
}

pub fn run_forge_create_intents_forwarder(
    rpc_url: &str,
    private_key: &str,
    usdt: &str,
    usdc: &str,
    owner: &str,
) -> Result<String> {
    let root = repo_root();
    let out = Command::new("forge")
        .args([
            "create",
            "--root",
            "packages/contracts",
            "--rpc-url",
            rpc_url,
            "--private-key",
            private_key,
            "--broadcast",
            "src/IntentsForwarder.sol:IntentsForwarder",
            "--constructor-args",
            usdt,
            usdc,
            owner,
        ])
        .current_dir(&root)
        .stdin(Stdio::null())
        .output()
        .context("forge create IntentsForwarder")?;

    if !out.status.success() {
        anyhow::bail!(
            "forge create failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    parse_forge_deployed_address(
        &String::from_utf8_lossy(&out.stdout),
        &String::from_utf8_lossy(&out.stderr),
    )
}

pub fn run_forge_create_mock_erc20(
    rpc_url: &str,
    private_key: &str,
    name: &str,
    symbol: &str,
    decimals: u8,
) -> Result<String> {
    let root = repo_root();
    let out = Command::new("forge")
        .args([
            "create",
            "--root",
            "packages/contracts",
            "--rpc-url",
            rpc_url,
            "--private-key",
            private_key,
            "--broadcast",
            "src/mocks/MockERC20.sol:MockERC20",
            "--constructor-args",
            name,
            symbol,
            &decimals.to_string(),
        ])
        .current_dir(&root)
        .stdin(Stdio::null())
        .output()
        .context("forge create MockERC20")?;

    if !out.status.success() {
        anyhow::bail!(
            "forge create failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    parse_forge_deployed_address(
        &String::from_utf8_lossy(&out.stdout),
        &String::from_utf8_lossy(&out.stderr),
    )
}

pub fn run_forge_create_mock_tron_tx_reader(rpc_url: &str, private_key: &str) -> Result<String> {
    let root = repo_root();
    let out = Command::new("forge")
        .args([
            "create",
            "--root",
            "packages/contracts",
            "--rpc-url",
            rpc_url,
            "--private-key",
            private_key,
            "--broadcast",
            "src/mocks/MockTronTxReader.sol:MockTronTxReader",
        ])
        .current_dir(&root)
        .stdin(Stdio::null())
        .output()
        .context("forge create MockTronTxReader")?;

    if !out.status.success() {
        anyhow::bail!(
            "forge create failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    parse_forge_deployed_address(
        &String::from_utf8_lossy(&out.stdout),
        &String::from_utf8_lossy(&out.stderr),
    )
}

pub fn run_forge_create_entrypoint_v07(rpc_url: &str, private_key: &str) -> Result<String> {
    let root = repo_root();
    let out = Command::new("forge")
        .args([
            "create",
            "--root",
            "packages/contracts",
            "--rpc-url",
            rpc_url,
            "--private-key",
            private_key,
            "--broadcast",
            "lib/account-abstraction/contracts/core/EntryPoint.sol:EntryPoint",
        ])
        .current_dir(&root)
        .stdin(Stdio::null())
        .output()
        .context("forge create EntryPoint (v0.7)")?;

    if !out.status.success() {
        anyhow::bail!(
            "forge create EntryPoint failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    parse_forge_deployed_address(
        &String::from_utf8_lossy(&out.stdout),
        &String::from_utf8_lossy(&out.stderr),
    )
}

pub fn run_forge_create_safe_singleton(rpc_url: &str, private_key: &str) -> Result<String> {
    let root = repo_root();
    let out = Command::new("forge")
        .args([
            "create",
            "--root",
            "packages/contracts",
            "--rpc-url",
            rpc_url,
            "--private-key",
            private_key,
            "--broadcast",
            "lib/safe-smart-account/contracts/Safe.sol:Safe",
        ])
        .current_dir(&root)
        .stdin(Stdio::null())
        .output()
        .context("forge create Safe singleton")?;

    if !out.status.success() {
        anyhow::bail!(
            "forge create Safe singleton failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    parse_forge_deployed_address(
        &String::from_utf8_lossy(&out.stdout),
        &String::from_utf8_lossy(&out.stderr),
    )
}

pub fn run_forge_create_safe_proxy_factory(rpc_url: &str, private_key: &str) -> Result<String> {
    let root = repo_root();
    let out = Command::new("forge")
        .args([
            "create",
            "--root",
            "packages/contracts",
            "--rpc-url",
            rpc_url,
            "--private-key",
            private_key,
            "--broadcast",
            "lib/safe-smart-account/contracts/proxies/SafeProxyFactory.sol:SafeProxyFactory",
        ])
        .current_dir(&root)
        .stdin(Stdio::null())
        .output()
        .context("forge create SafeProxyFactory")?;

    if !out.status.success() {
        anyhow::bail!(
            "forge create SafeProxyFactory failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    parse_forge_deployed_address(
        &String::from_utf8_lossy(&out.stdout),
        &String::from_utf8_lossy(&out.stderr),
    )
}

pub fn run_forge_create_safe_module_setup(rpc_url: &str, private_key: &str) -> Result<String> {
    let root = repo_root();
    let out = Command::new("forge")
        .args([
            "create",
            "--root",
            "packages/contracts",
            "--rpc-url",
            rpc_url,
            "--private-key",
            private_key,
            "--broadcast",
            "lib/safe-modules/modules/4337/contracts/SafeModuleSetup.sol:SafeModuleSetup",
        ])
        .current_dir(&root)
        .stdin(Stdio::null())
        .output()
        .context("forge create SafeModuleSetup")?;

    if !out.status.success() {
        anyhow::bail!(
            "forge create SafeModuleSetup failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    parse_forge_deployed_address(
        &String::from_utf8_lossy(&out.stdout),
        &String::from_utf8_lossy(&out.stderr),
    )
}

pub fn run_forge_create_safe_4337_module(
    rpc_url: &str,
    private_key: &str,
    entrypoint: &str,
) -> Result<String> {
    let root = repo_root();
    let out = Command::new("forge")
        .args([
            "create",
            "--root",
            "packages/contracts",
            "--rpc-url",
            rpc_url,
            "--private-key",
            private_key,
            "--broadcast",
            "lib/safe-modules/modules/4337/contracts/Safe4337Module.sol:Safe4337Module",
            "--constructor-args",
            entrypoint,
        ])
        .current_dir(&root)
        .stdin(Stdio::null())
        .output()
        .context("forge create Safe4337Module")?;

    if !out.status.success() {
        anyhow::bail!(
            "forge create Safe4337Module failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    parse_forge_deployed_address(
        &String::from_utf8_lossy(&out.stdout),
        &String::from_utf8_lossy(&out.stderr),
    )
}

pub fn run_forge_create_test_tron_tx_reader_no_sig(
    rpc_url: &str,
    private_key: &str,
) -> Result<String> {
    let root = repo_root();
    let out = Command::new("forge")
        .args([
            "create",
            "--root",
            "packages/contracts",
            "--rpc-url",
            rpc_url,
            "--private-key",
            private_key,
            "--broadcast",
            "src/mocks/TestTronTxReaderNoSig.sol:TestTronTxReaderNoSig",
        ])
        .current_dir(&root)
        .stdin(Stdio::null())
        .output()
        .context("forge create TestTronTxReaderNoSig")?;

    if !out.status.success() {
        anyhow::bail!(
            "forge create failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    parse_forge_deployed_address(
        &String::from_utf8_lossy(&out.stdout),
        &String::from_utf8_lossy(&out.stderr),
    )
}

pub fn run_forge_create_test_tron_tx_reader_sig(
    rpc_url: &str,
    private_key: &str,
    expected_witness_address: &str,
    expected_witness_delegatee: &str,
) -> Result<String> {
    let root = repo_root();
    let out = Command::new("forge")
        .args([
            "create",
            "--root",
            "packages/contracts",
            "--rpc-url",
            rpc_url,
            "--private-key",
            private_key,
            "--broadcast",
            "src/mocks/TestTronTxReaderSig.sol:TestTronTxReaderSig",
            "--constructor-args",
            expected_witness_address,
            expected_witness_delegatee,
        ])
        .current_dir(&root)
        .stdin(Stdio::null())
        .output()
        .context("forge create TestTronTxReaderSig")?;

    if !out.status.success() {
        anyhow::bail!(
            "forge create failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    parse_forge_deployed_address(
        &String::from_utf8_lossy(&out.stdout),
        &String::from_utf8_lossy(&out.stderr),
    )
}

pub fn run_forge_create_test_tron_tx_reader_sig_allowlist(
    rpc_url: &str,
    private_key: &str,
    allowed_signers_bytes20_hex: &[String],
) -> Result<String> {
    let root = repo_root();
    let allow_arg = format!("[{}]", allowed_signers_bytes20_hex.join(","));

    let out = Command::new("forge")
        .args([
            "create",
            "--root",
            "packages/contracts",
            "--rpc-url",
            rpc_url,
            "--private-key",
            private_key,
            "--broadcast",
            "src/mocks/TestTronTxReaderSigAllowlist.sol:TestTronTxReaderSigAllowlist",
            "--constructor-args",
            &allow_arg,
        ])
        .current_dir(&root)
        .stdin(Stdio::null())
        .output()
        .context("forge create TestTronTxReaderSigAllowlist")?;

    if !out.status.success() {
        anyhow::bail!(
            "forge create failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    parse_forge_deployed_address(
        &String::from_utf8_lossy(&out.stdout),
        &String::from_utf8_lossy(&out.stderr),
    )
}

pub fn run_forge_create_mock_untron_v3(
    rpc_url: &str,
    private_key: &str,
    reader: &str,
    controller: &str,
    tron_usdt: &str,
) -> Result<String> {
    let root = repo_root();
    let out = Command::new("forge")
        .args([
            "create",
            "--root",
            "packages/contracts",
            "--rpc-url",
            rpc_url,
            "--private-key",
            private_key,
            "--broadcast",
            "src/mocks/MockUntronV3.sol:MockUntronV3",
            "--constructor-args",
            reader,
            controller,
            tron_usdt,
        ])
        .current_dir(&root)
        .stdin(Stdio::null())
        .output()
        .context("forge create MockUntronV3")?;

    if !out.status.success() {
        anyhow::bail!(
            "forge create failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    parse_forge_deployed_address(
        &String::from_utf8_lossy(&out.stdout),
        &String::from_utf8_lossy(&out.stderr),
    )
}
