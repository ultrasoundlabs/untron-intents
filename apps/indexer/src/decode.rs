use crate::config::Stream;
use alloy::{
    primitives::{Address, B256, Bytes, U256},
    sol,
    sol_types::{Error as SolError, SolEventInterface},
};
use anyhow::Result;
use serde::Serialize;
use std::{borrow::Cow, fmt::Display};

#[derive(Debug, Clone)]
pub struct AsString<T>(pub T);

impl<T> From<T> for AsString<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<T: Display> Serialize for AsString<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(&self.0)
    }
}

#[derive(Debug, Clone)]
pub struct HexB256(pub B256);

impl Serialize for HexB256 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&format!("0x{}", hex::encode(self.0.as_slice())))
    }
}

#[derive(Debug, Clone)]
pub struct HexBytes(pub Bytes);

impl Serialize for HexBytes {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&format!("0x{}", hex::encode(self.0.as_ref())))
    }
}

type S<T> = AsString<T>;

macro_rules! sol_ty_to_rust {
    (address) => {
        S<Address>
    };
    (uint256) => {
        S<U256>
    };
    (uint64) => {
        S<u64>
    };
    (uint32) => {
        S<u32>
    };
    (uint8) => {
        S<u8>
    };
    (bytes32) => {
        HexB256
    };
    (bytes) => {
        HexBytes
    };
    (bool) => {
        bool
    };
}

macro_rules! stream_events {
    (
        $Iface:ident,
        decoded = $Decoded:path,
        $Out:ident,
        {
            $(
                $Variant:ident {
                    $( $field:ident : $solty:tt ),* $(,)?
                }
            ),* $(,)?
        }
    ) => {
        sol! {
            interface $Iface {
                $(
                    event $Variant( $( $solty $field ),* );
                )*
            }
        }

        #[derive(Debug, Clone, Serialize)]
        #[serde(tag = "event_type", content = "args")]
        pub enum $Out {
            $(
                $Variant {
                    $( $field: sol_ty_to_rust!($solty), )*
                },
            )*
            Unknown {
                #[serde(skip_serializing)]
                _signature: B256,
                #[serde(skip_serializing)]
                _data: Bytes,
            },
        }

        impl From<$Decoded> for $Out {
            fn from(decoded: $Decoded) -> Self {
                use $Decoded as Decoded;
                match decoded {
                    $(
                        Decoded::$Variant(ev) => $Out::$Variant {
                            $( $field: wrap_field!(ev.$field, $solty), )*
                        },
                    )*
                }
            }
        }

        impl StreamEvent for $Out {
            type Decoded = $Decoded;

            fn unknown(sig: B256, data: &Bytes) -> Self {
                Self::Unknown {
                    _signature: sig,
                    _data: data.clone(),
                }
            }
        }
    };
}

macro_rules! wrap_field {
    ($value:expr, bytes32) => {
        HexB256($value)
    };
    ($value:expr, bytes) => {
        HexBytes($value)
    };
    ($value:expr, bool) => {
        $value
    };
    ($value:expr, $other:tt) => {
        AsString($value)
    };
}

#[derive(Debug, Clone)]
pub enum SemanticEvent {
    Pool(PoolEvent),
    Forwarder(ForwarderEvent),
}

impl SemanticEvent {
    pub fn into_db_parts(self) -> (Cow<'static, str>, serde_json::Value) {
        match self {
            SemanticEvent::Pool(ev) => split_tagged(&ev),
            SemanticEvent::Forwarder(ev) => split_tagged(&ev),
        }
    }
}

fn split_tagged<T: Serialize>(value: &T) -> (Cow<'static, str>, serde_json::Value) {
    let mut v = serde_json::to_value(value).expect("serializable");
    let obj = v.as_object_mut().expect("tagged enum serializes to object");

    let event_type = obj
        .remove("event_type")
        .and_then(|v| v.as_str().map(|s| Cow::Owned(s.to_string())))
        .unwrap_or(Cow::Borrowed("Unknown"));

    let args = obj
        .remove("args")
        .unwrap_or_else(|| serde_json::Value::Object(Default::default()));

    (event_type, args)
}

// Our stored `abi_encoded_event_data` is `abi.encode(...)` of the full parameter list (including
// parameters that are `indexed` in the emitted Solidity events), so declare semantic events without
// `indexed` and decode from `data` only.
stream_events! {
    PoolStreamEvents,
    decoded = PoolStreamEvents::PoolStreamEventsEvents,
    PoolEvent,
    {
        OwnershipTransferred { old_owner: address, new_owner: address },
        RecommendedIntentFeeSet { fee_ppm: uint256, fee_flat: uint256 },
        ReceiverIntentParams {
            id: bytes32,
            forwarder: address,
            to_tron: address,
            forward_salt: bytes32,
            token: address,
            amount: uint256,
        },
        ReceiverIntentFeeSnap { id: bytes32, fee_ppm: uint256, fee_flat: uint256, tron_payment_amount: uint256 },
        IntentCreated {
            id: bytes32,
            creator: address,
            intent_type: uint8,
            token: address,
            amount: uint256,
            refund_beneficiary: address,
            deadline: uint256,
            intent_specs: bytes,
        },
        IntentClaimed { id: bytes32, solver: address, deposit_amount: uint256 },
        IntentUnclaimed {
            id: bytes32,
            caller: address,
            prev_solver: address,
            funded: bool,
            deposit_to_caller: uint256,
            deposit_to_refund_beneficiary: uint256,
            deposit_to_prev_solver: uint256,
        },
        IntentSolved { id: bytes32, solver: address, tron_tx_id: bytes32, tron_block_number: uint256 },
        IntentFunded { id: bytes32, funder: address, token: address, amount: uint256 },
        IntentSettled { id: bytes32, solver: address, escrow_token: address, escrow_amount: uint256, deposit_token: address, deposit_amount: uint256 },
        IntentClosed {
            id: bytes32,
            caller: address,
            solved: bool,
            funded: bool,
            settled: bool,
            refund_beneficiary: address,
            escrow_token: address,
            escrow_refunded: uint256,
            deposit_token: address,
            deposit_to_caller: uint256,
            deposit_to_refund_beneficiary: uint256,
            deposit_to_solver: uint256,
        },
    }
}

stream_events! {
    ForwarderStreamEvents,
    decoded = ForwarderStreamEvents::ForwarderStreamEventsEvents,
    ForwarderEvent,
    {
        OwnershipTransferred { old_owner: address, new_owner: address },
        BridgersSet { usdt_bridger: address, usdc_bridger: address },
        QuoterSet { token_in: address, quoter: address },
        ReceiverDeployed { receiver_salt: bytes32, receiver: address, implementation: address },
        ForwardStarted {
            forward_id: bytes32,
            base_receiver_salt: bytes32,
            forward_salt: bytes32,
            intent_hash: bytes32,
            target_chain: uint256,
            beneficiary: address,
            beneficiary_claim_only: bool,
            balance_param: uint256,
            token_in: address,
            token_out: address,
            receiver_used: address,
            ephemeral_receiver: address,
        },
        ForwardCompleted {
            forward_id: bytes32,
            ephemeral: bool,
            amount_pulled: uint256,
            amount_forwarded: uint256,
            relayer_rebate: uint256,
            msg_value_refunded: uint256,
            settled_locally: bool,
            bridger: address,
            expected_bridge_out: uint256,
            bridge_data_hash: bytes32,
        },
        SwapExecuted { forward_id: bytes32, token_in: address, token_out: address, min_out: uint256, actual_out: uint256 },
        BridgeInitiated { forward_id: bytes32, bridger: address, token_out: address, amount_in: uint256, target_chain: uint256 },
    }
}

pub fn decode_semantic_event(
    stream: Stream,
    event_signature: B256,
    abi_encoded_event_data: &Bytes,
) -> Result<SemanticEvent> {
    Ok(match stream {
        Stream::Pool => SemanticEvent::Pool(decode_event::<PoolEvent>(
            event_signature,
            abi_encoded_event_data,
        )?),
        Stream::Forwarder => SemanticEvent::Forwarder(decode_event::<ForwarderEvent>(
            event_signature,
            abi_encoded_event_data,
        )?),
    })
}

trait StreamEvent: Sized + From<Self::Decoded> {
    type Decoded: SolEventInterface;
    fn unknown(sig: B256, data: &Bytes) -> Self;
}

fn decode_event<E: StreamEvent>(event_signature: B256, data: &Bytes) -> Result<E> {
    match <E::Decoded as SolEventInterface>::decode_raw_log(&[event_signature], data.as_ref()) {
        Ok(ev) => Ok(E::from(ev)),
        Err(SolError::InvalidLog { .. }) => Ok(E::unknown(event_signature, data)),
        Err(e) => Err(anyhow::Error::new(e)),
    }
}
