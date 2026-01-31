pub mod address;
pub mod grpc;
pub mod proof;
pub mod rental;
pub mod resources;
pub mod sender;
pub mod wallet;

pub use address::TronAddress;
pub use grpc::TronGrpc;
pub use proof::{TronTxProofBuilder, TronTxProofBundle};
pub use rental::{
    JsonApiRentalProvider, JsonApiRentalProviderConfig, RentalAttempt, RentalContext,
    RentalResourceKind,
};
pub use resources::{AccountResources, ChainFees, TxCostQuote};
pub use sender::{FeePolicy, SignedTronTx};
pub use wallet::{BroadcastedTronTx, TronWallet};

pub mod protocol {
    #![allow(clippy::all, clippy::pedantic, clippy::nursery)]
    #![allow(
        dead_code,
        unused_imports,
        unused_variables,
        non_snake_case,
        non_camel_case_types,
        non_upper_case_globals
    )]

    tonic::include_proto!("protocol");
}
