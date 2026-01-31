use anyhow::{Context, Result};
use e2e::{
    anvil::spawn_anvil,
    binaries::{cargo_build_indexer_bins, cargo_build_solver_bin, run_migrations},
    cast::{
        cast_abi_encode, run_cast_create_delegate_resource_intent,
        run_cast_create_trx_transfer_intent, run_cast_mint_mock_erc20,
    },
    forge::{
        run_forge_build, run_forge_create_mock_erc20, run_forge_create_mock_untron_v3,
        run_forge_create_test_tron_tx_reader_no_sig, run_forge_create_untron_intents_with_args,
    },
    http::wait_for_http_ok,
    pool_db::{wait_for_intents_solved_and_settled, wait_for_pool_current_intents_count},
    postgres::{configure_postgrest_roles, wait_for_postgres},
    process::KillOnDrop,
    services::{spawn_indexer, spawn_solver_tron_grpc},
    tronbox::{
        decode_hex32, fetch_tron_tx_by_id_from_block, wait_for_tronbox_accounts,
        wait_for_tronbox_admin,
    },
    util::{find_free_port, require_bins},
};
use prost::Message;
use std::time::Duration;
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_solver_tron_grpc_fills_trx_transfer_and_delegate_resource() -> Result<()> {
    if !require_bins(&["docker", "anvil", "forge", "cast"]) {
        return Ok(());
    }

    // Start a private Tron network (tronbox/tre).
    let tron_tag = std::env::var("TRON_TRE_TAG").unwrap_or_else(|_| "1.0.4".to_string());
    let tron = GenericImage::new("tronbox/tre".to_string(), tron_tag)
        .with_exposed_port(9090.tcp())
        .with_exposed_port(50051.tcp())
        .with_exposed_port(50052.tcp())
        .with_wait_for(WaitFor::Nothing)
        .start()
        .await
        .context("start tronbox/tre container")?;

    let tron_http_port = tron.get_host_port_ipv4(9090).await?;
    let tron_grpc_port = tron.get_host_port_ipv4(50051).await?;
    let tron_http_base = format!("http://127.0.0.1:{tron_http_port}");
    let tron_grpc_url = format!("http://127.0.0.1:{tron_grpc_port}");

    wait_for_tronbox_admin(&tron_http_base, Duration::from_secs(240)).await?;
    let keys = wait_for_tronbox_accounts(&tron_http_base, Duration::from_secs(240)).await?;
    let tron_pk0 = keys[0].clone();
    let tron_pk1 = keys[1].clone();

    let tron_pk0_bytes = decode_hex32(&tron_pk0)?;
    let tron_pk1_bytes = decode_hex32(&tron_pk1)?;

    let tron_wallet0 = tron::TronWallet::new(tron_pk0_bytes).context("tron wallet0")?;
    let tron_wallet1 = tron::TronWallet::new(tron_pk1_bytes).context("tron wallet1")?;

    let to_evm = format!("{:#x}", tron_wallet1.address().evm());
    let receiver_evm = to_evm.clone();
    let tron_controller_address = tron_wallet0.address().to_base58check();

    // Pre-stake some TRX for ENERGY so DelegateResource is permitted on fresh private chains.
    {
        let mut grpc = tron::TronGrpc::connect(&tron_grpc_url, None)
            .await
            .context("connect tron grpc (freeze)")?;
        let _freeze_txid = tron_wallet0
            .broadcast_freeze_balance_v2(&mut grpc, 2_000_000, tron::protocol::ResourceCode::Energy)
            .await
            .context("freeze_balance_v2")?;
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    let network = format!("e2e-net-{}", find_free_port()?);
    let pg_name = format!("pg-{}", find_free_port()?);

    let pg = GenericImage::new("postgres", "18.1")
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stdout(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_DB", "untron")
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_network(network.clone())
        .with_container_name(pg_name.clone())
        .start()
        .await
        .context("start postgres container")?;

    let pg_port = pg.get_host_port_ipv4(5432).await?;
    let db_url = format!("postgres://postgres:postgres@127.0.0.1:{pg_port}/untron");
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
    let owner0 = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
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

    // Fund hub solver with claim deposit USDT.
    run_cast_mint_mock_erc20(&rpc_url, pk0, &usdt, owner0, "5000000")?;

    // Start indexer (pool-only).
    let _indexer = KillOnDrop::new(spawn_indexer(
        &db_url,
        &rpc_url,
        &intents_addr,
        "pool",
        None,
    )?);

    // Create two intents.
    let _ = run_cast_create_trx_transfer_intent(&rpc_url, pk0, &intents_addr, &to_evm, "1234", 1)?;
    let _ = run_cast_create_delegate_resource_intent(
        &rpc_url,
        pk0,
        &intents_addr,
        &receiver_evm,
        1,
        "1000000",
        "10",
        1,
    )?;
    let expected_trx_specs = cast_abi_encode("f(address,uint256)", &[&to_evm, "1234"])?;
    let expected_delegate_specs = cast_abi_encode(
        "f(address,uint8,uint256,uint256)",
        &[&receiver_evm, "1", "1000000", "10"],
    )?;

    wait_for_pool_current_intents_count(&db_url, 2, Duration::from_secs(45)).await?;

    // PostgREST.
    let pgrst_pw = "pgrst_pw";
    configure_postgrest_roles(&db_url, pgrst_pw).await?;

    let pgrst = GenericImage::new("postgrest/postgrest", "v14.2")
        .with_exposed_port(3000.tcp())
        .with_wait_for(WaitFor::Nothing)
        .with_env_var(
            "PGRST_DB_URI",
            format!("postgres://pgrst_authenticator:{pgrst_pw}@{pg_name}:5432/untron"),
        )
        .with_env_var("PGRST_DB_SCHEMA", "api")
        .with_env_var("PGRST_DB_ANON_ROLE", "pgrst_anon")
        .with_network(network)
        .start()
        .await
        .context("start postgrest container")?;

    let pgrst_port = pgrst.get_host_port_ipv4(3000).await?;
    let postgrest_url = format!("http://127.0.0.1:{pgrst_port}");
    wait_for_http_ok(&format!("{postgrest_url}/health"), Duration::from_secs(30)).await?;

    // Start solver configured for Tron gRPC mode.
    let _solver = KillOnDrop::new(spawn_solver_tron_grpc(
        &db_url,
        &postgrest_url,
        &rpc_url,
        &intents_addr,
        pk0,
        &tron_grpc_url,
        &tron_pk0,
        &tron_controller_address,
    )?);

    let rows = wait_for_intents_solved_and_settled(&db_url, 2, Duration::from_secs(180)).await?;

    let mut grpc = tron::TronGrpc::connect(&tron_grpc_url, None)
        .await
        .context("connect tron grpc (assert)")?;

    for r in rows {
        assert_eq!(r.row.creator, owner0.to_ascii_lowercase());
        assert_eq!(r.row.refund_beneficiary, owner0.to_ascii_lowercase());
        assert_eq!(
            r.row.escrow_token,
            "0x0000000000000000000000000000000000000000"
        );
        assert_eq!(r.row.escrow_amount, "1");
        assert!(r.row.solver.is_some());
        assert!(r.row.solver_claimed_at.is_some());
        assert!(r.row.solved);
        assert!(r.row.funded);
        assert!(r.row.settled);
        assert!(!r.row.closed);

        let tron_tx_id =
            decode_hex32(r.row.tron_tx_id.as_ref().unwrap()).context("decode tron_tx_id")?;
        let tron_block_number = *r.row.tron_block_number.as_ref().unwrap();

        let info = grpc
            .get_transaction_info_by_id(tron_tx_id)
            .await
            .context("get_transaction_info_by_id")?;
        assert_eq!(info.block_number, tron_block_number);

        let tx = fetch_tron_tx_by_id_from_block(&mut grpc, tron_tx_id, tron_block_number).await?;
        let raw = tx.raw_data.as_ref().context("tx missing raw_data")?;
        assert_eq!(raw.contract.len(), 1);
        let c = &raw.contract[0];
        let any = c
            .parameter
            .as_ref()
            .context("tx contract missing parameter")?;

        match r.row.intent_type {
            2 => {
                assert_eq!(r.row.intent_specs, expected_trx_specs);
                assert_eq!(
                    c.r#type,
                    tron::protocol::transaction::contract::ContractType::TransferContract as i32
                );
                let transfer = tron::protocol::TransferContract::decode(any.value.as_slice())
                    .context("decode TransferContract")?;
                assert_eq!(
                    transfer.owner_address,
                    tron_wallet0.address().prefixed_bytes().to_vec()
                );
                assert_eq!(
                    transfer.to_address,
                    tron_wallet1.address().prefixed_bytes().to_vec()
                );
                assert_eq!(transfer.amount, 1234);
            }
            3 => {
                assert_eq!(r.row.intent_specs, expected_delegate_specs);
                assert_eq!(
                    c.r#type,
                    tron::protocol::transaction::contract::ContractType::DelegateResourceContract
                        as i32
                );
                let del = tron::protocol::DelegateResourceContract::decode(any.value.as_slice())
                    .context("decode DelegateResourceContract")?;
                assert_eq!(
                    del.owner_address,
                    tron_wallet0.address().prefixed_bytes().to_vec()
                );
                assert_eq!(
                    del.receiver_address,
                    tron_wallet1.address().prefixed_bytes().to_vec()
                );
                assert_eq!(del.resource, tron::protocol::ResourceCode::Energy as i32);
                assert_eq!(del.balance, 1_000_000);
                assert!(del.lock);
                assert_eq!(del.lock_period, 10);
            }
            other => anyhow::bail!("unexpected intent_type in test rows: {other} (row={r:?})"),
        }
    }

    Ok(())
}
