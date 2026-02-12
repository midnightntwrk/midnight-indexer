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

use crate::{
    domain::{
        ApplyRegularTransactionOutcome, ApplySystemTransactionOutcome, ByteArray, ByteVec,
        IntentHash, LedgerEvent, LedgerVersion, NetworkId, Nonce, ProtocolVersion,
        SerializedContractAddress, SerializedLedgerParameters, SerializedLedgerStateKey,
        SerializedTransaction, SerializedZswapState, SerializedZswapStateRoot, TokenType,
        TransactionResult, UnshieldedUtxo,
        dust::{self},
        ledger::{
            Error, IntentV7, IntentV8, SerializableExt, TaggedSerializableExt, TransactionV7,
            TransactionV8,
        },
    },
    infra::ledger_db::LedgerDb,
};
use fastrace::trace;
use itertools::Itertools;
use log::info;
use midnight_base_crypto::{
    cost_model::{FixedPoint, NormalizedCost, SyntheticCost},
    hash::{HashOutput, persistent_commit},
    time::Timestamp,
};
use midnight_coin_structure_v7::{
    coin::{
        NIGHT as NIGHTV7, UnshieldedTokenType as UnshieldedTokenTypeV7,
        UserAddress as UserAddressV7,
    },
    contract::ContractAddress as ContractAddressV7,
};
use midnight_coin_structure_v8::{
    coin::{
        NIGHT as NIGHTV8, TokenType as TokenTypeV8, UnshieldedTokenType as UnshieldedTokenTypeV8,
        UserAddress as UserAddressV8,
    },
    contract::ContractAddress as ContractAddressV8,
};
use midnight_ledger_v7::{
    dust::{
        DustGenerationInfo as DustGenerationInfoV7, InitialNonce as InitialNonceV7,
        QualifiedDustOutput as QualifiedDustOutputV7,
    },
    events::{Event as EventV7, EventDetails as EventDetailsV7},
    semantics::{
        TransactionContext as TransactionContextV7, TransactionResult as TransactionResultV7,
    },
    structure::{
        LedgerParameters as LedgerParametersV7, LedgerState as LedgerStateV7,
        OutputInstructionUnshielded as OutputInstructionUnshieldedV7,
        SystemTransaction as SystemTransactionV7, Utxo as UtxoV7,
    },
    verify::WellFormedStrictness as WellFormedStrictnessV7,
};
use midnight_ledger_v8::{
    dust::{
        DustGenerationInfo as DustGenerationInfoV8, InitialNonce as InitialNonceV8,
        QualifiedDustOutput as QualifiedDustOutputV8,
    },
    events::{Event as EventV8, EventDetails as EventDetailsV8},
    semantics::{
        TransactionContext as TransactionContextV8, TransactionResult as TransactionResultV8,
    },
    structure::{
        INITIAL_PARAMETERS as INITIAL_PARAMETERS_V8, LedgerParameters as LedgerParametersV8,
        LedgerState as LedgerStateV8, OutputInstructionUnshielded as OutputInstructionUnshieldedV8,
        SystemTransaction as SystemTransactionV8, Utxo as UtxoV8,
    },
    verify::WellFormedStrictness as WellFormedStrictnessV8,
};
use midnight_onchain_runtime_v7::context::BlockContext as BlockContextV7;
use midnight_onchain_runtime_v8::context::BlockContext as BlockContextV8;
use midnight_serialize::{Deserializable, tagged_deserialize};
use midnight_storage_core::{
    arena::{ArenaKey, Sp, TypedArenaKey},
    db::DB,
    storage::default_storage,
};
use midnight_transient_crypto_v7::merkle_tree::{
    MerkleTreeCollapsedUpdate as MerkleTreeCollapsedUpdateV7,
    MerkleTreeDigest as MerkleTreeDigestV7, TreeInsertionPath as TreeInsertionPathV7,
};
use midnight_transient_crypto_v8::merkle_tree::{
    MerkleTreeCollapsedUpdate as MerkleTreeCollapsedUpdateV8,
    MerkleTreeDigest as MerkleTreeDigestV8, TreeInsertionPath as TreeInsertionPathV8,
};
use midnight_zswap_v7::ledger::State as ZswapStateV7;
use midnight_zswap_v8::ledger::State as ZswapStateV8;
use std::{collections::HashSet, io, ops::Deref, sync::LazyLock};

const OUTPUT_INDEX_ZERO: u32 = 0;

static STRICTNESS_V7: LazyLock<WellFormedStrictnessV7> = LazyLock::new(|| {
    let mut strictness = WellFormedStrictnessV7::default();
    strictness.enforce_balancing = false;
    strictness
});

static STRICTNESS_V8: LazyLock<WellFormedStrictnessV8> = LazyLock::new(|| {
    let mut strictness = WellFormedStrictnessV8::default();
    strictness.enforce_balancing = false;
    strictness
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerState {
    V7 {
        ledger_state: LedgerStateV7<LedgerDb>,
        block_fullness: SyntheticCost,
    },

    V8 {
        ledger_state: LedgerStateV8<LedgerDb>,
        block_fullness: SyntheticCost,
    },
}

impl LedgerState {
    #[allow(missing_docs)]
    pub fn new(network_id: NetworkId, protocol_version: ProtocolVersion) -> Result<Self, Error> {
        let ledger_state = match protocol_version.ledger_version()? {
            LedgerVersion::V7 => Self::V7 {
                ledger_state: LedgerStateV7::new(network_id),
                block_fullness: Default::default(),
            },

            LedgerVersion::V8 => Self::V8 {
                ledger_state: LedgerStateV8::new(network_id),
                block_fullness: Default::default(),
            },
        };

        Ok(ledger_state)
    }

    /// Create a [LedgerState] by deserializing the genesis state from chain spec. The
    /// deserialized state already includes block 0 transactions, so block 0 transaction
    /// application must be skipped.
    pub fn from_genesis(raw: &[u8], protocol_version: ProtocolVersion) -> Result<Self, Error> {
        match protocol_version.ledger_version()? {
            LedgerVersion::V7 => Err(Error::Deserialize(
                "GenesisLedgerStateV7",
                io::Error::new(
                    io::ErrorKind::Unsupported,
                    "genesis state from chain spec is not supported for V7",
                ),
            )),

            LedgerVersion::V8 => {
                let ledger_state = tagged_deserialize::<LedgerStateV8<LedgerDb>>(&mut &*raw)
                    .map_err(|error| Error::Deserialize("GenesisLedgerStateV8", error))?;

                let treasury_night = ledger_state
                    .treasury
                    .get(&TokenTypeV8::Unshielded(NIGHTV8))
                    .copied()
                    .unwrap_or(0);

                info!(
                    locked_pool = ledger_state.locked_pool,
                    reserve_pool = ledger_state.reserve_pool,
                    treasury_night;
                    "genesis ledger state deserialized from chain spec"
                );

                Ok(Self::V8 {
                    ledger_state,
                    block_fullness: Default::default(),
                })
            }
        }
    }

    /// Create a [LedgerState] with genesis pool settings. Unlike
    /// [`from_genesis`](Self::from_genesis), this creates a pre-block-0 state, so block 0
    /// transactions must be applied normally.
    pub fn with_genesis_settings(
        network_id: NetworkId,
        protocol_version: ProtocolVersion,
        locked_pool: u128,
        reserve_pool: u128,
        treasury: u128,
    ) -> Result<Self, Error> {
        match protocol_version.ledger_version()? {
            LedgerVersion::V7 => Err(Error::Deserialize(
                "GenesisSettingsV7",
                io::Error::new(
                    io::ErrorKind::Unsupported,
                    "genesis settings are not supported for V7",
                ),
            )),

            LedgerVersion::V8 => {
                let ledger_state = LedgerStateV8::with_genesis_settings(
                    network_id.to_string(),
                    INITIAL_PARAMETERS_V8,
                    locked_pool,
                    reserve_pool,
                    treasury,
                )
                .map_err(|error| Error::GenesisSettings(error.to_string().into()))?;

                info!(
                    locked_pool,
                    reserve_pool,
                    treasury;
                    "genesis ledger state created with genesis settings"
                );

                Ok(Self::V8 {
                    ledger_state,
                    block_fullness: Default::default(),
                })
            }
        }
    }

    /// Get the current ledger parameters without mutation.
    pub fn ledger_parameters(&self) -> LedgerParameters {
        match self {
            Self::V7 { ledger_state, .. } => {
                LedgerParameters::V7(ledger_state.parameters.deref().to_owned())
            }
            Self::V8 { ledger_state, .. } => {
                LedgerParameters::V8(ledger_state.parameters.deref().to_owned())
            }
        }
    }

    pub fn load(
        key: &SerializedLedgerStateKey,
        protocol_version: ProtocolVersion,
    ) -> Result<Self, Error> {
        let ledger_state = match protocol_version.ledger_version()? {
            LedgerVersion::V7 => {
                let arena_key = TypedArenaKey::<
                    LedgerStateV7<LedgerDb>,
                    <LedgerDb as DB>::Hasher,
                >::deserialize(&mut key.as_slice(), 0)
                .map_err(|error| Error::Deserialize("TypedArenaKeyV7", error))?;
                let ledger_state = default_storage::<LedgerDb>()
                    .get(&arena_key)
                    .map_err(|error| Error::LoadLedgerState(key.to_owned(), error))?;
                let ledger_state =
                    Sp::into_inner(ledger_state).expect("loaded ledger state exists");

                Self::V7 {
                    ledger_state,
                    block_fullness: Default::default(),
                }
            }

            LedgerVersion::V8 => {
                let arena_key = TypedArenaKey::<
                    LedgerStateV8<LedgerDb>,
                    <LedgerDb as DB>::Hasher,
                >::deserialize(&mut key.as_slice(), 0)
                .map_err(|error| Error::Deserialize("TypedArenaKeyV8", error))?;
                let ledger_state = default_storage::<LedgerDb>()
                    .get(&arena_key)
                    .map_err(|error| Error::LoadLedgerState(key.to_owned(), error))?;
                let ledger_state =
                    Sp::into_inner(ledger_state).expect("loaded ledger state exists");

                Self::V8 {
                    ledger_state,
                    block_fullness: Default::default(),
                }
            }
        };

        Ok(ledger_state)
    }

    pub fn translate(self, ledger_version: LedgerVersion) -> Result<Self, Error> {
        match (self, ledger_version) {
            (s @ LedgerState::V7 { .. }, LedgerVersion::V7) => Ok(s),

            (
                LedgerState::V7 {
                    ledger_state,
                    block_fullness,
                },
                LedgerVersion::V8,
            ) => {
                let mut ledger_state = Sp::new(ledger_state);
                ledger_state.persist();

                let key = ArenaKey::from(ledger_state.as_typed_key());
                let key =
                    TypedArenaKey::<LedgerStateV8<LedgerDb>, <LedgerDb as DB>::Hasher>::from(key);

                let ledger_state = default_storage::<LedgerDb>().get(&key).map_err(|error| {
                    Error::LedgerStateTranslation(LedgerVersion::V7, ledger_version, error)
                })?;
                let ledger_state = Sp::into_inner(ledger_state).expect("ledger state exists");

                Ok(LedgerState::V8 {
                    ledger_state,
                    block_fullness,
                })
            }

            (s @ LedgerState::V8 { .. }, LedgerVersion::V7) => Err(
                Error::BackwardsLedgerStateTranslation(s.ledger_version(), ledger_version),
            ),

            (s @ LedgerState::V8 { .. }, LedgerVersion::V8) => Ok(s),
        }
    }

    pub fn ledger_version(&self) -> LedgerVersion {
        match self {
            LedgerState::V7 { .. } => LedgerVersion::V7,
            LedgerState::V8 { .. } => LedgerVersion::V8,
        }
    }

    /// Compute the full ledger state root key without persisting or flushing to the database.
    /// This produces the same bytes as `persist()` for the state key, but without side effects.
    pub fn compute_state_root(&self) -> Result<SerializedLedgerStateKey, Error> {
        match self {
            LedgerState::V7 { ledger_state, .. } => {
                let ledger_state = Sp::new(ledger_state.clone());
                ledger_state
                    .as_typed_key()
                    .serialize()
                    .map_err(|error| Error::Serialize("StateRootV7", error))
            }

            LedgerState::V8 { ledger_state, .. } => {
                let ledger_state = Sp::new(ledger_state.clone());
                ledger_state
                    .as_typed_key()
                    .serialize()
                    .map_err(|error| Error::Serialize("StateRootV8", error))
            }
        }
    }

    pub fn persist(self) -> Result<(Self, SerializedLedgerStateKey), Error> {
        match self {
            LedgerState::V7 {
                ledger_state,
                block_fullness,
            } => {
                let mut ledger_state = Sp::new(ledger_state);
                ledger_state.persist();
                default_storage::<LedgerDb>().with_backend(|b| b.flush_all_changes_to_db());

                let key = ledger_state
                    .as_typed_key()
                    .serialize()
                    .map_err(|error| Error::Serialize("TypedArenaKeyV7", error))?;

                let ledger_state = Sp::into_inner(ledger_state).expect("ledger state exists");
                let ledger_state = LedgerState::V7 {
                    ledger_state,
                    block_fullness,
                };

                Ok((ledger_state, key))
            }

            LedgerState::V8 {
                ledger_state,
                block_fullness,
            } => {
                let mut ledger_state = Sp::new(ledger_state);
                ledger_state.persist();
                default_storage::<LedgerDb>().with_backend(|b| b.flush_all_changes_to_db());

                let key = ledger_state
                    .as_typed_key()
                    .serialize()
                    .map_err(|error| Error::Serialize("TypedArenaKeyV8", error))?;

                let ledger_state = Sp::into_inner(ledger_state).expect("ledger state exists");
                let ledger_state = LedgerState::V8 {
                    ledger_state,
                    block_fullness,
                };

                Ok((ledger_state, key))
            }
        }
    }

    /// Apply the given serialized regular transaction to this ledger state and return the
    /// transaction result as well as the created and spent unshielded UTXOs.
    #[trace]
    pub fn apply_regular_transaction(
        &mut self,
        transaction: &SerializedTransaction,
        parent_block_hash: ByteArray<32>,
        block_timestamp: u64,
        parent_block_timestamp: u64,
    ) -> Result<ApplyRegularTransactionOutcome, Error> {
        match self {
            Self::V7 {
                ledger_state,
                block_fullness,
            } => {
                let transaction =
                    tagged_deserialize::<TransactionV7<LedgerDb>>(&mut transaction.as_ref())
                        .map_err(|error| Error::Deserialize("LedgerTransactionV7", error))?;

                let cx = TransactionContextV7 {
                    ref_state: ledger_state.clone(),
                    block_context: BlockContextV7 {
                        tblock: timestamp(block_timestamp),
                        tblock_err: 30,
                        parent_block_hash: HashOutput(parent_block_hash.0),
                    },
                    whitelist: None,
                };

                let cost = transaction
                    .cost(&ledger_state.parameters, true)
                    .map_err(|error| Error::TransactionCost(error.into()))?;
                let verified_ledger_transaction = transaction
                    .well_formed(&cx.ref_state, *STRICTNESS_V7, cx.block_context.tblock)
                    .map_err(|error| Error::MalformedTransaction(error.into()))?;
                let (ledger_state, transaction_result) =
                    ledger_state.apply(&verified_ledger_transaction, &cx);

                let (transaction_result, events, should_count_cost) = match transaction_result {
                    TransactionResultV7::Success(events) => {
                        (TransactionResult::Success, events, true)
                    }

                    TransactionResultV7::PartialSuccess(segments, events) => {
                        let segments = segments
                            .into_iter()
                            .map(|(id, result)| (id, result.is_ok()))
                            .collect::<Vec<_>>();
                        (TransactionResult::PartialSuccess(segments), events, true)
                    }

                    TransactionResultV7::Failure(_) => (TransactionResult::Failure, vec![], false),
                };

                // Only count cost for successful/partial transactions (match node behavior)
                let block_fullness = if should_count_cost {
                    *block_fullness + cost
                } else {
                    *block_fullness
                };

                let (created_unshielded_utxos, spent_unshielded_utxos) =
                    make_unshielded_utxos_for_regular_transaction_v7(
                        transaction,
                        &transaction_result,
                        &ledger_state,
                    );

                let ledger_events = make_ledger_events_v7(events)?;

                *self = Self::V7 {
                    ledger_state,
                    block_fullness,
                };

                Ok(ApplyRegularTransactionOutcome {
                    transaction_result,
                    created_unshielded_utxos,
                    spent_unshielded_utxos,
                    ledger_events,
                })
            }

            Self::V8 {
                ledger_state,
                block_fullness,
            } => {
                let transaction =
                    tagged_deserialize::<TransactionV8<LedgerDb>>(&mut transaction.as_ref())
                        .map_err(|error| Error::Deserialize("LedgerTransactionV8", error))?;

                let cx = TransactionContextV8 {
                    ref_state: ledger_state.clone(),
                    block_context: BlockContextV8 {
                        tblock: timestamp(block_timestamp),
                        tblock_err: 30,
                        parent_block_hash: HashOutput(parent_block_hash.0),
                        last_block_time: timestamp(parent_block_timestamp),
                    },
                    whitelist: None,
                };

                let cost = transaction
                    .cost(&ledger_state.parameters, true)
                    .map_err(|error| Error::TransactionCost(error.into()))?;
                let verified_ledger_transaction = transaction
                    .well_formed(&cx.ref_state, *STRICTNESS_V8, cx.block_context.tblock)
                    .map_err(|error| Error::MalformedTransaction(error.into()))?;
                let (ledger_state, transaction_result) =
                    ledger_state.apply(&verified_ledger_transaction, &cx);

                let (transaction_result, events, should_count_cost) = match transaction_result {
                    TransactionResultV8::Success(events) => {
                        (TransactionResult::Success, events, true)
                    }

                    TransactionResultV8::PartialSuccess(segments, events) => {
                        let segments = segments
                            .into_iter()
                            .map(|(id, result)| (id, result.is_ok()))
                            .collect::<Vec<_>>();
                        (TransactionResult::PartialSuccess(segments), events, true)
                    }

                    TransactionResultV8::Failure(_) => (TransactionResult::Failure, vec![], false),
                };

                // Only count cost for successful/partial transactions (match node behavior)
                let block_fullness = if should_count_cost {
                    *block_fullness + cost
                } else {
                    *block_fullness
                };

                let (created_unshielded_utxos, spent_unshielded_utxos) =
                    make_unshielded_utxos_for_regular_transaction_v8(
                        transaction,
                        &transaction_result,
                        &ledger_state,
                    );

                let ledger_events = make_ledger_events_v8(events)?;

                *self = Self::V8 {
                    ledger_state,
                    block_fullness,
                };

                Ok(ApplyRegularTransactionOutcome {
                    transaction_result,
                    created_unshielded_utxos,
                    spent_unshielded_utxos,
                    ledger_events,
                })
            }
        }
    }

    /// Apply the given serialized system transaction to this ledger state.
    #[trace]
    pub fn apply_system_transaction(
        &mut self,
        transaction: &SerializedTransaction,
        block_timestamp: u64,
    ) -> Result<ApplySystemTransactionOutcome, Error> {
        match self {
            Self::V7 {
                ledger_state,
                block_fullness,
            } => {
                let transaction =
                    tagged_deserialize::<SystemTransactionV7>(&mut transaction.as_ref())
                        .map_err(|error| Error::Deserialize("SystemTransactionV7", error))?;

                let cost = transaction.cost(&ledger_state.parameters);
                let (ledger_state, events) = ledger_state
                    .apply_system_tx(&transaction, timestamp(block_timestamp))
                    .map_err(|error| Error::SystemTransaction(error.into()))?;
                let block_fullness = *block_fullness + cost;

                let created_unshielded_utxos =
                    make_unshielded_utxos_for_system_transaction_v7(transaction, &ledger_state);

                let ledger_events = make_ledger_events_v7(events)?;

                *self = Self::V7 {
                    ledger_state,
                    block_fullness,
                };

                Ok(ApplySystemTransactionOutcome {
                    created_unshielded_utxos,
                    ledger_events,
                })
            }

            Self::V8 {
                ledger_state,
                block_fullness,
            } => {
                let transaction =
                    tagged_deserialize::<SystemTransactionV8>(&mut transaction.as_ref())
                        .map_err(|error| Error::Deserialize("SystemTransactionV8", error))?;

                let cost = transaction.cost(&ledger_state.parameters);
                let (ledger_state, events) = ledger_state
                    .apply_system_tx(&transaction, timestamp(block_timestamp))
                    .map_err(|error| Error::SystemTransaction(error.into()))?;
                let block_fullness = *block_fullness + cost;

                let created_unshielded_utxos =
                    make_unshielded_utxos_for_system_transaction_v8(transaction, &ledger_state);

                let ledger_events = make_ledger_events_v8(events)?;

                *self = Self::V8 {
                    ledger_state,
                    block_fullness,
                };

                Ok(ApplySystemTransactionOutcome {
                    created_unshielded_utxos,
                    ledger_events,
                })
            }
        }
    }

    /// Get the first free index of the zswap state.
    pub fn zswap_first_free(&self) -> u64 {
        match self {
            Self::V7 { ledger_state, .. } => ledger_state.zswap.first_free,
            Self::V8 { ledger_state, .. } => ledger_state.zswap.first_free,
        }
    }

    /// Get the merkle tree root of the zswap state.
    pub fn zswap_merkle_tree_root(&self) -> ZswapStateRoot {
        match self {
            Self::V7 { ledger_state, .. } => {
                let root = ledger_state
                    .zswap
                    .coin_coms
                    .rehash()
                    .root()
                    .expect("zswap merkle tree root should exist");
                ZswapStateRoot::V7(root)
            }

            Self::V8 { ledger_state, .. } => {
                let root = ledger_state
                    .zswap
                    .coin_coms
                    .rehash()
                    .root()
                    .expect("zswap merkle tree root should exist");
                ZswapStateRoot::V8(root)
            }
        }
    }

    /// Extract the zswap state for the given contract address.
    #[trace(properties = { "address": "{address}" })]
    pub fn extract_contract_zswap_state(
        &self,
        address: &SerializedContractAddress,
    ) -> Result<SerializedZswapState, Error> {
        match self {
            Self::V7 { ledger_state, .. } => {
                let address = ContractAddressV7::deserialize(&mut address.as_ref(), 0)
                    .map_err(|error| Error::Deserialize("ContractAddressV7", error))?;

                let mut contract_zswap_state = ZswapStateV7::new();
                contract_zswap_state.coin_coms = ledger_state.zswap.filter(&[address]);

                contract_zswap_state
                    .tagged_serialize()
                    .map_err(|error| Error::Serialize("ZswapStateV7", error))
            }

            Self::V8 { ledger_state, .. } => {
                let address = ContractAddressV8::deserialize(&mut address.as_ref(), 0)
                    .map_err(|error| Error::Deserialize("ContractAddressV8", error))?;

                let mut contract_zswap_state = ZswapStateV8::new();
                contract_zswap_state.coin_coms = ledger_state.zswap.filter(&[address]);

                contract_zswap_state
                    .tagged_serialize()
                    .map_err(|error| Error::Serialize("ZswapStateV8", error))
            }
        }
    }

    /// Get the serialized merkle-tree collapsed update for the given indices.
    pub fn collapsed_update(&self, start_index: u64, end_index: u64) -> Result<ByteVec, Error> {
        match self {
            Self::V7 { ledger_state, .. } => MerkleTreeCollapsedUpdateV7::new(
                &ledger_state.zswap.coin_coms,
                start_index,
                end_index,
            )
            .map_err(|error| Error::InvalidUpdate(error.into()))?
            .tagged_serialize()
            .map_err(|error| Error::Serialize("MerkleTreeCollapsedUpdateV7", error)),

            Self::V8 { ledger_state, .. } => MerkleTreeCollapsedUpdateV8::new(
                &ledger_state.zswap.coin_coms,
                start_index,
                end_index,
            )
            .map_err(|error| Error::InvalidUpdate(error.into()))?
            .tagged_serialize()
            .map_err(|error| Error::Serialize("MerkleTreeCollapsedUpdateV8", error)),
        }
    }

    /// To be called after applying transactions.
    pub fn finalize_apply_transactions(
        &mut self,
        block_timestamp: u64,
    ) -> Result<LedgerParameters, Error> {
        match self {
            Self::V7 {
                ledger_state,
                block_fullness,
            } => {
                let timestamp = timestamp(block_timestamp);
                let normalized_fullness = block_fullness
                    .normalize(ledger_state.parameters.limits.block_limits)
                    .unwrap_or(NormalizedCost::ZERO);
                let overall_fullness = FixedPoint::max(
                    FixedPoint::max(
                        FixedPoint::max(
                            normalized_fullness.read_time,
                            normalized_fullness.compute_time,
                        ),
                        normalized_fullness.block_usage,
                    ),
                    FixedPoint::max(
                        normalized_fullness.bytes_written,
                        normalized_fullness.bytes_churned,
                    ),
                );

                let ledger_state = ledger_state
                    .post_block_update(timestamp, normalized_fullness, overall_fullness)
                    .map_err(|error| Error::BlockLimitExceeded(error.into()))?;

                let ledger_parameters = ledger_state.parameters.deref().to_owned();

                *self = Self::V7 {
                    ledger_state,
                    block_fullness: Default::default(),
                };

                Ok(LedgerParameters::V7(ledger_parameters))
            }

            Self::V8 {
                ledger_state,
                block_fullness,
            } => {
                let timestamp = timestamp(block_timestamp);
                let normalized_fullness = block_fullness
                    .normalize(ledger_state.parameters.limits.block_limits)
                    .unwrap_or(NormalizedCost::ZERO);
                let overall_fullness = FixedPoint::max(
                    FixedPoint::max(
                        FixedPoint::max(
                            normalized_fullness.read_time,
                            normalized_fullness.compute_time,
                        ),
                        normalized_fullness.block_usage,
                    ),
                    FixedPoint::max(
                        normalized_fullness.bytes_written,
                        normalized_fullness.bytes_churned,
                    ),
                );

                let ledger_state = ledger_state
                    .post_block_update(timestamp, normalized_fullness, overall_fullness)
                    .map_err(|error| Error::BlockLimitExceeded(error.into()))?;

                let ledger_parameters = ledger_state.parameters.deref().to_owned();

                *self = Self::V8 {
                    ledger_state,
                    block_fullness: Default::default(),
                };

                Ok(LedgerParameters::V8(ledger_parameters))
            }
        }
    }
}

/// Facade for ledger parameters across supported (protocol) versions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerParameters {
    V7(LedgerParametersV7),
    V8(LedgerParametersV8),
}

impl LedgerParameters {
    /// Serialize these ledger parameters.
    #[trace]
    pub fn serialize(&self) -> Result<SerializedLedgerParameters, Error> {
        match self {
            Self::V7(parameters) => parameters
                .tagged_serialize()
                .map_err(|error| Error::Serialize("SerializedLedgerParametersV7", error)),

            Self::V8(parameters) => parameters
                .tagged_serialize()
                .map_err(|error| Error::Serialize("SerializedLedgerParametersV8", error)),
        }
    }
}

/// Facade for zswap state root across supported (protocol) versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZswapStateRoot {
    V7(MerkleTreeDigestV7),
    V8(MerkleTreeDigestV8),
}

impl ZswapStateRoot {
    /// Untagged deserialize the given serialized zswap state root using the given protocol version.
    #[trace(properties = { "protocol_version": "{protocol_version}" })]
    pub fn deserialize(
        zswap_state_root: impl AsRef<[u8]>,
        protocol_version: ProtocolVersion,
    ) -> Result<Self, Error> {
        let zswap_state_root = match protocol_version.ledger_version()? {
            LedgerVersion::V7 => {
                let digest = MerkleTreeDigestV7::deserialize(&mut zswap_state_root.as_ref(), 0)
                    .map_err(|error| Error::Deserialize("MerkleTreeDigestV7", error))?;
                Self::V7(digest)
            }

            LedgerVersion::V8 => {
                let digest = MerkleTreeDigestV8::deserialize(&mut zswap_state_root.as_ref(), 0)
                    .map_err(|error| Error::Deserialize("MerkleTreeDigestV8", error))?;
                Self::V8(digest)
            }
        };

        Ok(zswap_state_root)
    }

    /// Serialize this zswap state root.
    #[trace]
    pub fn serialize(&self) -> Result<SerializedZswapStateRoot, Error> {
        match self {
            Self::V7(digest) => digest
                .serialize()
                .map_err(|error| Error::Serialize("MerkleTreeDigestV7", error)),

            Self::V8(digest) => digest
                .serialize()
                .map_err(|error| Error::Serialize("MerkleTreeDigestV8", error)),
        }
    }
}

fn timestamp(block_timestamp: u64) -> Timestamp {
    Timestamp::from_secs(block_timestamp / 1000)
}

fn make_ledger_events_v7<D>(events: Vec<EventV7<D>>) -> Result<Vec<LedgerEvent>, Error>
where
    D: DB,
{
    events
        .into_iter()
        .map(|event| {
            let raw = event
                .tagged_serialize()
                .map_err(|error| Error::Serialize("EventV7", error))?;
            Ok::<_, Error>((event, raw))
        })
        .filter_map_ok(|(event, raw)| match event.content {
            EventDetailsV7::ZswapInput { .. } => Some(Ok(LedgerEvent::zswap_input(raw))),

            EventDetailsV7::ZswapOutput { .. } => Some(Ok(LedgerEvent::zswap_output(raw))),

            EventDetailsV7::ContractDeploy { .. } => None,

            EventDetailsV7::ContractLog { .. } => None,

            EventDetailsV7::ParamChange(..) => Some(Ok(LedgerEvent::param_change(raw))),

            EventDetailsV7::DustInitialUtxo {
                output,
                generation,
                generation_index,
                ..
            } => Some(make_dust_initial_utxo_v7(
                output,
                generation,
                generation_index,
                raw,
            )),

            EventDetailsV7::DustGenerationDtimeUpdate { update, .. } => {
                Some(make_dust_generation_dtime_update_v7(update, raw))
            }

            EventDetailsV7::DustSpendProcessed { .. } => {
                Some(Ok(LedgerEvent::dust_spend_processed(raw)))
            }

            other => panic!("unexpected EventDetailsV7 variant {other:?}"),
        })
        .flatten()
        .collect::<Result<_, _>>()
}

fn make_ledger_events_v8<D>(events: Vec<EventV8<D>>) -> Result<Vec<LedgerEvent>, Error>
where
    D: DB,
{
    events
        .into_iter()
        .map(|event| {
            let raw = event
                .tagged_serialize()
                .map_err(|error| Error::Serialize("EventV8", error))?;
            Ok::<_, Error>((event, raw))
        })
        .filter_map_ok(|(event, raw)| match event.content {
            EventDetailsV8::ZswapInput { .. } => Some(Ok(LedgerEvent::zswap_input(raw))),

            EventDetailsV8::ZswapOutput { .. } => Some(Ok(LedgerEvent::zswap_output(raw))),

            EventDetailsV8::ContractDeploy { .. } => None,

            EventDetailsV8::ContractLog { .. } => None,

            EventDetailsV8::ParamChange(..) => Some(Ok(LedgerEvent::param_change(raw))),

            EventDetailsV8::DustInitialUtxo {
                output,
                generation,
                generation_index,
                ..
            } => Some(make_dust_initial_utxo_v8(
                output,
                generation,
                generation_index,
                raw,
            )),

            EventDetailsV8::DustGenerationDtimeUpdate { update, .. } => {
                Some(make_dust_generation_dtime_update_v8(update, raw))
            }

            EventDetailsV8::DustSpendProcessed { .. } => {
                Some(Ok(LedgerEvent::dust_spend_processed(raw)))
            }

            other => panic!("unexpected EventDetailsV8 variant {other:?}"),
        })
        .flatten()
        .collect::<Result<_, _>>()
}

fn make_dust_initial_utxo_v7(
    output: QualifiedDustOutputV7,
    generation: DustGenerationInfoV7,
    generation_index: u64,
    raw: ByteVec,
) -> Result<LedgerEvent, Error> {
    let owner = output
        .owner
        .serialize()
        .map_err(|error| Error::Serialize("DustPublicKeyV7", error))?;

    let qualified_output = dust::QualifiedDustOutput {
        initial_value: output.initial_value,
        owner,
        nonce: output.nonce.0.to_bytes_le().into(),
        seq: output.seq,
        ctime: output.ctime.to_secs(),
        backing_night: output.backing_night.0.0.into(),
        mt_index: output.mt_index,
    };

    let owner = generation
        .owner
        .serialize()
        .map_err(|error| Error::Serialize("DustPublicKeyV7", error))?;

    let generation_info = dust::DustGenerationInfo {
        night_utxo_hash: output.backing_night.0.0.into(),
        value: generation.value,
        owner,
        nonce: generation.nonce.0.0.into(),
        ctime: output.ctime.to_secs(),
        dtime: generation.dtime.to_secs(),
    };

    Ok(LedgerEvent::dust_initial_utxo(
        raw,
        qualified_output,
        generation_info,
        generation_index,
    ))
}

fn make_dust_initial_utxo_v8(
    output: QualifiedDustOutputV8,
    generation: DustGenerationInfoV8,
    generation_index: u64,
    raw: ByteVec,
) -> Result<LedgerEvent, Error> {
    let owner = output
        .owner
        .serialize()
        .map_err(|error| Error::Serialize("DustPublicKeyV8", error))?;

    let qualified_output = dust::QualifiedDustOutput {
        initial_value: output.initial_value,
        owner,
        nonce: output.nonce.0.to_bytes_le().into(),
        seq: output.seq,
        ctime: output.ctime.to_secs(),
        backing_night: output.backing_night.0.0.into(),
        mt_index: output.mt_index,
    };

    let owner = generation
        .owner
        .serialize()
        .map_err(|error| Error::Serialize("DustPublicKeyV8", error))?;

    let generation_info = dust::DustGenerationInfo {
        night_utxo_hash: output.backing_night.0.0.into(),
        value: generation.value,
        owner,
        nonce: generation.nonce.0.0.into(),
        ctime: output.ctime.to_secs(),
        dtime: generation.dtime.to_secs(),
    };

    Ok(LedgerEvent::dust_initial_utxo(
        raw,
        qualified_output,
        generation_info,
        generation_index,
    ))
}

fn make_dust_generation_dtime_update_v7(
    update: TreeInsertionPathV7<DustGenerationInfoV7>,
    raw: ByteVec,
) -> Result<LedgerEvent, Error> {
    let generation = &update.leaf.1;

    let owner = generation
        .owner
        .serialize()
        .map_err(|error| Error::Serialize("DustPublicKeyV7", error))?;

    let generation_info = dust::DustGenerationInfo {
        night_utxo_hash: update.leaf.0.0.into(),
        value: generation.value,
        owner,
        nonce: generation.nonce.0.0.into(),
        ctime: 0, // DustGenerationInfo from ledger doesn't have ctime, only dtime
        dtime: generation.dtime.to_secs(),
    };

    let mt_index = update
        .path
        .iter()
        .rev()
        .enumerate()
        .fold(0u64, |mt_index, (depth, entry)| {
            if !entry.goes_left {
                mt_index | (1u64 << depth)
            } else {
                mt_index
            }
        });

    let merkle_path = update
        .path
        .iter()
        .map(|entry| dust::DustMerklePathEntry {
            sibling_hash: entry.hash.as_ref().map(|h| h.0.0.to_bytes_le().to_vec()),
            goes_left: entry.goes_left,
        })
        .collect();

    Ok(LedgerEvent::dust_generation_dtime_update(
        raw,
        generation_info,
        mt_index,
        merkle_path,
    ))
}

fn make_dust_generation_dtime_update_v8(
    update: TreeInsertionPathV8<DustGenerationInfoV8>,
    raw: ByteVec,
) -> Result<LedgerEvent, Error> {
    let generation = &update.leaf.1;

    let owner = generation
        .owner
        .serialize()
        .map_err(|error| Error::Serialize("DustPublicKeyV8", error))?;

    let generation_info = dust::DustGenerationInfo {
        night_utxo_hash: update.leaf.0.0.into(),
        value: generation.value,
        owner,
        nonce: generation.nonce.0.0.into(),
        ctime: 0, // DustGenerationInfo from ledger doesn't have ctime, only dtime
        dtime: generation.dtime.to_secs(),
    };

    let mt_index = update
        .path
        .iter()
        .rev()
        .enumerate()
        .fold(0u64, |mt_index, (depth, entry)| {
            if !entry.goes_left {
                mt_index | (1u64 << depth)
            } else {
                mt_index
            }
        });

    let merkle_path = update
        .path
        .iter()
        .map(|entry| dust::DustMerklePathEntry {
            sibling_hash: entry.hash.as_ref().map(|h| h.0.0.to_bytes_le().to_vec()),
            goes_left: entry.goes_left,
        })
        .collect();

    Ok(LedgerEvent::dust_generation_dtime_update(
        raw,
        generation_info,
        mt_index,
        merkle_path,
    ))
}

fn make_unshielded_utxos_for_regular_transaction_v7<D>(
    transaction: TransactionV7<D>,
    transaction_result: &TransactionResult,
    ledger_state: &LedgerStateV7<D>,
) -> (Vec<UnshieldedUtxo>, Vec<UnshieldedUtxo>)
where
    D: DB,
{
    // Skip UTXO creation entirely for failed transactions, because no state changes occurred on the
    // ledger.
    if matches!(transaction_result, TransactionResult::Failure) {
        return (vec![], vec![]);
    }

    match transaction {
        TransactionV7::Standard(transaction) => {
            let successful_segments = match &transaction_result {
                TransactionResult::Success => transaction.segments().into_iter().collect(),

                TransactionResult::PartialSuccess(segments) => segments
                    .iter()
                    .filter_map(|(id, success)| success.then_some(id))
                    .copied()
                    .collect(),

                TransactionResult::Failure => HashSet::new(),
            };

            let mut outputs = vec![];
            let mut inputs = vec![];

            for segment in transaction.segments() {
                // Guaranteed phase.
                if segment == 0 {
                    for intent in transaction.intents.values() {
                        extend_unshielded_utxos_v7(
                            &mut outputs,
                            &mut inputs,
                            segment,
                            &intent,
                            true,
                            ledger_state,
                        );
                    }

                // Fallible phase.
                } else if let Some(intent) = transaction.intents.get(&segment)
                    && successful_segments.contains(&segment)
                {
                    extend_unshielded_utxos_v7(
                        &mut outputs,
                        &mut inputs,
                        segment,
                        &intent,
                        false,
                        ledger_state,
                    );
                }
            }

            (outputs, inputs)
        }

        // ClaimRewards creates a single unshielded UTXO for the claimed amount.
        TransactionV7::ClaimRewards(claim) => {
            let owner = UserAddressV7::from(claim.owner);
            let ledger_intent_hash = {
                // ClaimRewards don't have intents, but UTXOs need an intent hash. We compute this
                // hash the same way that the ledger does internally.
                let output = OutputInstructionUnshieldedV7 {
                    amount: claim.value,
                    target_address: owner,
                    nonce: claim.nonce,
                };
                output.mk_intent_hash(NIGHTV7)
            };
            let intent_hash = ledger_intent_hash.0.0.into();
            let initial_nonce = make_initial_nonce_v7(OUTPUT_INDEX_ZERO, intent_hash);
            let registered_for_dust_generation =
                registered_for_dust_generation_v7(OUTPUT_INDEX_ZERO, intent_hash, ledger_state);
            let utxo = UtxoV7 {
                value: claim.value,
                owner,
                type_: UnshieldedTokenTypeV7::default(),
                intent_hash: ledger_intent_hash,
                output_no: OUTPUT_INDEX_ZERO,
            };

            let utxo = UnshieldedUtxo {
                owner: owner.0.0.into(),
                token_type: TokenType::default(), // Native token (all zeros).
                value: claim.value,
                intent_hash,
                output_index: OUTPUT_INDEX_ZERO,
                ctime: ctime_v7(&utxo, ledger_state),
                initial_nonce,
                registered_for_dust_generation,
            };

            (vec![utxo], vec![]) // Creates one UTXO, spends none.
        }
    }
}

fn make_unshielded_utxos_for_regular_transaction_v8<D>(
    transaction: TransactionV8<D>,
    transaction_result: &TransactionResult,
    ledger_state: &LedgerStateV8<D>,
) -> (Vec<UnshieldedUtxo>, Vec<UnshieldedUtxo>)
where
    D: DB,
{
    // Skip UTXO creation entirely for failed transactions, because no state changes occurred on the
    // ledger.
    if matches!(transaction_result, TransactionResult::Failure) {
        return (vec![], vec![]);
    }

    match transaction {
        TransactionV8::Standard(transaction) => {
            let successful_segments = match &transaction_result {
                TransactionResult::Success => transaction.segments().into_iter().collect(),

                TransactionResult::PartialSuccess(segments) => segments
                    .iter()
                    .filter_map(|(id, success)| success.then_some(id))
                    .copied()
                    .collect(),

                TransactionResult::Failure => HashSet::new(),
            };

            let mut outputs = vec![];
            let mut inputs = vec![];

            for segment in transaction.segments() {
                // Guaranteed phase.
                if segment == 0 {
                    for intent in transaction.intents.values() {
                        extend_unshielded_utxos_v8(
                            &mut outputs,
                            &mut inputs,
                            segment,
                            &intent,
                            true,
                            ledger_state,
                        );
                    }

                // Fallible phase.
                } else if let Some(intent) = transaction.intents.get(&segment)
                    && successful_segments.contains(&segment)
                {
                    extend_unshielded_utxos_v8(
                        &mut outputs,
                        &mut inputs,
                        segment,
                        &intent,
                        false,
                        ledger_state,
                    );
                }
            }

            (outputs, inputs)
        }

        // ClaimRewards creates a single unshielded UTXO for the claimed amount.
        TransactionV8::ClaimRewards(claim) => {
            let owner = UserAddressV8::from(claim.owner);
            let ledger_intent_hash = {
                // ClaimRewards don't have intents, but UTXOs need an intent hash. We compute this
                // hash the same way that the ledger does internally.
                let output = OutputInstructionUnshieldedV8 {
                    amount: claim.value,
                    target_address: owner,
                    nonce: claim.nonce,
                };
                output.mk_intent_hash(NIGHTV8)
            };
            let intent_hash = ledger_intent_hash.0.0.into();
            let initial_nonce = make_initial_nonce_v8(OUTPUT_INDEX_ZERO, intent_hash);
            let registered_for_dust_generation =
                registered_for_dust_generation_v8(OUTPUT_INDEX_ZERO, intent_hash, ledger_state);
            let utxo = UtxoV8 {
                value: claim.value,
                owner,
                type_: UnshieldedTokenTypeV8::default(),
                intent_hash: ledger_intent_hash,
                output_no: OUTPUT_INDEX_ZERO,
            };

            let utxo = UnshieldedUtxo {
                owner: owner.0.0.into(),
                token_type: TokenType::default(), // Native token (all zeros).
                value: claim.value,
                intent_hash,
                output_index: OUTPUT_INDEX_ZERO,
                ctime: ctime_v8(&utxo, ledger_state),
                initial_nonce,
                registered_for_dust_generation,
            };

            (vec![utxo], vec![]) // Creates one UTXO, spends none.
        }
    }
}

fn make_unshielded_utxos_for_system_transaction_v7<D>(
    transaction: SystemTransactionV7,
    ledger_state: &LedgerStateV7<D>,
) -> Vec<UnshieldedUtxo>
where
    D: DB,
{
    match transaction {
        SystemTransactionV7::PayFromTreasuryUnshielded {
            outputs,
            token_type,
        } => {
            outputs
                .iter()
                .enumerate()
                .map(|(index, output)| {
                    // Compute intent_hash same way ledger does:
                    // midnight-ledger/ledger/src/structure.rs:589
                    let ledger_intent_hash = output.clone().mk_intent_hash(token_type);
                    let intent_hash = ledger_intent_hash.0.0.into();
                    let initial_nonce = make_initial_nonce_v7(index as u32, intent_hash);
                    let registered_for_dust_generation =
                        registered_for_dust_generation_v7(index as u32, intent_hash, ledger_state);
                    let utxo = UtxoV7 {
                        value: output.amount,
                        owner: output.target_address,
                        type_: token_type,
                        intent_hash: ledger_intent_hash,
                        output_no: index as u32,
                    };

                    UnshieldedUtxo {
                        owner: output.target_address.0.0.into(),
                        token_type: token_type.0.0.into(),
                        value: output.amount,
                        intent_hash,
                        output_index: index as u32,
                        ctime: ctime_v7(&utxo, ledger_state),
                        initial_nonce,
                        registered_for_dust_generation,
                    }
                })
                .collect()
        }

        _ => vec![], // Other system transaction types don't create unshielded UTXOs.
    }
}

fn make_unshielded_utxos_for_system_transaction_v8<D>(
    transaction: SystemTransactionV8,
    ledger_state: &LedgerStateV8<D>,
) -> Vec<UnshieldedUtxo>
where
    D: DB,
{
    match transaction {
        SystemTransactionV8::PayFromTreasuryUnshielded {
            outputs,
            token_type,
        } => {
            outputs
                .iter()
                .enumerate()
                .map(|(index, output)| {
                    // Compute intent_hash same way ledger does:
                    // midnight-ledger/ledger/src/structure.rs:589
                    let ledger_intent_hash = output.clone().mk_intent_hash(token_type);
                    let intent_hash = ledger_intent_hash.0.0.into();
                    let initial_nonce = make_initial_nonce_v8(index as u32, intent_hash);
                    let registered_for_dust_generation =
                        registered_for_dust_generation_v8(index as u32, intent_hash, ledger_state);
                    let utxo = UtxoV8 {
                        value: output.amount,
                        owner: output.target_address,
                        type_: token_type,
                        intent_hash: ledger_intent_hash,
                        output_no: index as u32,
                    };

                    UnshieldedUtxo {
                        owner: output.target_address.0.0.into(),
                        token_type: token_type.0.0.into(),
                        value: output.amount,
                        intent_hash,
                        output_index: index as u32,
                        ctime: ctime_v8(&utxo, ledger_state),
                        initial_nonce,
                        registered_for_dust_generation,
                    }
                })
                .collect()
        }

        _ => vec![], // Other system transaction types don't create unshielded UTXOs.
    }
}

fn extend_unshielded_utxos_v7<D>(
    outputs: &mut Vec<UnshieldedUtxo>,
    inputs: &mut Vec<UnshieldedUtxo>,
    segment_id: u16,
    intent: &IntentV7<D>,
    guaranteed: bool,
    ledger_state: &LedgerStateV7<D>,
) where
    D: DB,
{
    let ledger_intent_hash = intent
        .erase_proofs()
        .erase_signatures()
        .intent_hash(segment_id);
    let intent_hash = ledger_intent_hash.0.0.into();

    let intent_outputs = if guaranteed {
        intent.guaranteed_outputs()
    } else {
        intent.fallible_outputs()
    };
    let intent_outputs = intent_outputs
        .into_iter()
        .enumerate()
        .map(|(output_index, output)| {
            let output_index = output_index as u32;
            let initial_nonce = make_initial_nonce_v7(output_index, intent_hash);
            let registered_for_dust_generation =
                registered_for_dust_generation_v7(output_index, intent_hash, ledger_state);
            let utxo = UtxoV7 {
                value: output.value,
                owner: output.owner,
                type_: output.type_,
                intent_hash: ledger_intent_hash,
                output_no: output_index,
            };

            UnshieldedUtxo {
                owner: output.owner.0.0.into(),
                token_type: output.type_.0.0.into(),
                value: output.value,
                intent_hash,
                output_index,
                ctime: ctime_v7(&utxo, ledger_state),
                initial_nonce,
                registered_for_dust_generation,
            }
        });
    outputs.extend(intent_outputs);

    let intent_inputs = if guaranteed {
        intent.guaranteed_inputs()
    } else {
        intent.fallible_inputs()
    };
    let intent_inputs = intent_inputs.into_iter().map(|spend| {
        let intent_hash = spend.intent_hash.0.0.into();
        let initial_nonce = make_initial_nonce_v7(spend.output_no, intent_hash);
        let registered_for_dust_generation =
            registered_for_dust_generation_v7(spend.output_no, intent_hash, ledger_state);
        let utxo = UtxoV7 {
            value: spend.value,
            owner: UserAddressV7::from(spend.owner.clone()),
            type_: spend.type_,
            intent_hash: spend.intent_hash,
            output_no: spend.output_no,
        };

        UnshieldedUtxo {
            owner: UserAddressV7::from(spend.owner).0.0.into(),
            token_type: spend.type_.0.0.into(),
            value: spend.value,
            intent_hash,
            output_index: spend.output_no,
            ctime: ctime_v7(&utxo, ledger_state),
            initial_nonce,
            registered_for_dust_generation,
        }
    });
    inputs.extend(intent_inputs);
}

fn extend_unshielded_utxos_v8<D>(
    outputs: &mut Vec<UnshieldedUtxo>,
    inputs: &mut Vec<UnshieldedUtxo>,
    segment_id: u16,
    intent: &IntentV8<D>,
    guaranteed: bool,
    ledger_state: &LedgerStateV8<D>,
) where
    D: DB,
{
    let ledger_intent_hash = intent
        .erase_proofs()
        .erase_signatures()
        .intent_hash(segment_id);
    let intent_hash = ledger_intent_hash.0.0.into();

    let intent_outputs = if guaranteed {
        intent.guaranteed_outputs()
    } else {
        intent.fallible_outputs()
    };
    let intent_outputs = intent_outputs
        .into_iter()
        .enumerate()
        .map(|(output_index, output)| {
            let output_index = output_index as u32;
            let initial_nonce = make_initial_nonce_v8(output_index, intent_hash);
            let registered_for_dust_generation =
                registered_for_dust_generation_v8(output_index, intent_hash, ledger_state);
            let utxo = UtxoV8 {
                value: output.value,
                owner: output.owner,
                type_: output.type_,
                intent_hash: ledger_intent_hash,
                output_no: output_index,
            };

            UnshieldedUtxo {
                owner: output.owner.0.0.into(),
                token_type: output.type_.0.0.into(),
                value: output.value,
                intent_hash,
                output_index,
                ctime: ctime_v8(&utxo, ledger_state),
                initial_nonce,
                registered_for_dust_generation,
            }
        });
    outputs.extend(intent_outputs);

    let intent_inputs = if guaranteed {
        intent.guaranteed_inputs()
    } else {
        intent.fallible_inputs()
    };
    let intent_inputs = intent_inputs.into_iter().map(|spend| {
        let intent_hash = spend.intent_hash.0.0.into();
        let initial_nonce = make_initial_nonce_v8(spend.output_no, intent_hash);
        let registered_for_dust_generation =
            registered_for_dust_generation_v8(spend.output_no, intent_hash, ledger_state);
        let utxo = UtxoV8 {
            value: spend.value,
            owner: UserAddressV8::from(spend.owner.clone()),
            type_: spend.type_,
            intent_hash: spend.intent_hash,
            output_no: spend.output_no,
        };

        UnshieldedUtxo {
            owner: UserAddressV8::from(spend.owner).0.0.into(),
            token_type: spend.type_.0.0.into(),
            value: spend.value,
            intent_hash,
            output_index: spend.output_no,
            ctime: ctime_v8(&utxo, ledger_state),
            initial_nonce,
            registered_for_dust_generation,
        }
    });
    inputs.extend(intent_inputs);
}

fn make_initial_nonce_v7(output_index: u32, intent_hash: IntentHash) -> Nonce {
    let intent_hash = HashOutput(intent_hash.0);
    let initial_nonce = InitialNonceV7(persistent_commit(&output_index, intent_hash));
    ByteArray(initial_nonce.0.0)
}

fn make_initial_nonce_v8(output_index: u32, intent_hash: IntentHash) -> Nonce {
    let intent_hash = HashOutput(intent_hash.0);
    let initial_nonce = InitialNonceV8(persistent_commit(&output_index, intent_hash));
    ByteArray(initial_nonce.0.0)
}

fn registered_for_dust_generation_v7<D>(
    output_index: u32,
    intent_hash: IntentHash,
    ledger_state: &LedgerStateV7<D>,
) -> bool
where
    D: DB,
{
    let intent_hash_v7 = HashOutput(intent_hash.0);
    let initial_nonce = InitialNonceV7(persistent_commit(&output_index, intent_hash_v7));
    ledger_state
        .dust
        .generation
        .night_indices
        .contains_key(&initial_nonce)
}

fn registered_for_dust_generation_v8<D>(
    output_index: u32,
    intent_hash: IntentHash,
    ledger_state: &LedgerStateV8<D>,
) -> bool
where
    D: DB,
{
    let intent_hash = HashOutput(intent_hash.0);
    let initial_nonce = InitialNonceV8(persistent_commit(&output_index, intent_hash));
    ledger_state
        .dust
        .generation
        .night_indices
        .contains_key(&initial_nonce)
}

fn ctime_v7<D>(utxo: &UtxoV7, ledger_state: &LedgerStateV7<D>) -> Option<u64>
where
    D: DB,
{
    ledger_state
        .utxo
        .utxos
        .get(utxo)
        .map(|meta| meta.ctime.to_secs())
}

fn ctime_v8<D>(utxo: &UtxoV8, ledger_state: &LedgerStateV8<D>) -> Option<u64>
where
    D: DB,
{
    ledger_state
        .utxo
        .utxos
        .get(utxo)
        .map(|meta| meta.ctime.to_secs())
}

#[cfg(test)]
mod tests {
    use crate::{
        domain::{LedgerVersion, ProtocolVersion, ledger::LedgerState},
        error::BoxError,
    };
    use anyhow::Context;

    #[cfg(any(feature = "cloud", feature = "standalone"))]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_translate() -> Result<(), BoxError> {
        #[cfg(feature = "cloud")]
        let _postgres_container = {
            use crate::infra::{ledger_db, migrations, pool::postgres::PostgresPool};
            use sqlx::postgres::PgSslMode;
            use std::time::Duration;
            use testcontainers::{ImageExt, runners::AsyncRunner};
            use testcontainers_modules::postgres::Postgres;

            let postgres_container = Postgres::default()
                .with_db_name("indexer")
                .with_user("indexer")
                .with_password(env!("APP__INFRA__STORAGE__PASSWORD"))
                .with_tag("17.1-alpine")
                .start()
                .await
                .context("start Postgres container")?;
            let postgres_port = postgres_container
                .get_host_port_ipv4(5432)
                .await
                .context("get Postgres port")?;

            let config = crate::infra::pool::postgres::Config {
                host: "localhost".to_string(),
                port: postgres_port,
                dbname: "indexer".to_string(),
                user: "indexer".to_string(),
                password: env!("APP__INFRA__STORAGE__PASSWORD").into(),
                sslmode: PgSslMode::Prefer,
                max_connections: 10,
                idle_timeout: Duration::from_secs(60),
                max_lifetime: Duration::from_secs(5 * 60),
            };

            let pool = PostgresPool::new(config).await.context("create pool")?;
            migrations::postgres::run(&pool)
                .await
                .context("run migrations")?;

            ledger_db::init(ledger_db::Config { cache_size: 1_024 }, pool);

            postgres_container
        };

        #[cfg(feature = "standalone")]
        {
            use crate::infra::{
                ledger_db, migrations,
                pool::{self, sqlite::SqlitePool},
            };

            let temp_dir = tempfile::tempdir().context("cannot create tempdir")?;
            let sqlite_file = temp_dir.path().join("indexer.sqlite").display().to_string();
            let sqlite_ledger_db_file = temp_dir
                .path()
                .join("ledger-db.sqlite")
                .display()
                .to_string();

            let pool = SqlitePool::new(pool::sqlite::Config {
                cnn_url: sqlite_file,
            })
            .await
            .context("create pool")?;
            migrations::sqlite::run(&pool)
                .await
                .context("run migrations")?;

            ledger_db::init(ledger_db::Config {
                cache_size: 1_024,
                cnn_url: sqlite_ledger_db_file,
            })
            .await
            .expect("ledger DB can be initialized");
        }

        let ledger_state = LedgerState::new("undeployed".try_into()?, ProtocolVersion::OLDEST)
            .expect("ledger state can be constructed");
        assert_eq!(ledger_state.ledger_version(), LedgerVersion::V7);

        let new_ledger_state = ledger_state
            .clone()
            .translate(LedgerVersion::V7)
            .expect("ledger state v7 can be translated to v7");
        assert_eq!(new_ledger_state, ledger_state);

        let new_ledger_state = ledger_state
            .clone()
            .translate(LedgerVersion::V8)
            .expect("ledger state v7 can be translated to v8");
        assert_ne!(new_ledger_state, ledger_state);

        let result = new_ledger_state.translate(LedgerVersion::V7);
        assert!(result.is_err());

        Ok(())
    }
}
