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
use midnight_ledger::{
    serialize::deserialize, storage::DefaultDB, zswap::ledger::State as LedgerZswapState,
};
use std::{
    convert::Infallible,
    error::Error as StdError,
    hash::{DefaultHasher, Hash, Hasher},
    io,
};

pub type RawZswapState = ByteVec;

/// Abstraction for zswap state storage.
#[trait_variant::make(Send)]
pub trait ZswapStateStorage: Sync + 'static {
    type Error: StdError + Send + Sync + 'static;

    /// Load the last index.
    async fn load_last_index(&self) -> Result<Option<u64>, Self::Error>;

    /// Load the [RawZswapState] and the block height.
    async fn load_zswap_state(&self) -> Result<Option<(RawZswapState, u32)>, Self::Error>;

    /// Save the [RawZswapState].
    async fn save(
        &mut self,
        zswap_state: &RawZswapState,
        block_height: u32,
        last_index: Option<u64>,
    ) -> Result<(), Self::Error>;
}

pub struct NoopZswapStateStorage;

impl ZswapStateStorage for NoopZswapStateStorage {
    type Error = Infallible;

    async fn load_last_index(&self) -> Result<Option<u64>, Self::Error> {
        unimplemented!()
    }

    async fn load_zswap_state(&self) -> Result<Option<(RawZswapState, u32)>, Self::Error> {
        unimplemented!()
    }

    async fn save(
        &mut self,
        _zswap_state: &RawZswapState,
        _block_height: u32,
        _last_index: Option<u64>,
    ) -> Result<(), Self::Error> {
        unimplemented!()
    }
}

/// Wrapper around a ledger zswap state.
#[derive(Debug, Clone, Default, From, Deref)]
pub struct ZswapState(pub LedgerZswapState<DefaultDB>);

impl ZswapState {
    /// Deserialize the given raw zswap state using the given [NetworkId].
    pub fn deserialize(raw: RawZswapState, network_id: NetworkId) -> Result<Self, io::Error> {
        let state =
            deserialize::<LedgerZswapState<DefaultDB>, _>(&mut raw.as_ref(), network_id.into())?;
        Ok(Self(state))
    }
}

impl RawZswapState {
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.0.hash(&mut hasher);
        hasher.finish()
    }
}
