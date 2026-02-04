use anyhow::{Context, Result};
use axum::{extract::State, routing::post, Json, Router};
use e2e::{
    anvil::spawn_anvil,
    binaries::{cargo_build_indexer_bins, cargo_build_solver_bin, run_migrations},
    cast::{run_cast_create_usdt_transfer_intent, run_cast_mint_mock_erc20},
    docker::{PostgresOptions, PostgrestOptions, start_postgres, start_postgrest},
    docker_cleanup::cleanup_untron_e2e_containers,
    forge::{
        run_forge_build, run_forge_create_mock_erc20, run_forge_create_mock_untron_v3,
        run_forge_create_test_tron_tx_reader_no_sig, run_forge_create_untron_intents_with_args,
    },
    http::wait_for_http_ok,
    pool_db::{wait_for_intents_solved_and_settled, wait_for_pool_current_intents_count},
    postgres::{configure_postgrest_roles, wait_for_postgres},
    process::KillOnDrop,
    services::{spawn_indexer, spawn_solver_tron_grpc_custom},
    solver_db::fetch_job_by_intent_id,
    tronbox::{decode_hex32, wait_for_tronbox_accounts, wait_for_tronbox_admin},
    util::{find_free_port, require_bins},
};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};
use tokio::sync::{oneshot, Mutex};

// Minimal TRC20-like stub (creation bytecode), compiled with `solc --optimize` (London EVM).
//
// See `apps/e2e/tests/solver_usdt_tron_grpc.rs` for rationale.
const TRON_TEST_ERC20_CREATE_BIN_HEX: &str = "6080604052348015600f57600080fd5b5061020e8061001f6000396000f3fe608060405234801561001057600080fd5b50600436106100575760003560e01c806306fdde031461005c578063313ce5671461009957806370a08231146100b357806395d89b41146100d9578063a9059cbb146100fc575b600080fd5b6100836040518060400160405280600881526020016715195cdd1554d11560c21b81525081565b6040516100909190610122565b60405180910390f35b6100a1600681565b60405160ff9091168152602001610090565b6100cb6100c136600461018c565b506402540be40090565b604051908152602001610090565b610083604051806040016040528060048152602001631554d11560e21b81525081565b61011261010a3660046101ae565b600192915050565b6040519015158152602001610090565b602081526000825180602084015260005b818110156101505760208186018101516040868401015201610133565b506000604082850101526040601f19601f83011684010191505092915050565b80356001600160a01b038116811461018757600080fd5b919050565b60006020828403121561019e57600080fd5b6101a782610170565b9392505050565b600080604083850312156101c157600080fd5b6101ca83610170565b94602093909301359350505056fea264697066735822122021e9f44579327c7fd604a8d6d040464280e15cabc63be2ba5297d3075c64c5bb64736f6c634300081e0033";

fn abi_encode_address(a: alloy::primitives::Address) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[12..].copy_from_slice(a.as_slice());
    out
}

async fn wait_for_tron_tx_included(
    grpc: &mut tron::TronGrpc,
    txid: [u8; 32],
    timeout: Duration,
) -> Result<tron::protocol::TransactionInfo> {
    let start = Instant::now();
    let mut last_err: Option<String> = None;
    loop {
        match grpc.get_transaction_info_by_id(txid).await {
            Ok(info) => {
                last_err = None;
                if info.block_number > 0 {
                    return Ok(info);
                }
            }
            Err(e) => {
                last_err = Some(e.to_string());
            }
        };
        if start.elapsed() > timeout {
            anyhow::bail!(
                "timed out waiting for tron tx inclusion: txid=0x{} last_err={:?}",
                hex::encode(txid),
                last_err
            );
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn fetch_trc20_balance_u64(
    grpc: &mut tron::TronGrpc,
    token: tron::TronAddress,
    owner: tron::TronAddress,
) -> Result<u64> {
    let msg = tron::protocol::TriggerSmartContract {
        owner_address: owner.prefixed_bytes().to_vec(),
        contract_address: token.prefixed_bytes().to_vec(),
        data: {
            let selector = [0x70, 0xa0, 0x82, 0x31]; // balanceOf(address)
            let mut data = Vec::with_capacity(4 + 32);
            data.extend_from_slice(&selector);
            data.extend_from_slice(&abi_encode_address(owner.evm()));
            data
        },
        ..Default::default()
    };

    let tx_ext = grpc
        .trigger_constant_contract(msg)
        .await
        .context("TriggerConstantContract(balanceOf)")?;
    let Some(first) = tx_ext.constant_result.first() else {
        return Ok(0);
    };
    let mut buf = [0u8; 32];
    if first.len() >= 32 {
        buf.copy_from_slice(&first[first.len() - 32..]);
    } else {
        buf[32 - first.len()..].copy_from_slice(first);
    }
    Ok(u64::from_be_bytes(buf[24..].try_into().unwrap()))
}

#[derive(Clone, Default)]
struct RentalStubState {
    requests: Arc<Mutex<Vec<serde_json::Value>>>,
}

async fn rent_handler(
    State(st): State<RentalStubState>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    st.requests.lock().await.push(body);
    Json(serde_json::json!({"success": true, "orderId": "stub-1"}))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_solver_tron_energy_rental_is_attempted() -> Result<()> {
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

    // Deploy a minimal TRC20-like stub token.
    let mut grpc = tron::TronGrpc::connect(&tron_grpc_url, None)
        .await
        .context("connect tron grpc")?;
    let create_bin = hex::decode(TRON_TEST_ERC20_CREATE_BIN_HEX)
        .context("decode TRON_TEST_ERC20_CREATE_BIN_HEX")?;

    let deploy = tron_wallet0
        .build_and_sign_deploy_contract(
            &mut grpc,
            "TestUSDT",
            create_bin,
            200_000_000,
            100,
            100_000_000,
        )
        .await
        .context("build_and_sign_deploy_contract")?;
    let deploy_ret = grpc
        .broadcast_transaction(deploy.tx)
        .await
        .context("broadcast deploy tx")?;
    if !deploy_ret.result {
        let msg_hex = hex::encode(&deploy_ret.message);
        let msg_utf8 = String::from_utf8_lossy(&deploy_ret.message);
        anyhow::bail!(
            "deploy broadcast failed: msg_hex=0x{} msg_utf8={}",
            msg_hex,
            msg_utf8
        );
    }
    let deploy_info = wait_for_tron_tx_included(&mut grpc, deploy.txid, Duration::from_secs(180))
        .await
        .context("wait deploy tx included")?;

    if deploy_info.contract_address.len() != 21
        || deploy_info.contract_address[0] != tron::TronAddress::MAINNET_PREFIX
    {
        anyhow::bail!(
            "unexpected deployed contract_address bytes: {}",
            hex::encode(&deploy_info.contract_address)
        );
    }
    let tron_token = tron::TronAddress::from_evm(alloy::primitives::Address::from_slice(
        &deploy_info.contract_address[1..],
    ));
    let tron_token_evm = format!("{:#x}", tron_token.evm());

    // Sanity-check that the token reports enough balance for the solver inventory check.
    let b0 = fetch_trc20_balance_u64(&mut grpc, tron_token, tron_wallet0.address()).await?;
    if b0 < 555 {
        anyhow::bail!("expected token balance >= 555, got {b0}");
    }

    let receiver_evm = format!("{:#x}", tron_wallet1.address().evm());
    let tron_controller_address = tron_wallet0.address().to_base58check();

    // Energy rental stub server (local).
    let rental_state = RentalStubState::default();
    let rental_port = find_free_port()?;
    let rental_addr: SocketAddr = format!("127.0.0.1:{rental_port}").parse().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let app = Router::new()
        .route("/rent", post(rent_handler))
        .with_state(rental_state.clone());
    tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(rental_addr).await.unwrap();
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
            .ok();
    });

    let rental_cfg = serde_json::json!([{
        "name": "stub",
        "url": format!("http://127.0.0.1:{rental_port}/rent"),
        "method": "POST",
        "headers": { "content-type": "application/json" },
        "body": {
            "kind": "{{resource_kind}}",
            "amount": "{{amount}}",
            "addr": "{{address_base58check}}",
            "addr_hex41": "{{address_hex41}}",
            "addr_evm": "{{address_evm_hex}}",
            "txid": "{{txid}}"
        },
        "response": {
            "success_pointer": "/success",
            "order_id_pointer": "/orderId",
            "error_pointer": "/error"
        }
    }])
    .to_string();

    // Postgres (+ docker network for PostgREST).
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

    // Deploy hub contracts.
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
        &tron_token_evm,
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

    // PostgREST.
    let pgrst_pw = "pgrst_pw";
    configure_postgrest_roles(&db_url, pgrst_pw).await?;

    let pgrst = start_postgrest(PostgrestOptions {
        network: network.clone(),
        container_name: Some(format!("untron-e2e-pgrst-{}", find_free_port()?)),
        db_uri: format!("postgres://pgrst_authenticator:{pgrst_pw}@{pg_name}:5432/untron"),
        ..Default::default()
    })
    .await?;
    let postgrest_url = pgrst.base_url.clone();
    wait_for_http_ok(&format!("{postgrest_url}/health"), Duration::from_secs(30)).await?;

    // Start solver (Tron gRPC).
    let solver = spawn_solver_tron_grpc_custom(
        &db_url,
        &postgrest_url,
        &rpc_url,
        &intents_addr,
        pk0,
        &tron_grpc_url,
        &tron_pk0,
        &format!("{},{}", tron_pk0, tron_pk1),
        &tron_controller_address,
        "solver-energy-rental",
        "usdt_transfer",
        &[("TRON_ENERGY_RENTAL_APIS_JSON", &rental_cfg)],
    )?;
    let _solver = KillOnDrop::new(solver);

    // Create USDT_TRANSFER intent (to receiver on Tron).
    let amount = "555";
    run_cast_create_usdt_transfer_intent(&rpc_url, pk0, &intents_addr, &receiver_evm, amount, 1)?;
    wait_for_pool_current_intents_count(&db_url, 1, Duration::from_secs(60)).await?;
    let intent_id = e2e::pool_db::fetch_current_intents(&db_url)
        .await?
        .first()
        .context("missing intent row")?
        .id
        .clone();

    // Wait for solver to attempt energy rental (HTTP call shape asserted).
    let start = Instant::now();
    loop {
        if start.elapsed() > Duration::from_secs(30) {
            anyhow::bail!("timed out waiting for energy rental stub request");
        }
        let reqs = rental_state.requests.lock().await;
        if let Some(last) = reqs.last() {
            let expected_base58 = tron_wallet0.address().to_base58check();
            assert_eq!(last["kind"], "energy");
            assert_eq!(last["addr"], expected_base58);
            assert!(last["amount"].as_str().unwrap_or("").parse::<u64>().unwrap_or(0) > 0);
            let txid = last["txid"].as_str().unwrap_or("");
            assert!(txid.starts_with("0x") && txid.len() == 66);
            let hex41 = last["addr_hex41"].as_str().unwrap_or("");
            assert!(hex41.starts_with("0x41") && hex41.len() == 44);
            let evm = last["addr_evm"].as_str().unwrap_or("");
            assert!(evm.starts_with("0x") && evm.len() == 42);
            break;
        }
        drop(reqs);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Wait for intent to settle.
    wait_for_intents_solved_and_settled(&db_url, 1, Duration::from_secs(180)).await?;

    // Sanity-check job is in a terminal success state.
    let job = fetch_job_by_intent_id(&db_url, &intent_id).await?;
    assert_eq!(job.state, "done");
    assert!(job.tron_txid.is_some());

    let _ = shutdown_tx.send(());
    Ok(())
}
