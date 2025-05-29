// This file is part of midnight-indexer.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::domain::{ByteVec, NetworkId};
use derive_more::derive::{Deref, From};
use midnight_ledger::{serialize::deserialize, storage::DefaultDB};
use std::{convert::Infallible, error::Error as StdError, io};

pub type RawLedgerState = ByteVec;

/// Abstraction for ledger state storage.
#[trait_variant::make(Send)]
pub trait LedgerStateStorage: Sync + 'static {
    type Error: StdError + Send + Sync + 'static;

    /// Load the last index.
    async fn load_last_index(&self) -> Result<Option<u64>, Self::Error>;

    /// Load the ledger state and the block height.
    async fn load_ledger_state(&self) -> Result<Option<(RawLedgerState, u32)>, Self::Error>;

    /// Save the given ledger state, block_height and last index.
    async fn save(
        &mut self,
        zswap_state: &RawLedgerState,
        block_height: u32,
        last_index: Option<u64>,
    ) -> Result<(), Self::Error>;
}

pub struct NoopLedgerStateStorage;

impl LedgerStateStorage for NoopLedgerStateStorage {
    type Error = Infallible;

    async fn load_last_index(&self) -> Result<Option<u64>, Self::Error> {
        unimplemented!()
    }

    async fn load_ledger_state(&self) -> Result<Option<(RawLedgerState, u32)>, Self::Error> {
        unimplemented!()
    }

    async fn save(
        &mut self,
        _zswap_state: &RawLedgerState,
        _block_height: u32,
        _last_index: Option<u64>,
    ) -> Result<(), Self::Error> {
        unimplemented!()
    }
}

/// New type for ledger state from midnight_ledger.
#[derive(Debug, Clone, Default, From, Deref)]
pub struct LedgerState(pub midnight_ledger::structure::LedgerState<DefaultDB>);

impl LedgerState {
    /// Deserialize the given raw zswap state using the given [NetworkId].
    pub fn deserialize(raw: RawLedgerState, network_id: NetworkId) -> Result<Self, io::Error> {
        let state = deserialize::<midnight_ledger::structure::LedgerState<DefaultDB>, _>(
            &mut raw.as_ref(),
            network_id.into(),
        )?;

        Ok(Self(state))
    }
}
