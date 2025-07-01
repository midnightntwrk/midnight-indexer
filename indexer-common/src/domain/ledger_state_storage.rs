use crate::domain::{ProtocolVersion, RawLedgerState};
use std::{convert::Infallible, error::Error as StdError};

/// Abstraction for ledger state storage.
#[trait_variant::make(Send)]
pub trait LedgerStateStorage: Sync + 'static {
    type Error: StdError + Send + Sync + 'static;

    /// Load the last index.
    async fn load_last_index(&self) -> Result<Option<u64>, Self::Error>;

    /// Load the ledger state, block height and protocol version.
    async fn load_ledger_state(
        &self,
    ) -> Result<Option<(RawLedgerState, u32, ProtocolVersion)>, Self::Error>;

    /// Save the given ledger state, block_height and highest zswap state index.
    async fn save(
        &mut self,
        ledger_state: &RawLedgerState,
        block_height: u32,
        highest_zswap_state_index: Option<u64>,
        protocol_version: ProtocolVersion,
    ) -> Result<(), Self::Error>;
}

pub struct NoopLedgerStateStorage;

impl LedgerStateStorage for NoopLedgerStateStorage {
    type Error = Infallible;

    async fn load_last_index(&self) -> Result<Option<u64>, Self::Error> {
        unimplemented!()
    }

    async fn load_ledger_state(
        &self,
    ) -> Result<Option<(RawLedgerState, u32, ProtocolVersion)>, Self::Error> {
        unimplemented!()
    }

    #[allow(unused_variables)]
    async fn save(
        &mut self,
        ledger_state: &RawLedgerState,
        block_height: u32,
        highest_zswap_state_index: Option<u64>,
        protocol_version: ProtocolVersion,
    ) -> Result<(), Self::Error> {
        unimplemented!()
    }
}
