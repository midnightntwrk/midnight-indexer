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

//! # Guard rail
//!
//! Only ever deserialize in-memory states into [DefaultContractState]. Deserializing into
//! [LedgerDbContractState] would allocate nodes into the *shared* ledger arena, and those spill to
//! disk via storage-core's `flush_cache_evictions_to_db`, where only chain-indexer's `gc()` can
//! reclaim them — and in cloud deployments indexer-api shares that same database. This is enforced
//! by construction: [ContractState::deserialize] exists only for [DefaultContractState], and
//! states from the ledger arena are obtained with [LedgerDbContractState::load] instead.

use crate::{
    domain::{
        ContractBalance, ContractMaintenanceAuthority, ContractMaintenanceVerifyingKey,
        LedgerVersion, SerializedContractState, SerializedContractStateKey, TokenType,
        VerifyingKeyKind,
        ledger::{Error, TaggedSerializableExt},
    },
    infra::ledger_db::v1_1,
};
use fastrace::trace;
use midnight_coin_structure_v2::coin::TokenType as MidnightTokenType;
use midnight_coin_structure_v3::coin::TokenType as MidnightTokenTypeV9;
use midnight_onchain_runtime_v3::state::ContractState as ContractStateV3;
// v8's maintenance authority committee is `Vec<VerifyingKey>` (Schnorr only). v9 generalised it to
// a `ContractMaintenanceVerifyingKey` enum (Schnorr | ECDSA), re-exported by the v9 runtime.
use midnight_onchain_runtime_v4::state::{
    ContractMaintenanceVerifyingKey as ContractMaintenanceVerifyingKeyV4,
    ContractState as ContractStateV4,
};
use midnight_serialize_v1::{GLOBAL_TAG, Tagged, tagged_deserialize};
use midnight_storage_core_v1::{
    DefaultDB,
    arena::{ArenaHash, Sp, TypedArenaKey},
    db::DB,
    storage::default_storage,
};
use std::{
    fmt::{self, Debug},
    sync::LazyLock,
};

/// Hasher of the ledger DB, shared by every arena key this module deals with.
type Hasher = <v1_1::LedgerDb as DB>::Hasher;

/// A contract state deserialized from bytes into the in-memory arena. See the module-level guard
/// rail.
pub type DefaultContractState = ContractState<DefaultDB>;

/// A contract state resident in the persistent ledger arena.
pub type LedgerDbContractState = ContractState<v1_1::LedgerDb>;

/// Facade for `ContractState` from `midnight_ledger` across supported (protocol) versions.
///
/// Holds the arena pointer rather than the value: re-serializing through the pointer is
/// byte-identical to re-serializing the value — the derived `Serializable` of a `Storable` goes
/// through `Sp::new(self.clone())` — but avoids the clone and, more importantly, the
/// re-allocation into the arena that comes with it. Field reads go through `Deref`, which forces
/// only the root node; children stay lazy until touched.
///
/// The type parameter is not defaulted away in expression position, so use the
/// [DefaultContractState] and [LedgerDbContractState] aliases at call sites.
pub enum ContractState<D: DB = DefaultDB> {
    V3(Sp<ContractStateV3<D>, D>),
    V4(Sp<ContractStateV4<D>, D>),
}

// `Debug` and `Clone` are written out rather than derived: deriving would add `D: Debug` and
// `D: Clone` bounds, which the DB types do not satisfy.
impl<D: DB> Debug for ContractState<D> {
    /// Deliberately does not force the arena pointer — printing a contract state would otherwise
    /// fault in its whole DAG.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (variant, hash) = match self {
            Self::V3(state) => ("V3", state.hash()),
            Self::V4(state) => ("V4", state.hash()),
        };

        write!(
            f,
            "ContractState::{variant}({})",
            const_hex::encode(hash.0.as_slice())
        )
    }
}

impl<D: DB> Clone for ContractState<D> {
    fn clone(&self) -> Self {
        match self {
            Self::V3(state) => Self::V3(state.clone()),
            Self::V4(state) => Self::V4(state.clone()),
        }
    }
}

impl<D: DB> ContractState<D> {
    /// Tagged-serialize this contract state. Byte-identical to the DAG stored in the arena:
    /// storage-core walks the on-disk node payloads in deterministic topological order, without
    /// rehashing or recomputing annotations.
    #[trace]
    pub fn serialize(&self) -> Result<SerializedContractState, Error> {
        match self {
            Self::V3(state) => state
                .tagged_serialize()
                .map_err(|error| Error::Serialize("ContractStateV8", error)),
            Self::V4(state) => state
                .tagged_serialize()
                .map_err(|error| Error::Serialize("ContractStateV9", error)),
        }
    }

    /// Get the token balances for this contract.
    pub fn balances(&self) -> Result<Vec<ContractBalance>, Error> {
        match self {
            Self::V3(contract_state) => {
                contract_state
                    .balance
                    .iter()
                    .filter_map(|entry| {
                        // Read via deref: `Sp::into_inner` returns `None` for lazy or shared
                        // entries, silently dropping all balances.
                        let (token_type, amount) = &*entry;
                        let (token_type, amount) = (**token_type, **amount);

                        (amount > 0).then_some((token_type, amount))
                    })
                    .map(|(token_type, amount)| {
                        match token_type {
                            // For unshielded tokens extract the type directly.
                            MidnightTokenType::Unshielded(unshielded) => Ok(ContractBalance {
                                token_type: unshielded.0.0.into(),
                                amount,
                            }),

                            // For other tokens we serialize the type.
                            _ => {
                                let token_type = token_type
                                    .tagged_serialize()
                                    .map_err(|error| Error::Serialize("TokenTypeV8", error))?;

                                let token_type = TokenType::try_from(token_type.as_ref())
                                    .map_err(Error::ByteArrayLen)?;

                                Ok(ContractBalance { token_type, amount })
                            }
                        }
                    })
                    .collect()
            }

            Self::V4(contract_state) => {
                contract_state
                    .balance
                    .iter()
                    .filter_map(|entry| {
                        // Read via deref: `Sp::into_inner` returns `None` for lazy or shared
                        // entries, silently dropping all balances.
                        let (token_type, amount) = &*entry;
                        let (token_type, amount) = (**token_type, **amount);

                        (amount > 0).then_some((token_type, amount))
                    })
                    .map(|(token_type, amount)| {
                        match token_type {
                            // For unshielded tokens extract the type directly.
                            MidnightTokenTypeV9::Unshielded(unshielded) => Ok(ContractBalance {
                                token_type: unshielded.0.0.into(),
                                amount,
                            }),

                            // For other tokens we serialize the type.
                            _ => {
                                let token_type = token_type
                                    .tagged_serialize()
                                    .map_err(|error| Error::Serialize("TokenTypeV9", error))?;

                                let token_type = TokenType::try_from(token_type.as_ref())
                                    .map_err(Error::ByteArrayLen)?;

                                Ok(ContractBalance { token_type, amount })
                            }
                        }
                    })
                    .collect()
            }
        }
    }

    /// Get the maintenance authority for this contract.
    ///
    /// `ContractMaintenanceAuthority` is inline in the contract state's own node payload, so on a
    /// lazily loaded state this forces the root node alone.
    pub fn maintenance_authority(&self) -> Result<ContractMaintenanceAuthority, Error> {
        match self {
            Self::V3(contract_state) => {
                let authority = &contract_state.maintenance_authority;
                // v8 committee keys are all Schnorr (`Vec<VerifyingKey>`, no scheme tag).
                let committee = authority
                    .committee
                    .iter()
                    .map(|key| {
                        let key = key
                            .tagged_serialize()
                            .map_err(|error| Error::Serialize("VerifyingKeyV8", error))?;
                        Ok(ContractMaintenanceVerifyingKey {
                            kind: VerifyingKeyKind::Schnorr,
                            key,
                        })
                    })
                    .collect::<Result<Vec<_>, Error>>()?;

                Ok(ContractMaintenanceAuthority {
                    committee,
                    threshold: authority.threshold,
                    counter: authority.counter,
                })
            }

            Self::V4(contract_state) => {
                let authority = &contract_state.maintenance_authority;
                let committee = authority
                    .committee
                    .iter()
                    .map(|key| {
                        let (kind, key) = match key {
                            ContractMaintenanceVerifyingKeyV4::Schnorr(key) => {
                                (VerifyingKeyKind::Schnorr, key.tagged_serialize())
                            }
                            ContractMaintenanceVerifyingKeyV4::ECDSA(key) => {
                                (VerifyingKeyKind::Ecdsa, key.tagged_serialize())
                            }
                        };
                        let key = key.map_err(|error| Error::Serialize("VerifyingKeyV9", error))?;
                        Ok(ContractMaintenanceVerifyingKey { kind, key })
                    })
                    .collect::<Result<Vec<_>, Error>>()?;

                Ok(ContractMaintenanceAuthority {
                    committee,
                    threshold: authority.threshold,
                    counter: authority.counter,
                })
            }
        }
    }
}

impl DefaultContractState {
    /// Deserialize the given serialized contract state using the given protocol version, into the
    /// in-memory arena. See the module-level guard rail for why this is limited to [DefaultDB].
    #[trace(properties = { "ledger_version": "{ledger_version}" })]
    pub fn deserialize(
        contract_state: impl AsRef<[u8]>,
        ledger_version: LedgerVersion,
    ) -> Result<Self, Error> {
        let contract_state = match ledger_version {
            LedgerVersion::V8 => {
                let contract_state = tagged_deserialize(&mut contract_state.as_ref())
                    .map_err(|error| Error::Deserialize("ContractStateV8", error))?;
                Self::V3(contract_state)
            }
            LedgerVersion::V9 => {
                let contract_state = tagged_deserialize(&mut contract_state.as_ref())
                    .map_err(|error| Error::Deserialize("ContractStateV9", error))?;
                Self::V4(contract_state)
            }
        };

        Ok(contract_state)
    }
}

impl LedgerDbContractState {
    /// Load the contract state at the given arena key, faulting in nodes only as they are
    /// touched. Use this for reads of individual fields, e.g. the maintenance authority.
    ///
    /// The caller must have established that the key is loadable, e.g. via
    /// [crate::domain::ledger::LedgerState::contract_state_loadable]: a missing node panics
    /// inside storage-core rather than erroring.
    #[trace]
    pub fn load(key: &SerializedContractStateKey) -> Result<Self, Error> {
        Self::load_with_prefetch(key, false)
    }

    /// Load the contract state at the given arena key, breadth-first prefetching its whole DAG
    /// first. Use this before re-serializing the state, which touches every node: `pre_fetch`
    /// issues one batched query per DAG level, whereas faulting nodes in one at a time is one
    /// round trip per node.
    ///
    /// Note that `pre_fetch` truncates its walk at the configured `cache_max_nodes`, so a state
    /// larger than the cache is only partially prefetched.
    #[trace]
    pub fn load_prefetched(key: &SerializedContractStateKey) -> Result<Self, Error> {
        Self::load_with_prefetch(key, true)
    }

    fn load_with_prefetch(key: &SerializedContractStateKey, prefetch: bool) -> Result<Self, Error> {
        let storage = default_storage::<v1_1::LedgerDb>();

        match ContractStateArenaKey::deserialize(key)? {
            ContractStateArenaKey::V3(arena_key) => {
                if prefetch {
                    storage.with_backend(|b| b.pre_fetch(arena_key.key.hash(), None, true));
                }
                let contract_state = storage
                    .get_lazy(&arena_key)
                    .map_err(|error| Error::LoadContractState(key.to_owned(), error))?;

                Ok(Self::V3(contract_state))
            }

            ContractStateArenaKey::V4(arena_key) => {
                if prefetch {
                    storage.with_backend(|b| b.pre_fetch(arena_key.key.hash(), None, true));
                }
                let contract_state = storage
                    .get_lazy(&arena_key)
                    .map_err(|error| Error::LoadContractState(key.to_owned(), error))?;

                Ok(Self::V4(contract_state))
            }
        }
    }
}

/// The arena key of a contract state, discriminated by the ledger version it was written under.
///
/// The discrimination comes from the key's own tag: `Tagged for TypedArenaKey` yields
/// `storage-key(<inner tag>)`, and the two `ContractState` versions tag themselves
/// `contract-state[v6]` and `contract-state[v8]`. That matters because arena node payloads carry
/// no version tag and the two layouts are not compatible — `ContractMaintenanceAuthority` went
/// from `Vec<VerifyingKey>` to `Vec<ContractMaintenanceVerifyingKey>`, an enum with a discriminant
/// byte.
pub(super) enum ContractStateArenaKey {
    V3(TypedArenaKey<ContractStateV3<v1_1::LedgerDb>, Hasher>),
    V4(TypedArenaKey<ContractStateV4<v1_1::LedgerDb>, Hasher>),
}

impl ContractStateArenaKey {
    pub(super) fn deserialize(key: &SerializedContractStateKey) -> Result<Self, Error> {
        if key.starts_with(V3_TAG.as_bytes()) {
            let arena_key = tagged_deserialize(&mut key.as_ref())
                .map_err(|error| Error::Deserialize("ContractStateKeyV8", error))?;

            Ok(Self::V3(arena_key))
        } else if key.starts_with(V4_TAG.as_bytes()) {
            let arena_key = tagged_deserialize(&mut key.as_ref())
                .map_err(|error| Error::Deserialize("ContractStateKeyV9", error))?;

            Ok(Self::V4(arena_key))
        } else {
            Err(Error::UnknownContractStateKeyTag(key.to_owned()))
        }
    }

    pub(super) fn hash(&self) -> &ArenaHash<Hasher> {
        match self {
            Self::V3(arena_key) => arena_key.key.hash(),
            Self::V4(arena_key) => arena_key.key.hash(),
        }
    }
}

/// The tag prefix `tagged_serialize` writes ahead of a V8 contract state key.
static V3_TAG: LazyLock<String> =
    LazyLock::new(tag_prefix::<TypedArenaKey<ContractStateV3<v1_1::LedgerDb>, Hasher>>);

/// The tag prefix `tagged_serialize` writes ahead of a V9 contract state key.
static V4_TAG: LazyLock<String> =
    LazyLock::new(tag_prefix::<TypedArenaKey<ContractStateV4<v1_1::LedgerDb>, Hasher>>);

fn tag_prefix<T>() -> String
where
    T: Tagged,
{
    format!("{GLOBAL_TAG}{}:", T::tag())
}

#[cfg(test)]
mod tests {
    use crate::domain::{
        ByteArray, LedgerVersion, TokenType,
        ledger::{
            DefaultContractState, TaggedSerializableExt, contract_state::ContractStateArenaKey,
        },
    };
    use midnight_base_crypto_v1::hash::HashOutput;
    use midnight_coin_structure_v2::coin::{TokenType as MidnightTokenType, UnshieldedTokenType};
    use midnight_coin_structure_v3::coin::{
        TokenType as MidnightTokenTypeV9, UnshieldedTokenType as UnshieldedTokenTypeV9,
    };
    use midnight_onchain_runtime_v3::state::ContractState as ContractStateV3;
    use midnight_onchain_runtime_v4::state::ContractState as ContractStateV4;
    use midnight_storage_core_v1::DefaultDB;

    #[test]
    fn test_balances_v8() {
        let mut contract_state = ContractStateV3::<DefaultDB>::default();
        contract_state.balance = contract_state.balance.insert(
            MidnightTokenType::Unshielded(UnshieldedTokenType(HashOutput(TOKEN_TYPE.0))),
            AMOUNT,
        );
        let contract_state = contract_state
            .tagged_serialize()
            .expect("contract state can be serialized");

        let balances = DefaultContractState::deserialize(contract_state, LedgerVersion::V8)
            .expect("contract state can be deserialized")
            .balances()
            .expect("balances can be extracted");

        assert_eq!(balances.len(), 1);
        assert_eq!(balances[0].token_type, TOKEN_TYPE);
        assert_eq!(balances[0].amount, AMOUNT);
    }

    #[test]
    fn test_balances_v9() {
        let mut contract_state = ContractStateV4::<DefaultDB>::default();
        contract_state.balance = contract_state.balance.insert(
            MidnightTokenTypeV9::Unshielded(UnshieldedTokenTypeV9(HashOutput(TOKEN_TYPE.0))),
            AMOUNT,
        );
        let contract_state = contract_state
            .tagged_serialize()
            .expect("contract state can be serialized");

        let balances = DefaultContractState::deserialize(contract_state, LedgerVersion::V9)
            .expect("contract state can be deserialized")
            .balances()
            .expect("balances can be extracted");

        assert_eq!(balances.len(), 1);
        assert_eq!(balances[0].token_type, TOKEN_TYPE);
        assert_eq!(balances[0].amount, AMOUNT);
    }

    /// Round-tripping a state through the in-memory arena must reproduce the exact input bytes;
    /// this is the property the whole key-instead-of-blob change rests on, minus the persistence.
    #[test]
    fn serialize_round_trips_v8() {
        let mut contract_state = ContractStateV3::<DefaultDB>::default();
        contract_state.balance = contract_state.balance.insert(
            MidnightTokenType::Unshielded(UnshieldedTokenType(HashOutput(TOKEN_TYPE.0))),
            AMOUNT,
        );
        let expected = contract_state
            .tagged_serialize()
            .expect("contract state can be serialized");

        let serialized = DefaultContractState::deserialize(&expected, LedgerVersion::V8)
            .expect("contract state can be deserialized")
            .serialize()
            .expect("contract state can be serialized");

        assert_eq!(serialized, expected);
    }

    #[test]
    fn serialize_round_trips_v9() {
        let mut contract_state = ContractStateV4::<DefaultDB>::default();
        contract_state.balance = contract_state.balance.insert(
            MidnightTokenTypeV9::Unshielded(UnshieldedTokenTypeV9(HashOutput(TOKEN_TYPE.0))),
            AMOUNT,
        );
        let expected = contract_state
            .tagged_serialize()
            .expect("contract state can be serialized");

        let serialized = DefaultContractState::deserialize(&expected, LedgerVersion::V9)
            .expect("contract state can be deserialized")
            .serialize()
            .expect("contract state can be serialized");

        assert_eq!(serialized, expected);
    }

    /// The two contract state versions must be distinguishable from their keys alone, because the
    /// arena node payloads they point at carry no version tag.
    #[test]
    fn contract_state_key_tags_are_distinct() {
        let v3 = super::tag_prefix::<
            midnight_storage_core_v1::arena::TypedArenaKey<
                ContractStateV3<crate::infra::ledger_db::v1_1::LedgerDb>,
                super::Hasher,
            >,
        >();
        let v4 = super::tag_prefix::<
            midnight_storage_core_v1::arena::TypedArenaKey<
                ContractStateV4<crate::infra::ledger_db::v1_1::LedgerDb>,
                super::Hasher,
            >,
        >();

        assert_ne!(v3, v4);
        assert!(v3.contains("storage-key"));
        assert!(v4.contains("storage-key"));
    }

    /// An unrecognised tag must be refused rather than silently read as one of the known versions.
    #[test]
    fn unknown_contract_state_key_tag_is_rejected() {
        let key = b"midnight:storage-key(nonsense[v1]):".to_vec().into();
        assert!(ContractStateArenaKey::deserialize(&key).is_err());
    }

    const TOKEN_TYPE: TokenType = ByteArray([7; 32]);
    const AMOUNT: u128 = 1_000_000;
}
