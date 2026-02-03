use super::protocol::{
    Account, AccountResourceMessage, BlockExtention, BytesMessage, ChainParameters,
    CreateSmartContract, DelegateResourceContract, EmptyMessage, EstimateEnergyMessage,
    FreezeBalanceV2Contract, NumberMessage, Return, Transaction, TransactionExtention,
    TransactionInfo, TransferContract, TriggerSmartContract, UnDelegateResourceContract,
    wallet_client::WalletClient,
};
use anyhow::{Context, Result};
use bytes::{Buf, BufMut, Bytes};
use prost::Message;
use std::marker::PhantomData;
use std::str::FromStr;
use tonic::{
    Request, Status,
    codec::{Codec, DecodeBuf, Decoder, EncodeBuf, Encoder},
    metadata::MetadataValue,
    transport::Channel,
};

#[derive(Clone)]
pub struct TronGrpc {
    api_key: Option<MetadataValue<tonic::metadata::Ascii>>,
    channel: Channel,
    wallet: WalletClient<Channel>,
}

impl TronGrpc {
    pub async fn connect(grpc_url: &str, api_key: Option<&str>) -> Result<Self> {
        let channel = Channel::from_shared(grpc_url.to_string())
            .context("invalid TRON_GRPC_URL")?
            .connect()
            .await
            .context("connect TRON gRPC")?;

        let api_key = match api_key {
            Some(k) if !k.trim().is_empty() => {
                Some(MetadataValue::from_str(k).context("invalid TRON_API_KEY (metadata value)")?)
            }
            _ => None,
        };

        Ok(Self {
            api_key,
            channel: channel.clone(),
            wallet: WalletClient::new(channel),
        })
    }

    fn req<T>(&self, msg: T) -> Request<T> {
        let mut req = Request::new(msg);
        if let Some(key) = &self.api_key {
            req.metadata_mut().insert("tron-pro-api-key", key.clone());
        }
        req
    }

    pub async fn get_now_block2(&mut self) -> Result<BlockExtention> {
        let resp = self
            .wallet
            .get_now_block2(self.req(EmptyMessage {}))
            .await
            .context("GetNowBlock2")?;
        Ok(resp.into_inner())
    }

    pub async fn get_block_by_num2(&mut self, num: i64) -> Result<BlockExtention> {
        let resp = self
            .wallet
            .get_block_by_num2(self.req(NumberMessage { num }))
            .await
            .context("GetBlockByNum2")?;
        Ok(resp.into_inner())
    }

    /// Fetches a block as raw protobuf bytes and extracts the canonical, nested `Transaction`
    /// bytes for each `TransactionExtention` without re-encoding (preserves unknown fields and
    /// wire layout relevant for `txTrieRoot`).
    pub async fn get_block_by_num2_raw_bytes(&mut self, num: i64) -> Result<Bytes> {
        let mut grpc = tonic::client::Grpc::new(self.channel.clone());
        grpc.ready().await.context("TRON gRPC not ready")?;

        let path =
            tonic::codegen::http::uri::PathAndQuery::from_static("/protocol.Wallet/GetBlockByNum2");
        let codec = ProstRequestBytesResponseCodec::<NumberMessage>::default();

        let resp = grpc
            .unary(self.req(NumberMessage { num }), path, codec)
            .await
            .context("GetBlockByNum2 (raw)")?;

        Ok(resp.into_inner())
    }

    /// Fetches a block as raw protobuf bytes and extracts the canonical, nested `Transaction`
    /// bytes for each `TransactionExtention` without re-encoding (preserves unknown fields and
    /// wire layout relevant for `txTrieRoot`).
    pub async fn get_block_by_num2_raw_txs(
        &mut self,
        num: i64,
    ) -> Result<(BlockExtention, Vec<Vec<u8>>)> {
        let bytes = self.get_block_by_num2_raw_bytes(num).await?;
        let block = BlockExtention::decode(bytes.clone()).context("decode BlockExtention")?;
        let raw_txs = extract_block_extention_transaction_bytes(bytes.as_ref())
            .context("extract tx bytes from BlockExtention")?;

        if raw_txs.len() != block.transactions.len() {
            anyhow::bail!(
                "BlockExtention tx count mismatch: parsed_raw={} decoded_struct={}",
                raw_txs.len(),
                block.transactions.len()
            );
        }

        Ok((block, raw_txs))
    }

    pub async fn get_transaction_info_by_id(&mut self, txid: [u8; 32]) -> Result<TransactionInfo> {
        let resp = self
            .wallet
            .get_transaction_info_by_id(self.req(BytesMessage {
                value: txid.to_vec(),
            }))
            .await
            .context("GetTransactionInfoById")?;
        Ok(resp.into_inner())
    }

    pub async fn trigger_contract(
        &mut self,
        msg: TriggerSmartContract,
    ) -> Result<TransactionExtention> {
        let resp = self
            .wallet
            .trigger_contract(self.req(msg))
            .await
            .context("TriggerContract")?;
        Ok(resp.into_inner())
    }

    pub async fn trigger_constant_contract(
        &mut self,
        msg: TriggerSmartContract,
    ) -> Result<TransactionExtention> {
        let resp = self
            .wallet
            .trigger_constant_contract(self.req(msg))
            .await
            .context("TriggerConstantContract")?;
        Ok(resp.into_inner())
    }

    pub async fn deploy_contract(
        &mut self,
        msg: CreateSmartContract,
    ) -> Result<TransactionExtention> {
        let resp = self
            .wallet
            .deploy_contract(self.req(msg))
            .await
            .context("DeployContract")?;
        Ok(resp.into_inner())
    }

    /// Creates a native TRX transfer transaction skeleton.
    ///
    /// Notes:
    /// - Some private networks (e.g. tronbox/tre) may not implement `CreateTransaction2`,
    ///   so we use the legacy `CreateTransaction` here.
    pub async fn create_transfer_transaction(
        &mut self,
        msg: TransferContract,
    ) -> Result<Transaction> {
        let resp = self
            .wallet
            .create_transaction(self.req(msg))
            .await
            .context("CreateTransaction")?;
        Ok(resp.into_inner())
    }

    /// Creates a DelegateResource transaction skeleton.
    ///
    /// This calls `Wallet/DelegateResource` (DelegateResourceContract -> TransactionExtention).
    pub async fn delegate_resource(
        &mut self,
        msg: DelegateResourceContract,
    ) -> Result<TransactionExtention> {
        let resp = self
            .wallet
            .delegate_resource(self.req(msg))
            .await
            .context("DelegateResource")?;
        Ok(resp.into_inner())
    }

    /// Creates an UnDelegateResource transaction skeleton.
    ///
    /// This calls `Wallet/UnDelegateResource` (UnDelegateResourceContract -> TransactionExtention).
    pub async fn undelegate_resource(
        &mut self,
        msg: UnDelegateResourceContract,
    ) -> Result<TransactionExtention> {
        let resp = self
            .wallet
            .un_delegate_resource(self.req(msg))
            .await
            .context("UnDelegateResource")?;
        Ok(resp.into_inner())
    }

    /// Creates a FreezeBalanceV2 transaction skeleton.
    pub async fn freeze_balance_v2(
        &mut self,
        msg: FreezeBalanceV2Contract,
    ) -> Result<TransactionExtention> {
        let resp = self
            .wallet
            .freeze_balance_v2(self.req(msg))
            .await
            .context("FreezeBalanceV2")?;
        Ok(resp.into_inner())
    }

    pub async fn broadcast_transaction(&mut self, tx: Transaction) -> Result<Return> {
        let resp = self
            .wallet
            .broadcast_transaction(self.req(tx))
            .await
            .context("BroadcastTransaction")?;
        Ok(resp.into_inner())
    }

    pub async fn get_account(&mut self, address_prefixed: Vec<u8>) -> Result<Account> {
        let resp = self
            .wallet
            .get_account(self.req(Account {
                address: address_prefixed,
                ..Default::default()
            }))
            .await
            .context("GetAccount")?;
        Ok(resp.into_inner())
    }

    pub async fn get_account_resource(
        &mut self,
        address_prefixed: Vec<u8>,
    ) -> Result<AccountResourceMessage> {
        let resp = self
            .wallet
            .get_account_resource(self.req(Account {
                address: address_prefixed,
                ..Default::default()
            }))
            .await
            .context("GetAccountResource")?;
        Ok(resp.into_inner())
    }

    pub async fn get_chain_parameters(&mut self) -> Result<ChainParameters> {
        let resp = self
            .wallet
            .get_chain_parameters(self.req(EmptyMessage {}))
            .await
            .context("GetChainParameters")?;
        Ok(resp.into_inner())
    }

    pub async fn estimate_energy(
        &mut self,
        msg: TriggerSmartContract,
    ) -> Result<EstimateEnergyMessage> {
        let resp = self
            .wallet
            .estimate_energy(self.req(msg))
            .await
            .context("EstimateEnergy")?;
        Ok(resp.into_inner())
    }
}

#[derive(Clone, Default)]
struct ProstRequestBytesResponseCodec<T>(PhantomData<T>);

impl<T> Codec for ProstRequestBytesResponseCodec<T>
where
    T: Message + Send + 'static,
{
    type Encode = T;
    type Decode = Bytes;
    type Encoder = ProstMessageEncoder<T>;
    type Decoder = RawBytesDecoder;

    fn encoder(&mut self) -> Self::Encoder {
        ProstMessageEncoder(PhantomData)
    }

    fn decoder(&mut self) -> Self::Decoder {
        RawBytesDecoder
    }
}

#[derive(Clone, Default)]
struct ProstMessageEncoder<T>(PhantomData<T>);

impl<T> Encoder for ProstMessageEncoder<T>
where
    T: Message,
{
    type Item = T;
    type Error = Status;

    fn encode(
        &mut self,
        item: Self::Item,
        dst: &mut EncodeBuf<'_>,
    ) -> std::result::Result<(), Self::Error> {
        let mut buf = Vec::with_capacity(item.encoded_len());
        item.encode(&mut buf)
            .map_err(|e| Status::internal(format!("prost encode failed: {e}")))?;
        dst.put_slice(&buf);
        Ok(())
    }
}

#[derive(Clone, Default)]
struct RawBytesDecoder;

impl Decoder for RawBytesDecoder {
    type Item = Bytes;
    type Error = Status;

    fn decode(
        &mut self,
        src: &mut DecodeBuf<'_>,
    ) -> std::result::Result<Option<Self::Item>, Self::Error> {
        if src.remaining() == 0 {
            return Ok(None);
        }
        Ok(Some(src.copy_to_bytes(src.remaining())))
    }
}

pub(crate) fn extract_block_extention_transaction_bytes(buf: &[u8]) -> Result<Vec<Vec<u8>>> {
    let mut transactions: Vec<Vec<u8>> = Vec::new();
    let mut cursor = buf;

    while !cursor.is_empty() {
        let (key, rest) = decode_varint(cursor).context("decode field key")?;
        cursor = rest;

        let field_number = (key >> 3) as u32;
        let wire_type = (key & 0x07) as u8;

        match wire_type {
            0 => {
                let (_, rest) = decode_varint(cursor).context("skip varint")?;
                cursor = rest;
            }
            1 => cursor = cursor.get(8..).context("skip fixed64")?,
            2 => {
                let (len, rest) = decode_varint(cursor).context("decode len")?;
                let len = usize::try_from(len).context("len overflow")?;
                let (chunk, after) = rest.split_at(len);
                cursor = after;

                if field_number == 1 {
                    let tx = extract_transaction_bytes_from_transaction_extention(chunk)
                        .context("extract TransactionExtention.transaction bytes")?;
                    transactions.push(tx);
                }
            }
            5 => cursor = cursor.get(4..).context("skip fixed32")?,
            _ => anyhow::bail!("unsupported wire type {wire_type} in BlockExtention"),
        }
    }

    Ok(transactions)
}

pub(crate) fn extract_transaction_bytes_from_transaction_extention(buf: &[u8]) -> Result<Vec<u8>> {
    let mut cursor = buf;
    while !cursor.is_empty() {
        let (key, rest) = decode_varint(cursor).context("decode field key")?;
        cursor = rest;

        let field_number = (key >> 3) as u32;
        let wire_type = (key & 0x07) as u8;

        match wire_type {
            0 => {
                let (_, rest) = decode_varint(cursor).context("skip varint")?;
                cursor = rest;
            }
            1 => cursor = cursor.get(8..).context("skip fixed64")?,
            2 => {
                let (len, rest) = decode_varint(cursor).context("decode len")?;
                let len = usize::try_from(len).context("len overflow")?;
                let (chunk, after) = rest.split_at(len);
                cursor = after;

                if field_number == 1 {
                    return Ok(chunk.to_vec());
                }
            }
            5 => cursor = cursor.get(4..).context("skip fixed32")?,
            _ => anyhow::bail!("unsupported wire type {wire_type} in TransactionExtention"),
        }
    }

    anyhow::bail!("missing TransactionExtention.transaction field")
}

fn decode_varint(buf: &[u8]) -> Result<(u64, &[u8])> {
    let mut out: u64 = 0;
    let mut shift: u32 = 0;
    let mut idx: usize = 0;

    while idx < buf.len() {
        let byte = buf[idx];
        let low = u64::from(byte & 0x7f);
        if shift >= 64 {
            anyhow::bail!("varint overflow");
        }
        out |= low << shift;
        idx += 1;
        if (byte & 0x80) == 0 {
            return Ok((out, &buf[idx..]));
        }
        shift += 7;
    }

    anyhow::bail!("unexpected EOF decoding varint");
}

#[cfg(test)]
mod tests {
    use super::{decode_varint, extract_block_extention_transaction_bytes};

    fn encode_varint(mut value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                break;
            }
            out.push(byte | 0x80);
        }
        out
    }

    #[test]
    fn decode_varint_roundtrips_small_values() {
        for v in [0_u64, 1, 2, 10, 127, 128, 300, 16384, 1_000_000] {
            let enc = encode_varint(v);
            let (dec, rest) = decode_varint(&enc).expect("decode");
            assert_eq!(dec, v);
            assert!(rest.is_empty());
        }
    }

    #[test]
    fn extract_tx_bytes_from_block_extention_without_reencoding() {
        // Canonical tx bytes include a Transaction.Result with an extra, high-tag field (1002).
        let tx_bytes =
            hex::decode("0a93010a028b58220806e328e403f663c74098dfcfeeb9335a750839126f0a35747970652e676f6f676c65617069732e636f6d2f70726f746f636f6c2e44656c65676174655265736f75726365436f6e747261637412360a15416651c0e2ff4bb6306dedee3432178bc3440f6dd7100118c0ccf3c7332215414656aefbd8de0f2b0fa3fb7a4724079add283ce42803708790cceeb9331241f04bc4c12a3323d37ad1a57ab6eab4e0e258dbf34733d73ea7fec3d1a49e98f632d848c19fa36b4b0cf2de320f6f5adcd90696fb747c47b0a8d23363b6f6de50012a021801d23e0cc83e81f6a50cd03ed4b6a20f").expect("tx hex");

        // TransactionExtention:
        // - field 1 (transaction): bytes
        let mut txe = Vec::new();
        txe.push(0x0a); // (1<<3)|2
        txe.extend_from_slice(&encode_varint(tx_bytes.len() as u64));
        txe.extend_from_slice(&tx_bytes);

        // BlockExtention:
        // - field 1 (transactions): repeated TransactionExtention
        let mut block = Vec::new();
        block.push(0x0a); // (1<<3)|2
        block.extend_from_slice(&encode_varint(txe.len() as u64));
        block.extend_from_slice(&txe);

        let extracted =
            extract_block_extention_transaction_bytes(&block).expect("extract tx bytes");
        assert_eq!(extracted.len(), 1);
        assert_eq!(extracted[0], tx_bytes);
    }
}
