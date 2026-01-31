use anyhow::{Context, Result};
use tron::{TronAddress, TronGrpc, TronTxProofBuilder, TronWallet};

fn decode_hex32(s: &str) -> Result<[u8; 32]> {
    let s = s.trim();
    let s = s.strip_prefix("0x").unwrap_or(s);
    let bytes = hex::decode(s).context("invalid hex")?;
    if bytes.len() != 32 {
        anyhow::bail!("expected 32-byte hex, got {}", bytes.len());
    }
    Ok(bytes.try_into().unwrap())
}

fn main() -> Result<()> {
    let grpc_url = std::env::var("TRON_GRPC_URL").context("missing TRON_GRPC_URL")?;
    let pk = std::env::var("TRON_PRIVATE_KEY_HEX").context("missing TRON_PRIVATE_KEY_HEX")?;
    let to = std::env::var("TRON_TO").ok();
    let do_delegate = std::env::var("TRON_DO_DELEGATE").ok().is_some();

    let pk = decode_hex32(&pk)?;
    let wallet = TronWallet::new(pk).context("TronWallet::new")?;

    let rt = tokio::runtime::Runtime::new().context("tokio runtime")?;
    rt.block_on(async move {
        let mut grpc = TronGrpc::connect(&grpc_url, std::env::var("TRON_API_KEY").ok().as_deref())
            .await
            .context("TronGrpc::connect")?;

        let head = grpc.get_now_block2().await.context("get_now_block2")?;
        let head_num = head
            .block_header
            .as_ref()
            .and_then(|h| h.raw_data.as_ref())
            .map(|r| r.number)
            .unwrap_or_default();
        println!("head block: {head_num}");

        let addr = wallet.address();
        let acct = grpc
            .get_account(addr.prefixed_bytes().to_vec())
            .await
            .context("get_account")?;
        println!("wallet: {addr} balance_sun={}", acct.balance);

        let to_addr = match to {
            Some(s) => TronAddress::parse_text(&s).context("parse TRON_TO")?,
            None => addr,
        };

        println!("broadcasting transfer: to={to_addr} amount_sun=1");
        let txid = wallet
            .broadcast_transfer_contract(&mut grpc, to_addr, 1)
            .await
            .context("broadcast_transfer_contract")?;
        println!("txid=0x{}", hex::encode(txid));

        // Wait until tx_info has a block number, then check whether the txid is present in the block.
        let mut tx_block = 0i64;
        for _ in 0..60 {
            let info = grpc.get_transaction_info_by_id(txid).await?;
            if info.block_number > 0 {
                tx_block = info.block_number;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        println!("tx block_number={tx_block}");
        if tx_block > 0 {
            let b = grpc.get_block_by_num2(tx_block).await?;
            let found = b.transactions.iter().any(|txe| txe.txid.as_slice() == txid);
            println!(
                "block txs={} txid_in_block={found}",
                b.transactions.len()
            );
        }

        let builder = TronTxProofBuilder::new(19);
        let start = std::time::Instant::now();
        let proof = loop {
            match builder.build(&mut grpc, txid).await {
                Ok(p) => break p,
                Err(err) => {
                    if start.elapsed() > std::time::Duration::from_secs(240) {
                        return Err(err).context("build proof (timeout)");
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        };
        println!(
            "proof ok: encoded_tx_len={} proof_len={} index={}",
            proof.encoded_tx.len(),
            proof.proof.len(),
            proof.index
        );

        if do_delegate {
            let receiver = to_addr;
            println!("broadcasting delegate: receiver={receiver} balance_sun=1000000 resource=ENERGY lock_period=10");
            let txid = wallet
                .broadcast_delegate_resource_contract(
                    &mut grpc,
                    receiver,
                    tron::protocol::ResourceCode::Energy,
                    1_000_000,
                    true,
                    10,
                )
                .await
                .context("broadcast_delegate_resource_contract")?;
            println!("delegate txid=0x{}", hex::encode(txid));

            let start = std::time::Instant::now();
            let proof = loop {
                match builder.build(&mut grpc, txid).await {
                    Ok(p) => break p,
                    Err(err) => {
                        if start.elapsed() > std::time::Duration::from_secs(240) {
                            return Err(err).context("build delegate proof (timeout)");
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                }
            };
            println!(
                "delegate proof ok: encoded_tx_len={} proof_len={} index={}",
                proof.encoded_tx.len(),
                proof.proof.len(),
                proof.index
            );
        }

        Ok::<_, anyhow::Error>(())
    })?;

    Ok(())
}
