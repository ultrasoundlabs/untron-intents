use crate::util::repo_root;
use anyhow::{Context, Result};
use std::process::{Command, Stdio};

pub fn cast_abi_encode(signature: &str, args: &[&str]) -> Result<String> {
    let mut cmd = Command::new("cast");
    cmd.arg("abi-encode").arg(signature);
    for a in args {
        cmd.arg(a);
    }
    let out = cmd
        .current_dir(repo_root())
        .stdin(Stdio::null())
        .output()
        .context("cast abi-encode")?;

    if !out.status.success() {
        return Err(anyhow::anyhow!(
            "cast abi-encode failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn run_cast_rpc(rpc_url: &str, method: &str, params: &[&str]) -> Result<String> {
    let mut args = vec!["rpc", "--rpc-url", rpc_url, method];
    args.extend_from_slice(params);
    let out = Command::new("cast")
        .args(args)
        .current_dir(repo_root())
        .stdin(Stdio::null())
        .output()
        .context("cast rpc")?;
    if !out.status.success() {
        anyhow::bail!("cast rpc failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn run_cast_call(rpc_url: &str, to: &str, signature: &str, args: &[&str]) -> Result<String> {
    let root = repo_root();
    let mut cmd = Command::new("cast");
    cmd.args(["call", "--rpc-url", rpc_url, to, signature]);
    for a in args {
        cmd.arg(a);
    }
    let out = cmd
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .context("cast call")?;
    if !out.status.success() {
        anyhow::bail!("cast call failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn run_cast_mint_mock_erc20(
    rpc_url: &str,
    private_key: &str,
    token: &str,
    to: &str,
    amount: &str,
) -> Result<()> {
    let status = Command::new("cast")
        .args([
            "send",
            "--rpc-url",
            rpc_url,
            "--private-key",
            private_key,
            token,
            "mint(address,uint256)",
            to,
            amount,
        ])
        .current_dir(repo_root())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("cast send mint")?;
    if !status.success() {
        anyhow::bail!("cast send mint failed");
    }
    Ok(())
}

pub fn run_cast_erc20_approve(
    rpc_url: &str,
    private_key: &str,
    token: &str,
    spender: &str,
    amount: &str,
) -> Result<()> {
    let status = Command::new("cast")
        .args([
            "send",
            "--rpc-url",
            rpc_url,
            "--private-key",
            private_key,
            token,
            "approve(address,uint256)",
            spender,
            amount,
        ])
        .current_dir(repo_root())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("cast send approve")?;
    if !status.success() {
        anyhow::bail!("cast send approve failed");
    }
    Ok(())
}

pub fn run_cast_entrypoint_deposit_to(
    rpc_url: &str,
    private_key: &str,
    entrypoint: &str,
    beneficiary: &str,
    amount_wei: &str,
) -> Result<()> {
    let status = Command::new("cast")
        .args([
            "send",
            "--rpc-url",
            rpc_url,
            "--private-key",
            private_key,
            "--value",
            amount_wei,
            entrypoint,
            "depositTo(address)",
            beneficiary,
        ])
        .current_dir(repo_root())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("cast send EntryPoint.depositTo")?;
    if !status.success() {
        anyhow::bail!("cast send depositTo failed");
    }
    Ok(())
}

pub fn run_cast_transfer_eth(
    rpc_url: &str,
    private_key: &str,
    to: &str,
    amount_wei: &str,
) -> Result<()> {
    let status = Command::new("cast")
        .args([
            "send",
            "--rpc-url",
            rpc_url,
            "--private-key",
            private_key,
            "--value",
            amount_wei,
            to,
        ])
        .current_dir(repo_root())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("cast send eth transfer")?;
    if !status.success() {
        anyhow::bail!("cast send eth transfer failed");
    }
    Ok(())
}

pub fn run_cast_create_intent(
    rpc_url: &str,
    private_key: &str,
    intents: &str,
    amount_wei: u64,
) -> Result<u64> {
    // createIntent((intentType, intentSpecs, refundBeneficiary, token, amount), deadline)
    let deadline_u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600;
    let deadline = deadline_u64.to_string();
    let amount_str = amount_wei.to_string();

    let status = Command::new("cast")
        .args([
            "send",
            "--rpc-url",
            rpc_url,
            "--private-key",
            private_key,
            "--value",
            &amount_str,
            intents,
            "createIntent((uint8,bytes,address,address,uint256),uint256)",
            &format!(
                "(0,0x,0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266,0x0000000000000000000000000000000000000000,{amount_str})"
            ),
            &deadline,
        ])
        .current_dir(repo_root())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("cast send createIntent")?;

    if !status.success() {
        anyhow::bail!("cast send failed");
    }
    Ok(deadline_u64)
}

pub fn run_cast_create_trx_transfer_intent(
    rpc_url: &str,
    private_key: &str,
    intents: &str,
    to: &str,
    amount_sun: &str,
    escrow_amount_wei: u64,
) -> Result<u64> {
    let deadline_u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600;
    let deadline = deadline_u64.to_string();
    let escrow_amount_str = escrow_amount_wei.to_string();

    let intent_specs = cast_abi_encode("f(address,uint256)", &[to, amount_sun])?;

    let status = Command::new("cast")
        .args([
            "send",
            "--rpc-url",
            rpc_url,
            "--private-key",
            private_key,
            "--value",
            &escrow_amount_str,
            intents,
            "createIntent((uint8,bytes,address,address,uint256),uint256)",
            &format!(
                "(2,{intent_specs},0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266,0x0000000000000000000000000000000000000000,{escrow_amount_str})"
            ),
            &deadline,
        ])
        .current_dir(repo_root())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("cast send createIntent (TRX_TRANSFER)")?;

    if !status.success() {
        anyhow::bail!("cast send failed");
    }
    Ok(deadline_u64)
}

pub fn run_cast_create_trx_transfer_intent_erc20(
    rpc_url: &str,
    private_key: &str,
    intents: &str,
    escrow_token: &str,
    escrow_amount: &str,
    to: &str,
    amount_sun: &str,
) -> Result<u64> {
    let deadline_u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600;
    let deadline = deadline_u64.to_string();

    let intent_specs = cast_abi_encode("f(address,uint256)", &[to, amount_sun])?;

    // TRX_TRANSFER = intentType 2
    let status = Command::new("cast")
        .args([
            "send",
            "--rpc-url",
            rpc_url,
            "--private-key",
            private_key,
            intents,
            "createIntent((uint8,bytes,address,address,uint256),uint256)",
            &format!(
                "(2,{intent_specs},0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266,{escrow_token},{escrow_amount})"
            ),
            &deadline,
        ])
        .current_dir(repo_root())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("cast send createIntent (TRX_TRANSFER, ERC20 escrow)")?;

    if !status.success() {
        anyhow::bail!("cast send failed");
    }
    Ok(deadline_u64)
}

#[allow(clippy::too_many_arguments)]
pub fn run_cast_create_delegate_resource_intent(
    rpc_url: &str,
    private_key: &str,
    intents: &str,
    receiver: &str,
    resource: u8,
    balance_sun: &str,
    lock_period: &str,
    escrow_amount_wei: u64,
) -> Result<u64> {
    let deadline_u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600;
    let deadline = deadline_u64.to_string();
    let escrow_amount_str = escrow_amount_wei.to_string();

    let intent_specs = cast_abi_encode(
        "f(address,uint8,uint256,uint256)",
        &[receiver, &resource.to_string(), balance_sun, lock_period],
    )?;

    let status = Command::new("cast")
        .args([
            "send",
            "--rpc-url",
            rpc_url,
            "--private-key",
            private_key,
            "--value",
            &escrow_amount_str,
            intents,
            "createIntent((uint8,bytes,address,address,uint256),uint256)",
            &format!(
                "(3,{intent_specs},0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266,0x0000000000000000000000000000000000000000,{escrow_amount_str})"
            ),
            &deadline,
        ])
        .current_dir(repo_root())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("cast send createIntent (DELEGATE_RESOURCE)")?;

    if !status.success() {
        anyhow::bail!("cast send failed");
    }
    Ok(deadline_u64)
}

pub fn run_cast_create_usdt_transfer_intent(
    rpc_url: &str,
    private_key: &str,
    intents: &str,
    to: &str,
    amount: &str,
    escrow_amount_wei: u64,
) -> Result<u64> {
    let deadline_u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600;
    let deadline = deadline_u64.to_string();
    let escrow_amount_str = escrow_amount_wei.to_string();

    let intent_specs = cast_abi_encode("f(address,uint256)", &[to, amount])?;

    let status = Command::new("cast")
        .args([
            "send",
            "--rpc-url",
            rpc_url,
            "--private-key",
            private_key,
            "--value",
            &escrow_amount_str,
            intents,
            "createIntent((uint8,bytes,address,address,uint256),uint256)",
            &format!(
                "(1,{intent_specs},0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266,0x0000000000000000000000000000000000000000,{escrow_amount_str})"
            ),
            &deadline,
        ])
        .current_dir(repo_root())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("cast send createIntent (USDT_TRANSFER)")?;

    if !status.success() {
        anyhow::bail!("cast send failed");
    }
    Ok(deadline_u64)
}

pub fn run_cast_create_trigger_smart_contract_intent(
    rpc_url: &str,
    private_key: &str,
    intents: &str,
    to: &str,
    call_value_sun: &str,
    data_hex: &str,
    escrow_amount_wei: u64,
) -> Result<u64> {
    let deadline_u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600;
    let deadline = deadline_u64.to_string();
    let escrow_amount_str = escrow_amount_wei.to_string();

    // IMPORTANT: `TriggerSmartContractIntent` contains a dynamic `bytes` field, so the canonical
    // encoding for Solidity `abi.decode(intentSpecs, (TriggerSmartContractIntent))` is the
    // single-argument tuple encoding (i.e. it begins with an offset word).
    //
    // This matches `abi.encode(TriggerSmartContractIntent(to, callValueSun, data))`.
    let tuple = format!("({to},{call_value_sun},{data_hex})");
    let intent_specs = cast_abi_encode("f((address,uint256,bytes))", &[&tuple])?;

    // TriggerSmartContract = intentType 0
    let status = Command::new("cast")
        .args([
            "send",
            "--rpc-url",
            rpc_url,
            "--private-key",
            private_key,
            "--value",
            &escrow_amount_str,
            intents,
            "createIntent((uint8,bytes,address,address,uint256),uint256)",
            &format!(
                "(0,{intent_specs},0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266,0x0000000000000000000000000000000000000000,{escrow_amount_str})"
            ),
            &deadline,
        ])
        .current_dir(repo_root())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("cast send createIntent (TRIGGER_SMART_CONTRACT)")?;

    if !status.success() {
        anyhow::bail!("cast send failed");
    }
    Ok(deadline_u64)
}
