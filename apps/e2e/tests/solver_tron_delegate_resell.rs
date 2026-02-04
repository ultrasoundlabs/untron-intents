use anyhow::{Context, Result};
use axum::{Json, Router, extract::State, routing::post};
use e2e::{
    anvil::spawn_anvil,
    binaries::{cargo_build_indexer_bins, cargo_build_solver_bin, run_migrations},
    cast::{run_cast_create_delegate_resource_intent, run_cast_mint_mock_erc20},
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
    tronbox::{
        decode_hex32, fetch_tron_tx_by_id_from_block, wait_for_tronbox_accounts,
        wait_for_tronbox_admin,
    },
    util::{find_free_port, require_bins},
};
use prost::Message;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};
use tokio::sync::{Mutex, oneshot};

#[derive(Clone)]
struct RentalStubState {
    provider_key: [u8; 32],
    tron_grpc_url: String,
    receiver: tron::TronAddress,
    last_txid: Arc<Mutex<Option<[u8; 32]>>>,
}

async fn rent_handler(
    State(st): State<RentalStubState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    let kind = body["kind"].as_str().unwrap_or("");
    if kind != "energy" && kind != "bandwidth" && kind != "tron_power" {
        return Err(axum::http::StatusCode::BAD_REQUEST);
    }
    let amount_units: u64 = body["amount"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let lock_period: i64 = body["lock_period"]
        .as_str()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    if amount_units == 0 || lock_period <= 0 {
        return Err(axum::http::StatusCode::BAD_REQUEST);
    }

    let (resource, totals) = match kind {
        "energy" => (tron::protocol::ResourceCode::Energy, "energy"),
        "bandwidth" => (tron::protocol::ResourceCode::Bandwidth, "bandwidth"),
        "tron_power" => (tron::protocol::ResourceCode::TronPower, "tron_power"),
        _ => (tron::protocol::ResourceCode::Energy, "energy"),
    };

    let wallet =
        tron::TronWallet::new(st.provider_key).map_err(|_| axum::http::StatusCode::BAD_REQUEST)?;
    let mut grpc = tron::TronGrpc::connect(&st.tron_grpc_url, None)
        .await
        .map_err(|_| axum::http::StatusCode::BAD_REQUEST)?;

    let msg = grpc
        .get_account_resource(st.receiver.prefixed_bytes().to_vec())
        .await
        .map_err(|_| axum::http::StatusCode::SERVICE_UNAVAILABLE)?;
    let stake_totals = match totals {
        "energy" => tron::resources::parse_energy_stake_totals(&msg)
            .map_err(|_| axum::http::StatusCode::SERVICE_UNAVAILABLE)?,
        "bandwidth" => tron::resources::parse_net_stake_totals(&msg)
            .map_err(|_| axum::http::StatusCode::SERVICE_UNAVAILABLE)?,
        _ => return Err(axum::http::StatusCode::BAD_REQUEST),
    };
    let balance_sun = tron::resources::trx_sun_for_resource_units(amount_units, stake_totals);
    let balance_sun =
        i64::try_from(balance_sun).map_err(|_| axum::http::StatusCode::SERVICE_UNAVAILABLE)?;

    let txid = wallet
        .broadcast_delegate_resource_contract(
            &mut grpc,
            st.receiver,
            resource,
            balance_sun,
            true,
            lock_period,
        )
        .await
        .map_err(|_| axum::http::StatusCode::SERVICE_UNAVAILABLE)?;

    *st.last_txid.lock().await = Some(txid);
    Ok(Json(serde_json::json!({
        "success": true,
        "txid": format!("0x{}", hex::encode(txid))
    })))
}

async fn rent_fail_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "success": false,
        "error": "no_liquidity"
    }))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_solver_delegate_resource_resell_via_rental_api() -> Result<()> {
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

    let receiver_pk = decode_hex32(&keys[0])?;
    let provider_pk = decode_hex32(&keys[1])?;
    let receiver_wallet = tron::TronWallet::new(receiver_pk).context("receiver wallet")?;
    let provider_wallet = tron::TronWallet::new(provider_pk).context("provider wallet")?;

    // Pre-stake some TRX for ENERGY so DelegateResource is permitted.
    {
        let mut grpc = tron::TronGrpc::connect(&tron_grpc_url, None)
            .await
            .context("connect tron grpc (freeze)")?;
        provider_wallet
            .broadcast_freeze_balance_v2(&mut grpc, 2_000_000, tron::protocol::ResourceCode::Energy)
            .await
            .context("freeze_balance_v2")?;
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    // Rental stub server (local).
    let rental_port = find_free_port()?;
    let rental_addr: SocketAddr = format!("127.0.0.1:{rental_port}").parse().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let rental_state = RentalStubState {
        provider_key: provider_pk,
        tron_grpc_url: tron_grpc_url.clone(),
        receiver: receiver_wallet.address(),
        last_txid: Arc::new(Mutex::new(None)),
    };
    let app = Router::new()
        .route("/rent", post(rent_handler))
        .route("/rent_fail", post(rent_fail_handler))
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

    let rental_cfg = serde_json::json!([
        {
            "name": "bad",
            "url": format!("http://127.0.0.1:{rental_port}/rent_fail"),
            "method": "POST",
            "headers": { "content-type": "application/json" },
            "body": {
                "kind": "{{resource_kind}}",
                "amount": "{{amount}}",
                "lock_period": "{{lock_period}}",
                "receiver": "{{address_base58check}}"
            },
            "response": {
                "success_pointer": "/success",
                "txid_pointer": "/txid",
                "error_pointer": "/error"
            }
        },
        {
            "name": "good",
            "url": format!("http://127.0.0.1:{rental_port}/rent"),
            "method": "POST",
            "headers": { "content-type": "application/json" },
            "body": {
                "kind": "{{resource_kind}}",
                "amount": "{{amount}}",
                "lock_period": "{{lock_period}}",
                "receiver": "{{address_base58check}}"
            },
            "response": {
                "success_pointer": "/success",
                "txid_pointer": "/txid",
                "error_pointer": "/error"
            }
        }
    ])
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

    // Start solver configured for Tron gRPC mode + resell enabled.
    let tron_controller_address = receiver_wallet.address().to_base58check();
    let tron_pk_hex = format!("0x{}", hex::encode(receiver_pk));
    let tron_pk_csv = format!("0x{}", hex::encode(receiver_pk));
    let _solver = KillOnDrop::new(spawn_solver_tron_grpc_custom(
        &db_url,
        &postgrest_url,
        &rpc_url,
        &intents_addr,
        pk0,
        &tron_grpc_url,
        &tron_pk_hex,
        &tron_pk_csv,
        &tron_controller_address,
        "solver-resell",
        "delegate_resource",
        &[
            ("TRON_DELEGATE_RESOURCE_RESELL_ENABLED", "true"),
            ("TRON_ENERGY_RENTAL_APIS_JSON", &rental_cfg),
            // Force a fast freeze so the solver deterministically falls back from "bad" -> "good".
            ("TRON_RENTAL_PROVIDER_FAIL_THRESHOLD", "1"),
            ("TRON_RENTAL_PROVIDER_FAIL_WINDOW_SECS", "60"),
            ("TRON_RENTAL_PROVIDER_FREEZE_SECS", "600"),
        ],
    )?);

    // Create a DelegateResource intent. Receiver is the solver's Tron account, but the delegation
    // tx must be sent by the "provider" account (wallet1) via the rental API.
    let receiver_evm = format!("{:#x}", receiver_wallet.address().evm());
    run_cast_create_delegate_resource_intent(
        &rpc_url,
        pk0,
        &intents_addr,
        &receiver_evm,
        1,         // ENERGY
        "1000000", // balanceSun
        "10",      // lockPeriod
        1,
    )?;
    wait_for_pool_current_intents_count(&db_url, 1, Duration::from_secs(60)).await?;

    let rows = wait_for_intents_solved_and_settled(&db_url, 1, Duration::from_secs(180)).await?;
    let row = rows.first().context("missing intent row")?;

    let tron_tx_id =
        decode_hex32(row.row.tron_tx_id.as_ref().unwrap()).context("decode tron_tx_id")?;
    let tron_block_number = *row.row.tron_block_number.as_ref().unwrap();

    let mut grpc = tron::TronGrpc::connect(&tron_grpc_url, None)
        .await
        .context("connect tron grpc (assert)")?;
    let tx = fetch_tron_tx_by_id_from_block(&mut grpc, tron_tx_id, tron_block_number).await?;
    let raw = tx.raw_data.as_ref().context("tx missing raw_data")?;
    assert_eq!(raw.contract.len(), 1);
    let c = &raw.contract[0];
    assert_eq!(
        c.r#type,
        tron::protocol::transaction::contract::ContractType::DelegateResourceContract as i32
    );
    let any = c
        .parameter
        .as_ref()
        .context("tx contract missing parameter")?;
    let del = tron::protocol::DelegateResourceContract::decode(any.value.as_slice())
        .context("decode DelegateResourceContract")?;
    assert_eq!(
        del.owner_address,
        provider_wallet.address().prefixed_bytes().to_vec(),
        "expected provider to be tx owner"
    );
    assert_eq!(
        del.receiver_address,
        receiver_wallet.address().prefixed_bytes().to_vec()
    );

    // Assert circuit breaker state persisted + rendered request stored.
    {
        let pool = sqlx::PgPool::connect(&db_url)
            .await
            .context("connect db (assert)")?;

        let frozen: Option<i64> = sqlx::query_scalar(
            "select extract(epoch from frozen_until)::bigint from solver.rental_provider_freezes where provider = $1",
        )
        .bind("bad")
        .fetch_optional(&pool)
        .await
        .context("query rental_provider_freezes")?;
        assert!(frozen.is_some(), "expected provider 'bad' to be frozen");

        let (provider, request_json): (String, Option<serde_json::Value>) = sqlx::query_as(
            "select provider, request_json from solver.tron_rentals order by job_id desc limit 1",
        )
        .fetch_one(&pool)
        .await
        .context("query tron_rentals")?;
        assert_eq!(provider, "good", "expected solver to fall back to 'good'");

        let req = request_json.context("expected request_json to be persisted")?;
        let url = req["url"].as_str().unwrap_or("");
        assert!(
            url.contains("/rent"),
            "expected rendered request url to contain /rent, got {url:?}"
        );
        assert_eq!(req["method"].as_str().unwrap_or(""), "POST");
    }

    let _ = shutdown_tx.send(());
    Ok(())
}
