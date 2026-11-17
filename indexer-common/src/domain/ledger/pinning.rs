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

//! Pinning: roots that are never unpersisted, such as contract states, are queued with [Pin::pin]
//! and pinned with everything they reach once flushed, so gc no longer marks from them and its
//! sweep skips what they reach.

use crate::{
    domain::ledger::LedgerState,
    infra::ledger_db::v1_1::{self, pinning},
};
use midnight_storage_core_v1::{
    arena::{ArenaHash, Sp},
    db::DB,
    storable::Storable,
};

/// A gc root that is never unpersisted, such as a contract state, so what it reaches is never
/// garbage: once it is pinned, gc's sweep skips it and gc no longer marks from it. gc still marks
/// whatever an unpinned root reaches, pinned or not.
pub(crate) trait Pin {
    /// Persist this root for good and queue it to pin, with everything it reaches: as they are
    /// flushed, or else by [LedgerState::pin_roots].
    fn pin(&mut self);
}

impl<T: Storable<v1_1::LedgerDb>> Pin for Sp<T, v1_1::LedgerDb> {
    fn pin(&mut self) {
        // Persist before taking the key, which turns an inlined node's key into a reference.
        self.persist();
        self.as_typed_key().key.hash().clone().pin();
    }
}

/// An already persisted root, such as one found on startup.
impl Pin for ArenaHash<<v1_1::LedgerDb as DB>::Hasher> {
    fn pin(&mut self) {
        pinning::queue(self.clone());
    }
}

/// What [LedgerState::pin] pinned, and the roots it left unpinned.
#[derive(Debug, Default)]
pub struct Pinned {
    /// Nodes pinned.
    pub nodes: usize,
    /// Roots left unpinned because a node they reach is missing.
    pub failed_roots: Vec<ArenaHash<<v1_1::LedgerDb as DB>::Hasher>>,
    /// Roots pinned as their nodes or counts were written, reading nothing.
    pub roots_pinned_on_write: usize,
    /// Roots pinned, or left unpinned, by walking the ledger DB.
    pub roots_walked: usize,
}

impl LedgerState {
    /// Pin everything reachable from the roots queued since the last call, so gc's sweep
    /// skips it and gc no longer marks from those roots. Call once they are flushed, i.e. after
    /// [Self::persist]. Returns the nodes pinned and the roots left unpinned.
    pub fn pin_roots() -> Pinned {
        Self::pin_queued()
    }

    /// Pin the given already persisted roots, and everything they reach. Reads the ledger DB only,
    /// loading nothing into the arena. Returns the nodes pinned and the roots left unpinned.
    pub fn pin(
        roots: impl IntoIterator<Item = ArenaHash<<v1_1::LedgerDb as DB>::Hasher>>,
    ) -> Pinned {
        roots.into_iter().for_each(|mut root| root.pin());
        Self::pin_queued()
    }

    /// Pin the roots queued and not pinned as their nodes were flushed.
    fn pin_queued() -> Pinned {
        pinning::pin_queued()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        domain::{
            LedgerVersion,
            ledger::{
                ContractZswapIndices, LedgerState, SerializableExt,
                ledger_state::tests::init_ledger_db,
            },
        },
        error::BoxError,
        infra::ledger_db::v1_1::pinning::{clear_queue, pinned},
    };
    use midnight_storage_core_v1::arena::ArenaHash;

    /// Pinning per block, as indexing does: getting a contract state or its contract zswap state
    /// key queues its root, and `pin_roots` pins it once the ledger state is persisted, with no
    /// root queued by hand. Covers V8 and V9, each contract owning a coin commitment of its own, so
    /// each contract zswap state has a root of its own. Their nodes are new, so they are pinned as
    /// written, without the walk, which in a debug build could not read them back: an inlined
    /// Merkle path nests 67 deep, past the 50 a debug build deserializes.
    #[cfg(any(feature = "cloud", feature = "standalone"))]
    #[tokio::test(flavor = "multi_thread")]
    async fn contract_states_pin_per_block() -> Result<(), BoxError> {
        use midnight_base_crypto_v1::hash::HashOutput;
        use midnight_coin_structure_v2::{
            coin::{TokenType as TokenTypeV8, UnshieldedTokenType as UnshieldedTokenTypeV8},
            contract::ContractAddress as ContractAddressV8,
        };
        use midnight_coin_structure_v3::{
            coin::{TokenType as TokenTypeV9, UnshieldedTokenType as UnshieldedTokenTypeV9},
            contract::ContractAddress as ContractAddressV9,
        };
        use midnight_onchain_runtime_v3::state::ContractState as ContractStateV3;
        use midnight_onchain_runtime_v4::state::ContractState as ContractStateV4;
        use midnight_storage_core_v1::arena::Sp;
        use midnight_transient_crypto_v2::curve::Fr as FrV8;
        use midnight_transient_crypto_v3::curve::Fr as FrV9;
        use std::ops::Deref;

        let _ledger_db = init_ledger_db().await?;

        let mut state = LedgerState::new("undeployed".try_into()?, LedgerVersion::V8)
            .expect("ledger state can be constructed");

        // A contract of its own per version, owning a coin commitment of its own, as a translated
        // state keeps its contracts' nodes.
        for (ledger_version, byte) in [(LedgerVersion::V8, 12), (LedgerVersion::V9, 13)] {
            state = state.translate(ledger_version)?;
            let hash = HashOutput([byte; 32]);
            let address = match &mut state {
                LedgerState::V8 { ledger_state, .. } => {
                    let address = ContractAddressV8(hash);
                    let mut contract_state = ContractStateV3::default();
                    contract_state.balance = contract_state.balance.insert(
                        TokenTypeV8::Unshielded(UnshieldedTokenTypeV8(hash)),
                        byte.into(),
                    );
                    ledger_state.contract = ledger_state.contract.insert(address, contract_state);
                    let mut zswap = ledger_state.zswap.deref().to_owned();
                    zswap.coin_coms = zswap
                        .coin_coms
                        .try_update(zswap.first_free, &FrV8::from(byte), Some(Sp::new(address)))
                        .expect("insert coin commitment");
                    zswap.first_free += 1;
                    ledger_state.zswap = Sp::new(zswap);
                    address.serialize()?
                }
                LedgerState::V9 { ledger_state, .. } => {
                    let address = ContractAddressV9(hash);
                    let mut contract_state = ContractStateV4::default();
                    contract_state.balance = contract_state.balance.insert(
                        TokenTypeV9::Unshielded(UnshieldedTokenTypeV9(hash)),
                        byte.into(),
                    );
                    ledger_state.contract = ledger_state.contract.insert(address, contract_state);
                    let mut zswap = ledger_state.zswap.deref().to_owned();
                    zswap.coin_coms = zswap
                        .coin_coms
                        .try_update(zswap.first_free, &FrV9::from(byte), Some(Sp::new(address)))
                        .expect("insert coin commitment");
                    zswap.first_free += 1;
                    ledger_state.zswap = Sp::new(zswap);
                    address.serialize()?
                }
            };

            let (key, _) = state
                .contract_state(&address)?
                .expect("the contract is in the ledger state");
            let zswap_key =
                state.contract_zswap_state_key(&address, &mut ContractZswapIndices::default())?;
            (state, _) = state.persist()?;

            let pinned_roots = LedgerState::pin_roots();
            assert!(pinned_roots.nodes > 0, "{ledger_version}: nodes pinned");
            assert_eq!(
                (
                    pinned_roots.roots_pinned_on_write,
                    pinned_roots.roots_walked
                ),
                (2, 0),
                "{ledger_version}: both roots pinned as written"
            );
            let contract_root = ArenaHash::try_from(&key)?;
            let zswap_root = ArenaHash::try_from(&zswap_key)?;
            assert_eq!(
                pinned(&contract_root),
                Some(true),
                "{ledger_version}: contract state pinned"
            );
            assert_eq!(
                pinned(&zswap_root),
                Some(true),
                "{ledger_version}: contract zswap state pinned"
            );
        }

        Ok(())
    }

    /// Pinning by stored keys, as the startup backfill does, pins a persisted contract state's
    /// node, and leaves the ledger state persisted with it unpinned: only roots that are never
    /// unpersisted get pinned. Clearing the queue of roots to pin relies on one process per test,
    /// as nextest runs them.
    #[cfg(any(feature = "cloud", feature = "standalone"))]
    #[tokio::test(flavor = "multi_thread")]
    async fn contract_states_pin_by_key() -> Result<(), BoxError> {
        use midnight_base_crypto_v1::hash::HashOutput;
        use midnight_coin_structure_v2::{
            coin::{TokenType as MidnightTokenType, UnshieldedTokenType},
            contract::ContractAddress as ContractAddressV8,
        };
        use midnight_onchain_runtime_v3::state::ContractState as ContractStateV3;

        let _ledger_db = init_ledger_db().await?;

        let address = ContractAddressV8(HashOutput([11; 32]));
        let mut state = LedgerState::new("undeployed".try_into()?, LedgerVersion::V8)
            .expect("ledger state can be constructed");
        match &mut state {
            LedgerState::V8 { ledger_state, .. } => {
                let mut contract_state = ContractStateV3::default();
                contract_state.balance = contract_state.balance.insert(
                    MidnightTokenType::Unshielded(UnshieldedTokenType(HashOutput([11; 32]))),
                    11,
                );
                ledger_state.contract = ledger_state.contract.insert(address, contract_state);
            }
            LedgerState::V9 { .. } => unreachable!("constructed as V8"),
        }

        let (key, _) = state
            .contract_state(&address.serialize()?)?
            .expect("the contract is in the ledger state");
        let zswap_key = state.contract_zswap_state_key(
            &address.serialize()?,
            &mut ContractZswapIndices::default(),
        )?;
        // Stored unpinned, as the history written before pinning existed.
        clear_queue();
        let (state, ledger_state_key) = state.persist()?;
        let ledger_state_root = LedgerState::arena_root_hash(&ledger_state_key, LedgerVersion::V8)?;
        drop(state);

        let contract_root = ArenaHash::try_from(&key)?;
        let zswap_root = ArenaHash::try_from(&zswap_key)?;
        assert_eq!(
            pinned(&contract_root),
            Some(false),
            "contract state stored unpinned"
        );
        assert_eq!(
            pinned(&zswap_root),
            Some(false),
            "contract zswap state stored unpinned"
        );
        LedgerState::pin([contract_root.clone()]);
        assert_eq!(pinned(&contract_root), Some(true), "contract state pinned");
        assert_eq!(
            pinned(&ledger_state_root),
            Some(false),
            "ledger state unpinned"
        );

        // Pinning again finds it pinned.
        LedgerState::pin([contract_root.clone()]);
        assert_eq!(pinned(&contract_root), Some(true));

        // A contract zswap state pins by its key the same way.
        let pinned_zswap = LedgerState::pin([zswap_root.clone()]);
        assert!(pinned_zswap.failed_roots.is_empty());
        assert_eq!(
            pinned(&zswap_root),
            Some(true),
            "contract zswap state pinned"
        );

        Ok(())
    }
}
