// This file is part of midnight-indexer.
// Copyright (C) Midnight Foundation
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

use crate::{
    domain::{
        SerializedZswapState, SerializedZswapStateKey,
        ledger::{Error, TaggedSerializableExt},
    },
    infra::ledger_db::v1_1,
};
use fastrace::trace;
use midnight_serialize_v1::tagged_deserialize;
use midnight_storage_core_v1::{
    arena::{Sp, TypedArenaKey},
    db::DB,
    storage::default_storage,
};
use midnight_zswap_v9::ledger::State as ZswapStateV9;

/// Hasher of the ledger DB.
type Hasher = <v1_1::LedgerDb as DB>::Hasher;

/// A contract's filtered zswap state, resident in the ledger arena.
///
/// Unlike [super::ContractState] this is not version-dispatched, because there is nothing to
/// dispatch on and nothing to gain: zswap's `State` tags itself `zswap-ledger-state[v5]` in both
/// ledger v8 and v9, so a key written under either version deserializes as either type and the
/// bytes handed back to clients are the same in both cases — exactly as today, where the stored
/// blob carries that one tag regardless of the block's protocol version.
///
/// This is sound because the value is never forced: `get_lazy` hands back a lazy pointer without
/// reading the node, and re-serializing walks the stored node payloads at the byte level. The type
/// parameter therefore only supplies the tag and the `Storable` bound.
pub struct ContractZswapState(Sp<ZswapStateV9<v1_1::LedgerDb>, v1_1::LedgerDb>);

impl ContractZswapState {
    /// Load the zswap state at the given arena key, breadth-first prefetching its whole DAG:
    /// re-serializing touches every node, and `pre_fetch` turns that from one round trip per node
    /// into one batched query per DAG level.
    ///
    /// The caller must have established that the key is loadable, e.g. via
    /// [super::LedgerState::contract_zswap_state_loadable]: a missing node panics inside
    /// storage-core rather than erroring.
    #[trace]
    pub fn load_prefetched(key: &SerializedZswapStateKey) -> Result<Self, Error> {
        let arena_key = Self::arena_key(key)?;
        let storage = default_storage::<v1_1::LedgerDb>();

        storage.with_backend(|b| b.pre_fetch(arena_key.key.hash(), None, true));
        let zswap_state = storage
            .get_lazy(&arena_key)
            .map_err(|error| Error::LoadContractZswapState(key.to_owned(), error))?;

        Ok(Self(zswap_state))
    }

    /// Tagged-serialize this zswap state, byte-identical to the DAG stored in the arena.
    #[trace]
    pub fn serialize(&self) -> Result<SerializedZswapState, Error> {
        self.0
            .tagged_serialize()
            .map_err(|error| Error::Serialize("ZswapState", error))
    }

    pub(super) fn arena_key(
        key: &SerializedZswapStateKey,
    ) -> Result<TypedArenaKey<ZswapStateV9<v1_1::LedgerDb>, Hasher>, Error> {
        tagged_deserialize(&mut key.as_ref())
            .map_err(|error| Error::Deserialize("ZswapStateKey", error))
    }
}
