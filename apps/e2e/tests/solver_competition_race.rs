use anyhow::{Context, Result};
use e2e::{
    anvil::spawn_anvil,
    binaries::{cargo_build_indexer_bins, cargo_build_solver_bin, run_migrations},
    cast::{cast_abi_encode, run_cast_create_trx_transfer_intent, run_cast_mint_mock_erc20},
    docker::{PostgresOptions, PostgrestOptions, start_postgres, start_postgrest},
    docker_cleanup::cleanup_untron_e2e_containers,
    forge::{
        run_forge_build, run_forge_create_mock_erc20, run_forge_create_mock_untron_v3,
        run_forge_create_test_tron_tx_reader_no_sig, run_forge_create_untron_intents_with_args,
    },
    http::wait_for_http_ok,
    pool_db::{fetch_current_intents, wait_for_intents_solved_and_settled, wait_for_pool_current_intents_count},
    postgres::{configure_postgrest_roles, wait_for_postgres},
    process::KillOnDrop,
    services::{spawn_indexer, spawn_solver_tron_grpc_custom},
    solver_db::fetch_job_by_intent_id,
    tronbox::{decode_hex32, wait_for_tronbox_accounts, wait_for_tronbox_admin},
    util::{find_free_port, require_bins},
};
use prost::Message;
use std::time::Duration;
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};

fn now_block_number(b: &tron::protocol::BlockExtention) -> Option<i64> {
    b.block_header
        .as_ref()
        .and_then(|h| h.raw_data.as_ref())
        .map(|rd| rd.number)
}

async fn count_matching_trx_transfers(
    grpc: &mut tron::TronGrpc,
    block_from: i64,
    block_to: i64,
    owner: tron::TronAddress,
    to: tron::TronAddress,
    amount: i64,
) -> Result<usize> {
    let mut count = 0usize;
    for n in block_from..=block_to {
        let (_bext, raw_txs) = grpc
            .get_block_by_num2_raw_txs(n)
            .await
            .with_context(|| format!("get_block_by_num2_raw_txs({n})"))?;

        for raw in raw_txs {
            let tx = tron::protocol::Transaction::decode(raw.as_slice())
                .context("decode Transaction")?;
            let Some(raw_data) = tx.raw_data.as_ref() else {
                continue;
            };
            if raw_data.contract.len() != 1 {
                continue;
            }
            let c = &raw_data.contract[0];
            if c.r#type
                != tron::protocol::transaction::contract::ContractType::TransferContract as i32
            {
                continue;
            }
            let Some(any) = c.parameter.as_ref() else {
                continue;
            };
            let transfer = tron::protocol::TransferContract::decode(any.value.as_slice())
                .context("decode TransferContract")?;
            if transfer.owner_address == owner.prefixed_bytes().to_vec()
                && transfer.to_address == to.prefixed_bytes().to_vec()
                && transfer.amount == amount
            {
                count += 1;
            }
        }
    }
    Ok(count)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_two_solvers_only_one_broadcasts_final_tron_tx() -> Result<()> {
    if !require_bins(&["docker", "anvil", "forge", "cast"]) {
        return Ok(());
    }

    cleanup_untron_e2e_containers().ok();

    // Start a private Tron network (tronbox/tre).
    let tron_tag = std::env::var("TRON_TRE_TAG").unwrap_or_else(|_| "1.0.4".to_string());
    let tron = GenericImage::new("tronbox/tre".to_string(), tron_tag)
        .with_exposed_port(9090.tcp())
        .with_exposed_port(50051.tcp())
        .with_exposed_port(50052.tcp())
        .with_wait_for(WaitFor::Nothing)
        .with_container_name(format!("untron-e2e-tron-{}", find_free_port()?))
        .start()
        .await
        .context("start tronbox/tre container")?;

    let tron_http_port = tron.get_host_port_ipv4(9090).await?;
    let tron_grpc_port = tron.get_host_port_ipv4(50051).await?;
    let tron_http_base = format!("http://127.0.0.1:{tron_http_port}");
    let tron_grpc_url = format!("http://127.0.0.1:{tron_grpc_port}");

    wait_for_tronbox_admin(&tron_http_base, Duration::from_secs(240)).await?;
    let keys = wait_for_tronbox_accounts(&tron_http_base, Duration::from_secs(240)).await?;
    if keys.len() < 2 {
        anyhow::bail!("expected at least 2 tronbox accounts, got {}", keys.len());
    }
    let tron_pk0 = keys[0].clone();
    let tron_pk1 = keys[1].clone();

    let tron_wallet0 = tron::TronWallet::new(decode_hex32(&tron_pk0)?).context("tron wallet0")?;
    let tron_wallet1 = tron::TronWallet::new(decode_hex32(&tron_pk1)?).context("tron wallet1")?;

    let to_evm = format!("{:#x}", tron_wallet1.address().evm());
    let tron_controller_address = tron_wallet0.address().to_base58check();

    let network = format!("e2e-net-{}", find_free_port()?);
    let pg_name = format!("untron-e2e-pg-{}", find_free_port()?);
    let pg = start_postgres(PostgresOptions {
        network: Some(network.clone()),
        container_name: Some(pg_name.clone()),
        ..Default::default()
    })
    .await?;
    let db_url = pg.db_url.clone();
    wait_for_postgres(&db_url, Duration::from_secs(30)).await?;

    cargo_build_indexer_bins()?;
    cargo_build_solver_bin()?;
    run_migrations(&db_url, true)?;

    // Hub chain.
    let anvil_port = find_free_port()?;
    let rpc_url = format!("http://127.0.0.1:{anvil_port}");
    let _anvil = KillOnDrop::new(spawn_anvil(anvil_port)?);
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Deploy contracts.
    run_forge_build()?;
    let pk0 = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    let pk1 = "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d";
    let owner0 = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
    let owner1 = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8";

    let usdt = run_forge_create_mock_erc20(&rpc_url, pk0, "USDT", "USDT", 6)?;
    let test_reader = run_forge_create_test_tron_tx_reader_no_sig(&rpc_url, pk0)?;
    let v3 = run_forge_create_mock_untron_v3(
        &rpc_url,
        pk0,
        &test_reader,
        "0x0000000000000000000000000000000000000001",
        "0x0000000000000000000000000000000000000002",
    )?;
    let intents_addr =
        run_forge_create_untron_intents_with_args(&rpc_url, pk0, owner0, &v3, &usdt)?;

    // Fund both solvers with claim deposit USDT.
    run_cast_mint_mock_erc20(&rpc_url, pk0, &usdt, owner0, "5000000")?;
    run_cast_mint_mock_erc20(&rpc_url, pk0, &usdt, owner1, "5000000")?;

    // Start indexer (pool-only).
    let _indexer = KillOnDrop::new(spawn_indexer(
        &db_url,
        &rpc_url,
        &intents_addr,
        "pool",
        None,
    )?);

    // PostgREST.
    let pgrst_pw = "pgrst_pw";
    configure_postgrest_roles(&db_url, pgrst_pw).await?;
    let pgrst = start_postgrest(PostgrestOptions {
        network,
        container_name: Some(format!("untron-e2e-pgrst-{}", find_free_port()?)),
        db_uri: format!("postgres://pgrst_authenticator:{pgrst_pw}@{pg_name}:5432/untron"),
        ..Default::default()
    })
    .await?;
    let postgrest_url = pgrst.base_url.clone();
    wait_for_http_ok(&format!("{postgrest_url}/health"), Duration::from_secs(30)).await?;

    // Start two solvers (same Tron key, different hub keys).
    let _solver0 = KillOnDrop::new(spawn_solver_tron_grpc_custom(
        &db_url,
        &postgrest_url,
        &rpc_url,
        &intents_addr,
        pk0,
        &tron_grpc_url,
        &tron_pk0,
        &format!("{},{}", tron_pk0, tron_pk1),
        &tron_controller_address,
        "solver-race-0",
        "trx_transfer",
        &[],
    )?);
    let _solver1 = KillOnDrop::new(spawn_solver_tron_grpc_custom(
        &db_url,
        &postgrest_url,
        &rpc_url,
        &intents_addr,
        pk1,
        &tron_grpc_url,
        &tron_pk0,
        &format!("{},{}", tron_pk0, tron_pk1),
        &tron_controller_address,
        "solver-race-1",
        "trx_transfer",
        &[],
    )?);

    // Record block height so we can detect extra broadcasts.
    let mut grpc = tron::TronGrpc::connect(&tron_grpc_url, None)
        .await
        .context("connect tron grpc")?;
    let start_block = now_block_number(&grpc.get_now_block2().await?).context("start block")?;

    // Create intent.
    run_cast_create_trx_transfer_intent(&rpc_url, pk0, &intents_addr, &to_evm, "1234", 1)?;
    wait_for_pool_current_intents_count(&db_url, 1, Duration::from_secs(60)).await?;

    let intent_id = fetch_current_intents(&db_url)
        .await?
        .first()
        .context("missing intent row")?
        .id
        .clone();
    let expected_specs = cast_abi_encode("f(address,uint256)", &[&to_evm, "1234"])?;

    // Wait for settlement.
    wait_for_intents_solved_and_settled(&db_url, 1, Duration::from_secs(180)).await?;
    let job = fetch_job_by_intent_id(&db_url, &intent_id).await?;
    assert_eq!(job.state, "done");

    // Assert final tron tx is a TRX transfer matching the intent.
    let current = fetch_current_intents(&db_url).await?;
    let row = current
        .into_iter()
        .find(|r| r.id == intent_id)
        .context("missing settled intent row")?;
    assert_eq!(row.row.intent_specs, expected_specs);

    let end_block = now_block_number(&grpc.get_now_block2().await?).context("end block")?;
    let amount = 1234i64;
    let matching = count_matching_trx_transfers(
        &mut grpc,
        start_block,
        end_block,
        tron_wallet0.address(),
        tron_wallet1.address(),
        amount,
    )
    .await?;
    assert_eq!(matching, 1, "expected exactly one matching Tron transfer");

    Ok(())
}

