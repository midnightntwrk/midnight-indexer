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
        AddressOrContract, ApplyRegularTransactionOutcome, ApplySystemTransactionOutcome,
        ByteArray, ByteVec, IntentHash, LedgerEvent, LedgerEventAttributes, LedgerVersion,
        NetworkId, Nonce, SerializedContractAddress, SerializedLedgerParameters,
        SerializedLedgerStateKey, SerializedTransaction, SerializedZswapMerkleTreeRoot,
        SerializedZswapState, TokenType, TransactionResult, UnshieldedAddress, UnshieldedUtxo,
        bridge::BridgeClaim,
        dust::{self},
        ledger::{
            Error, IntentV8, IntentV9, SerializableExt, TaggedSerializableExt, TransactionV8,
            TransactionV9,
        },
    },
    infra::ledger_db::v1_1,
};
use fastrace::trace;
use itertools::Itertools;
use log::{error, info, warn};
use midnight_base_crypto_v1::{
    cost_model::{FixedPoint, NormalizedCost, SyntheticCost},
    hash::{HashOutput, persistent_commit},
    time::Timestamp,
};
use midnight_coin_structure_v2::{
    coin::{NIGHT, TokenType as LedgerTokenType, UnshieldedTokenType, UserAddress},
    contract::ContractAddress as ContractAddressV8,
};
use midnight_coin_structure_v3::{
    coin::{
        NIGHT as NIGHT_V9, UnshieldedTokenType as UnshieldedTokenTypeV9,
        UserAddress as UserAddressV9,
    },
    contract::ContractAddress as ContractAddressV9,
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
        LedgerParameters as LedgerParametersV8, LedgerState as LedgerStateV8,
        OutputInstructionUnshielded as OutputInstructionUnshieldedV8,
        SystemTransaction as SystemTransactionV8, Utxo as UtxoV8,
    },
    verify::WellFormedStrictness as WellFormedStrictnessV8,
};
use midnight_ledger_v9::{
    dust::{
        DustGenerationInfo as DustGenerationInfoV9, InitialNonce as InitialNonceV9,
        QualifiedDustOutput as QualifiedDustOutputV9,
    },
    error::FeeCalculationError as FeeCalculationErrorV9,
    events::{Event as EventV9, EventDetails as EventDetailsV9},
    semantics::{
        TransactionContext as TransactionContextV9, TransactionResult as TransactionResultV9,
    },
    structure::{
        ClaimKind as ClaimKindV9, LedgerParameters as LedgerParametersV9,
        LedgerState as LedgerStateV9, OutputInstructionUnshielded as OutputInstructionUnshieldedV9,
        SPECKS_PER_DUST as SPECKS_PER_DUST_V9, SystemTransaction as SystemTransactionV9,
        Utxo as UtxoV9,
    },
    verify::WellFormedStrictness as WellFormedStrictnessV9,
};
use midnight_onchain_runtime_v3::context::BlockContext as BlockContextV3;
use midnight_onchain_runtime_v4::{
    context::BlockContext as BlockContextV4,
    ops::{LogEventType, VersionedLogItem},
    state::{EntryPointBuf, StateValue},
};
use midnight_serialize_v1::{Deserializable, tagged_deserialize};
use midnight_storage_core_v1::{
    arena::{Sp, TypedArenaKey},
    db::DB,
    storage::default_storage,
};
use midnight_transient_crypto_v2::merkle_tree::{
    MerkleTreeCollapsedUpdate, MerkleTreeDigest, TreeInsertionPath,
};
use midnight_transient_crypto_v3::merkle_tree::{
    MerkleTreeCollapsedUpdate as MerkleTreeCollapsedUpdateV9,
    MerkleTreeDigest as MerkleTreeDigestV9, TreeInsertionPath as TreeInsertionPathV9,
};
use midnight_zswap_v8::ledger::State as ZswapStateV8;
use midnight_zswap_v9::ledger::State as ZswapStateV9;
use std::{collections::HashSet, ops::Deref, sync::LazyLock};

const OUTPUT_INDEX_ZERO: u32 = 0;

/// Canonical serialized payload sizes per `LogEventType` variant. The address in the Unshielded
/// Spend/Receive/Burn events is `Either<ZswapCoinPublicKey, ContractAddress>`, which Compact
/// serialises as 65 bytes (`[is_left:1][left:32][right:32]`, both variant slots present; see
/// `take_either_address`). Per MIP-0002, Spend/Receive then carry `domain_sep` (32) + `token_type`
/// (32) + `amount` (16) = 145; Burn has no `domain_sep`, so `65 + 32 + 16 = 113`. `UnshieldedMint`
/// has a `domain_sep` in place of the address (`32 + 32 + 16 = 80`).
const SHIELDED_SPEND_SIZE: usize = 32;
const SHIELDED_RECEIVE_SIZE: usize = 578; // 32 + (1 + 32) + (1 + 512).
const SHIELDED_MINT_SIZE: usize = 81; // 32 + 32 + (1 + 16).
const SHIELDED_BURN_SIZE: usize = 49; // 32 + (1 + 16).
const UNSHIELDED_SPEND_SIZE: usize = 145; // (1 + 32 + 32) + 32 + 32 + 16.
const UNSHIELDED_RECEIVE_SIZE: usize = 145; // (1 + 32 + 32) + 32 + 32 + 16.
const UNSHIELDED_MINT_SIZE: usize = 80; // 32 + 32 + 16.
const UNSHIELDED_BURN_SIZE: usize = 113; // (1 + 32 + 32) + 32 + 16.
const MISC_SIZE: usize = 288; // 32 + 256.

const BYTES_32_SIZE: usize = 32;
const UINT_128_SIZE: usize = 16;
const EITHER_SIZE: usize = 1 + 2 * BYTES_32_SIZE; // is_left + left(32) + right(32).
const MAYBE_512_SIZE: usize = 1 + 512;

static STRICTNESS_V8: LazyLock<WellFormedStrictnessV8> = LazyLock::new(|| {
    let mut strictness = WellFormedStrictnessV8::default();
    strictness.enforce_balancing = false;
    strictness
});

static STRICTNESS_V9: LazyLock<WellFormedStrictnessV9> = LazyLock::new(|| {
    let mut strictness = WellFormedStrictnessV9::default();
    strictness.enforce_balancing = false;
    strictness
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerState {
    V8 {
        ledger_state: LedgerStateV8<v1_1::LedgerDb>,
        block_fullness: SyntheticCost,
    },
    V9 {
        ledger_state: LedgerStateV9<v1_1::LedgerDb>,
        // base-crypto is unified at 1.1.0 across v8 and v9, so the cost model
        // types (incl. SyntheticCost) are shared with the V8 variant.
        block_fullness: SyntheticCost,
    },
}

impl LedgerState {
    #[allow(missing_docs)]
    pub fn new(network_id: NetworkId, ledger_version: LedgerVersion) -> Result<Self, Error> {
        let ledger_state = match ledger_version {
            LedgerVersion::V8 => Self::V8 {
                ledger_state: LedgerStateV8::new(network_id),
                block_fullness: Default::default(),
            },
            LedgerVersion::V9 => Self::V9 {
                ledger_state: LedgerStateV9::new(network_id),
                block_fullness: Default::default(),
            },
        };

        Ok(ledger_state)
    }

    /// Create a [LedgerState] by deserializing the genesis ledger state from system properties.
    pub fn from_genesis(
        raw: impl AsRef<[u8]>,
        ledger_version: LedgerVersion,
    ) -> Result<Self, Error> {
        match ledger_version {
            LedgerVersion::V8 => {
                let ledger_state =
                    tagged_deserialize::<LedgerStateV8<v1_1::LedgerDb>>(&mut raw.as_ref())
                        .map_err(|error| Error::Deserialize("GenesisLedgerStateV8", error))?;

                let treasury_night = ledger_state
                    .treasury
                    .get(&LedgerTokenType::Unshielded(NIGHT))
                    .copied()
                    .unwrap_or(0);

                info!(
                    locked_pool = ledger_state.locked_pool,
                    reserve_pool = ledger_state.reserve_pool,
                    treasury_night;
                    "deserialized genesis ledger state"
                );

                Ok(Self::V8 {
                    ledger_state,
                    block_fullness: Default::default(),
                })
            }

            LedgerVersion::V9 => {
                let ledger_state =
                    tagged_deserialize::<LedgerStateV9<v1_1::LedgerDb>>(&mut raw.as_ref())
                        .map_err(|error| Error::Deserialize("GenesisLedgerStateV9", error))?;

                Ok(Self::V9 {
                    ledger_state,
                    block_fullness: Default::default(),
                })
            }
        }
    }

    pub fn ledger_parameters(&self) -> LedgerParameters {
        match self {
            Self::V8 { ledger_state, .. } => {
                LedgerParameters::V8(ledger_state.parameters.deref().to_owned())
            }
            Self::V9 { ledger_state, .. } => {
                LedgerParameters::V9(ledger_state.parameters.deref().to_owned())
            }
        }
    }

    /// Net remaining-claimable for the recipient, from the ledger's `bridge_receiving` map
    /// (credited net on deposit, removed on claim). Authoritative, unlike event-derived
    /// `deposited - claimed`, which carries the bridge fee. `0` for V8 (ledger 9 only).
    pub fn bridge_receiving(&self, address: UnshieldedAddress) -> u128 {
        match self {
            Self::V8 { .. } => 0,
            Self::V9 { ledger_state, .. } => {
                let address = UserAddressV9(HashOutput(address.0));
                ledger_state
                    .bridge_receiving
                    .get(&address)
                    .copied()
                    .unwrap_or(0)
            }
        }
    }

    pub fn load(
        key: &SerializedLedgerStateKey,
        ledger_version: LedgerVersion,
    ) -> Result<Self, Error> {
        let ledger_state = match ledger_version {
            LedgerVersion::V8 => {
                let arena_key = TypedArenaKey::<
                    LedgerStateV8<v1_1::LedgerDb>,
                    <v1_1::LedgerDb as DB>::Hasher,
                >::deserialize(&mut key.as_slice(), 0)
                .map_err(|error| Error::Deserialize("TypedArenaKeyV8", error))?;
                let ledger_state = default_storage::<v1_1::LedgerDb>()
                    .get_lazy(&arena_key)
                    .map_err(|error| Error::LoadLedgerState(key.to_owned(), error))?;
                let ledger_state = (*ledger_state).clone();

                Self::V8 {
                    ledger_state,
                    block_fullness: Default::default(),
                }
            }

            LedgerVersion::V9 => {
                let arena_key = TypedArenaKey::<
                    LedgerStateV9<v1_1::LedgerDb>,
                    <v1_1::LedgerDb as DB>::Hasher,
                >::deserialize(&mut key.as_slice(), 0)
                .map_err(|error| Error::Deserialize("TypedArenaKeyV9", error))?;
                let ledger_state = default_storage::<v1_1::LedgerDb>()
                    .get_lazy(&arena_key)
                    .map_err(|error| Error::LoadLedgerState(key.to_owned(), error))?;
                let ledger_state = (*ledger_state).clone();

                Self::V9 {
                    ledger_state,
                    block_fullness: Default::default(),
                }
            }
        };

        Ok(ledger_state)
    }

    pub fn translate(self, ledger_version: LedgerVersion) -> Result<Self, Error> {
        match (self, ledger_version) {
            (s @ LedgerState::V8 { .. }, LedgerVersion::V8) => Ok(s),
            (s @ LedgerState::V9 { .. }, LedgerVersion::V9) => Ok(s),
            (LedgerState::V8 { .. }, LedgerVersion::V9) => Err(
                Error::UnsupportedLedgerStateTranslation(LedgerVersion::V8, LedgerVersion::V9),
            ),
            (LedgerState::V9 { .. }, LedgerVersion::V8) => Err(
                Error::BackwardsLedgerStateTranslation(LedgerVersion::V9, LedgerVersion::V8),
            ),
        }
    }

    pub fn ledger_version(&self) -> LedgerVersion {
        match self {
            LedgerState::V8 { .. } => LedgerVersion::V8,
            LedgerState::V9 { .. } => LedgerVersion::V9,
        }
    }

    pub fn root(&self) -> Result<ByteVec, Error> {
        match self {
            Self::V8 { ledger_state, .. } => default_storage::<v1_1::LedgerDb>()
                .alloc(ledger_state.to_owned())
                .as_typed_key()
                .serialize()
                .map_err(|error| Error::Serialize("LedgerStateV8", error)),
            Self::V9 { ledger_state, .. } => default_storage::<v1_1::LedgerDb>()
                .alloc(ledger_state.to_owned())
                .as_typed_key()
                .serialize()
                .map_err(|error| Error::Serialize("LedgerStateV9", error)),
        }
    }

    pub fn persist(self) -> Result<(Self, SerializedLedgerStateKey), Error> {
        match self {
            LedgerState::V8 {
                ledger_state,
                block_fullness,
            } => {
                let mut ledger_state = Sp::new(ledger_state);
                ledger_state.persist();
                default_storage::<v1_1::LedgerDb>().with_backend(|b| b.flush_all_changes_to_db());

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

            LedgerState::V9 {
                ledger_state,
                block_fullness,
            } => {
                let mut ledger_state = Sp::new(ledger_state);
                ledger_state.persist();
                default_storage::<v1_1::LedgerDb>().with_backend(|b| b.flush_all_changes_to_db());

                let key = ledger_state
                    .as_typed_key()
                    .serialize()
                    .map_err(|error| Error::Serialize("TypedArenaKeyV9", error))?;

                let ledger_state = Sp::into_inner(ledger_state).expect("ledger state exists");
                let ledger_state = LedgerState::V9 {
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
            Self::V8 {
                ledger_state,
                block_fullness,
            } => {
                let transaction =
                    tagged_deserialize::<TransactionV8<v1_1::LedgerDb>>(&mut transaction.as_ref())
                        .map_err(|error| Error::Deserialize("LedgerTransactionV8", error))?;

                let cx = TransactionContextV8 {
                    ref_state: ledger_state.clone(),
                    block_context: BlockContextV3 {
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
                let fees = transaction
                    .fees(&ledger_state.parameters, true)
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
                    fees,
                    // The bridge relies on ledger 9 primitives, so a `CardanoBridge` claim cannot
                    // occur on a ledger 8 chain; there is never a bridge claim to extract here.
                    bridge_claim: None,
                })
            }

            Self::V9 {
                ledger_state,
                block_fullness,
            } => {
                let transaction =
                    tagged_deserialize::<TransactionV9<v1_1::LedgerDb>>(&mut transaction.as_ref())
                        .map_err(|error| Error::Deserialize("LedgerTransactionV9", error))?;

                let cx = TransactionContextV9 {
                    ref_state: ledger_state.clone(),
                    block_context: BlockContextV4 {
                        tblock: timestamp(block_timestamp),
                        tblock_err: 30,
                        parent_block_hash: HashOutput(parent_block_hash.0),
                        last_block_time: timestamp(parent_block_timestamp),
                    },
                    whitelist: None,
                };

                // The stateless `cost` estimates verifier-key sizes, under-costing
                // contract calls; `cost_with_state` reads them from the ledger state.
                // The ledger has no state-aware `fees`, so the fee is computed from
                // the cost the same way `Transaction::fees` does.
                let cost = transaction
                    .cost_with_state(&ledger_state.parameters, ledger_state, true)
                    .map_err(|error| Error::TransactionCost(error.into()))?;
                let fees = {
                    let normalized = cost
                        .normalize(ledger_state.parameters.limits.block_limits)
                        .ok_or(FeeCalculationErrorV9::BlockLimitExceeded)
                        .map_err(|error| Error::TransactionCost(error.into()))?;
                    ledger_state
                        .parameters
                        .fee_prices
                        .overall_cost(&normalized)
                        .into_atomic_units(SPECKS_PER_DUST_V9)
                };
                let verified_ledger_transaction = transaction
                    .well_formed(&cx.ref_state, *STRICTNESS_V9, cx.block_context.tblock)
                    .map_err(|error| Error::MalformedTransaction(error.into()))?;
                let (ledger_state, transaction_result) =
                    ledger_state.apply(&verified_ledger_transaction, &cx);

                let (transaction_result, events, should_count_cost) = match transaction_result {
                    TransactionResultV9::Success(events) => {
                        (TransactionResult::Success, events, true)
                    }

                    TransactionResultV9::PartialSuccess(segments, events) => {
                        let segments = segments
                            .into_iter()
                            .map(|(id, result)| (id, result.is_ok()))
                            .collect::<Vec<_>>();
                        (TransactionResult::PartialSuccess(segments), events, true)
                    }

                    TransactionResultV9::Failure(_) => (TransactionResult::Failure, vec![], false),
                };

                // Only count cost for successful/partial transactions (match node behavior)
                let block_fullness = if should_count_cost {
                    *block_fullness + cost
                } else {
                    *block_fullness
                };

                // Extract a Cardano-bridge claim before `transaction` is moved into
                // `make_unshielded_utxos_for_regular_transaction_v9`. A `ClaimRewards` with
                // `ClaimKind::CardanoBridge` is a user claiming bridged NIGHT: the recipient is the
                // claim owner and the amount is the claim value.
                let bridge_claim = match &transaction {
                    TransactionV9::ClaimRewards(claim)
                        if claim.kind == ClaimKindV9::CardanoBridge =>
                    {
                        Some(BridgeClaim {
                            recipient: UserAddressV9::from(claim.owner.clone()).0.0.into(),
                            amount: claim.value,
                        })
                    }
                    _ => None,
                };

                let (created_unshielded_utxos, spent_unshielded_utxos) =
                    make_unshielded_utxos_for_regular_transaction_v9(
                        transaction,
                        &transaction_result,
                        &ledger_state,
                    );

                let ledger_events = make_ledger_events_v9(events)?;

                *self = Self::V9 {
                    ledger_state,
                    block_fullness,
                };

                Ok(ApplyRegularTransactionOutcome {
                    transaction_result,
                    created_unshielded_utxos,
                    spent_unshielded_utxos,
                    ledger_events,
                    fees,
                    bridge_claim,
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

            Self::V9 {
                ledger_state,
                block_fullness,
            } => {
                let transaction =
                    tagged_deserialize::<SystemTransactionV9>(&mut transaction.as_ref())
                        .map_err(|error| Error::Deserialize("SystemTransactionV9", error))?;

                let cost = transaction.cost(&ledger_state.parameters);
                let (ledger_state, events) = ledger_state
                    .apply_system_tx(&transaction, timestamp(block_timestamp))
                    .map_err(|error| Error::SystemTransaction(error.into()))?;
                let block_fullness = *block_fullness + cost;

                let created_unshielded_utxos =
                    make_unshielded_utxos_for_system_transaction_v9(transaction, &ledger_state);

                let ledger_events = make_ledger_events_v9(events)?;

                *self = Self::V9 {
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
            Self::V8 { ledger_state, .. } => ledger_state.zswap.first_free,
            Self::V9 { ledger_state, .. } => ledger_state.zswap.first_free,
        }
    }

    /// Get the first free index of the dust commitment tree.
    pub fn dust_commitments_first_free(&self) -> u64 {
        match self {
            Self::V8 { ledger_state, .. } => ledger_state.dust.utxo.commitments_first_free,
            Self::V9 { ledger_state, .. } => ledger_state.dust.utxo.commitments_first_free,
        }
    }

    /// Get the first free index of the dust generation tree.
    pub fn dust_generations_first_free(&self) -> u64 {
        match self {
            Self::V8 { ledger_state, .. } => {
                ledger_state.dust.generation.generating_tree_first_free
            }
            Self::V9 { ledger_state, .. } => {
                ledger_state.dust.generation.generating_tree_first_free
            }
        }
    }

    /// Get the Merkle tree root of the zswap state.
    pub fn zswap_merkle_tree_root(&self) -> ZswapMerkleTreeRoot {
        match self {
            Self::V8 { ledger_state, .. } => {
                let root = ledger_state
                    .zswap
                    .coin_coms
                    .rehash()
                    .root()
                    .expect("zswap state Merkle tree root should exist");
                ZswapMerkleTreeRoot::V8(root)
            }
            Self::V9 { ledger_state, .. } => {
                let root = ledger_state
                    .zswap
                    .coin_coms
                    .rehash()
                    .root()
                    .expect("zswap state Merkle tree root should exist");
                ZswapMerkleTreeRoot::V9(root)
            }
        }
    }

    /// Get the serialized merkle tree root of the dust commitment tree.
    pub fn dust_commitment_merkle_tree_root(&self) -> Result<ByteVec, Error> {
        match self {
            Self::V8 { ledger_state, .. } => ledger_state
                .dust
                .utxo
                .commitments
                .rehash()
                .root()
                .expect("dust commitment merkle tree root should exist")
                .serialize()
                .map_err(|error| Error::Serialize("DustCommitmentMerkleTreeRoot", error)),
            Self::V9 { ledger_state, .. } => ledger_state
                .dust
                .utxo
                .commitments
                .rehash()
                .root()
                .expect("dust commitment merkle tree root should exist")
                .serialize()
                .map_err(|error| Error::Serialize("DustCommitmentMerkleTreeRoot", error)),
        }
    }

    /// Get the serialized merkle tree root of the dust generation tree.
    pub fn dust_generation_merkle_tree_root(&self) -> Result<ByteVec, Error> {
        match self {
            Self::V8 { ledger_state, .. } => ledger_state
                .dust
                .generation
                .generating_tree
                .rehash()
                .root()
                .expect("dust generation merkle tree root should exist")
                .serialize()
                .map_err(|error| Error::Serialize("DustGenerationMerkleTreeRoot", error)),
            Self::V9 { ledger_state, .. } => ledger_state
                .dust
                .generation
                .generating_tree
                .rehash()
                .root()
                .expect("dust generation merkle tree root should exist")
                .serialize()
                .map_err(|error| Error::Serialize("DustGenerationMerkleTreeRoot", error)),
        }
    }

    /// Extract the zswap state for the given contract address.
    #[trace(properties = { "address": "{address}" })]
    pub fn extract_contract_zswap_state(
        &self,
        address: &SerializedContractAddress,
    ) -> Result<SerializedZswapState, Error> {
        match self {
            Self::V8 { ledger_state, .. } => {
                let address = ContractAddressV8::deserialize(&mut address.as_ref(), 0)
                    .map_err(|error| Error::Deserialize("ContractAddressV8", error))?;

                let mut contract_zswap_state = ZswapStateV8::new();
                contract_zswap_state.coin_coms = ledger_state.zswap.filter(&[address]);

                contract_zswap_state
                    .tagged_serialize()
                    .map_err(|error| Error::Serialize("ZswapStateV8", error))
            }

            Self::V9 { ledger_state, .. } => {
                let address = ContractAddressV9::deserialize(&mut address.as_ref(), 0)
                    .map_err(|error| Error::Deserialize("ContractAddressV9", error))?;

                let mut contract_zswap_state = ZswapStateV9::new();
                contract_zswap_state.coin_coms = ledger_state.zswap.filter(&[address]);

                contract_zswap_state
                    .tagged_serialize()
                    .map_err(|error| Error::Serialize("ZswapStateV9", error))
            }
        }
    }

    /// Create a zswap state Merkle tree collapsed update.
    pub fn make_zswap_collapsed_update(
        &self,
        start_index: u64,
        end_index: u64,
    ) -> Result<ByteVec, Error> {
        match self {
            Self::V8 { ledger_state, .. } => MerkleTreeCollapsedUpdate::new(
                &ledger_state.zswap.coin_coms.rehash(),
                start_index,
                end_index,
            )
            .map_err(|error| Error::InvalidUpdate(error.into()))?
            .tagged_serialize()
            .map_err(|error| Error::Serialize("MerkleTreeCollapsedUpdate", error)),
            Self::V9 { ledger_state, .. } => MerkleTreeCollapsedUpdateV9::new(
                &ledger_state.zswap.coin_coms.rehash(),
                start_index,
                end_index,
            )
            .map_err(|error| Error::InvalidUpdate(error.into()))?
            .tagged_serialize()
            .map_err(|error| Error::Serialize("MerkleTreeCollapsedUpdate", error)),
        }
    }

    /// Get the serialized dust generations merkle-tree collapsed update for the given indices.
    pub fn dust_generations_collapsed_update(
        &self,
        start_index: u64,
        end_index: u64,
    ) -> Result<ByteVec, Error> {
        match self {
            Self::V8 { ledger_state, .. } => MerkleTreeCollapsedUpdate::new(
                &ledger_state.dust.generation.generating_tree.rehash(),
                start_index,
                end_index,
            )
            .map_err(|error| Error::InvalidUpdate(error.into()))?
            .tagged_serialize()
            .map_err(|error| Error::Serialize("DustGenerationsMerkleTreeCollapsedUpdate", error)),
            Self::V9 { ledger_state, .. } => MerkleTreeCollapsedUpdateV9::new(
                &ledger_state.dust.generation.generating_tree.rehash(),
                start_index,
                end_index,
            )
            .map_err(|error| Error::InvalidUpdate(error.into()))?
            .tagged_serialize()
            .map_err(|error| Error::Serialize("DustGenerationsMerkleTreeCollapsedUpdate", error)),
        }
    }

    /// Get the serialized dust commitments merkle-tree collapsed update for the given indices.
    pub fn dust_commitments_collapsed_update(
        &self,
        start_index: u64,
        end_index: u64,
    ) -> Result<ByteVec, Error> {
        match self {
            Self::V8 { ledger_state, .. } => MerkleTreeCollapsedUpdate::new(
                &ledger_state.dust.utxo.commitments.rehash(),
                start_index,
                end_index,
            )
            .map_err(|error| Error::InvalidUpdate(error.into()))?
            .tagged_serialize()
            .map_err(|error| Error::Serialize("DustCommitmentsMerkleTreeCollapsedUpdate", error)),
            Self::V9 { ledger_state, .. } => MerkleTreeCollapsedUpdateV9::new(
                &ledger_state.dust.utxo.commitments.rehash(),
                start_index,
                end_index,
            )
            .map_err(|error| Error::InvalidUpdate(error.into()))?
            .tagged_serialize()
            .map_err(|error| Error::Serialize("DustCommitmentsMerkleTreeCollapsedUpdate", error)),
        }
    }

    /// To be called after applying transactions.
    pub fn finalize_apply_transactions(
        &mut self,
        block_timestamp: u64,
    ) -> Result<LedgerParameters, Error> {
        match self {
            Self::V8 {
                ledger_state,
                block_fullness,
            } => {
                let timestamp = timestamp(block_timestamp);
                let block_limits = ledger_state.parameters.limits.block_limits;
                let normalized_fullness =
                    clamp_and_normalize(block_fullness, &block_limits, "post_block_update");
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

            Self::V9 {
                ledger_state,
                block_fullness,
            } => {
                let timestamp = timestamp(block_timestamp);
                let block_limits = ledger_state.parameters.limits.block_limits;
                let normalized_fullness =
                    clamp_and_normalize(block_fullness, &block_limits, "post_block_update");
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

                *self = Self::V9 {
                    ledger_state,
                    block_fullness: Default::default(),
                };

                Ok(LedgerParameters::V9(ledger_parameters))
            }
        }
    }
}

/// Facade for ledger parameters across supported (protocol) versions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerParameters {
    V8(LedgerParametersV8),
    V9(LedgerParametersV9),
}

impl LedgerParameters {
    /// Serialize these ledger parameters.
    #[trace]
    pub fn serialize(&self) -> Result<SerializedLedgerParameters, Error> {
        match self {
            Self::V8(parameters) => parameters
                .tagged_serialize()
                .map_err(|error| Error::Serialize("SerializedLedgerParametersV8", error)),
            Self::V9(parameters) => parameters
                .tagged_serialize()
                .map_err(|error| Error::Serialize("SerializedLedgerParametersV9", error)),
        }
    }
}

/// Facade for zswap state Merkle tree root across supported (protocol) versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZswapMerkleTreeRoot {
    V8(MerkleTreeDigest),
    V9(MerkleTreeDigestV9),
}

impl ZswapMerkleTreeRoot {
    /// Untagged deserialize the given serialized zswap state root using the given protocol version.
    #[trace(properties = { "ledger_version": "{ledger_version}" })]
    pub fn deserialize(
        zswap_state_root: impl AsRef<[u8]>,
        ledger_version: LedgerVersion,
    ) -> Result<Self, Error> {
        let zswap_state_root = match ledger_version {
            LedgerVersion::V8 => {
                let digest = MerkleTreeDigest::deserialize(&mut zswap_state_root.as_ref(), 0)
                    .map_err(|error| Error::Deserialize("MerkleTreeDigestV8", error))?;
                Self::V8(digest)
            }
            LedgerVersion::V9 => {
                let digest = MerkleTreeDigestV9::deserialize(&mut zswap_state_root.as_ref(), 0)
                    .map_err(|error| Error::Deserialize("MerkleTreeDigestV9", error))?;
                Self::V9(digest)
            }
        };

        Ok(zswap_state_root)
    }

    /// Serialize this zswap state root.
    #[trace]
    pub fn serialize(&self) -> Result<SerializedZswapMerkleTreeRoot, Error> {
        match self {
            Self::V8(digest) => digest
                .serialize()
                .map_err(|error| Error::Serialize("MerkleTreeDigestV8", error)),
            Self::V9(digest) => digest
                .serialize()
                .map_err(|error| Error::Serialize("MerkleTreeDigestV9", error)),
        }
    }
}

fn timestamp(block_timestamp: u64) -> Timestamp {
    Timestamp::from_secs(block_timestamp / 1000)
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
            EventDetailsV8::ZswapInput { nullifier, .. } => Some(Ok(LedgerEvent::zswap_input(
                raw,
                nullifier.0.0.to_vec().into(),
            ))),

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

            EventDetailsV8::DustSpendProcessed {
                nullifier,
                commitment,
                ..
            } => Some(Ok(LedgerEvent::dust_spend_processed(
                raw,
                nullifier.0.0.to_bytes_le().to_vec().into(),
                commitment.0.0.to_bytes_le().to_vec().into(),
            ))),

            other => Some(Err(Error::UnsupportedEventVariant(format!("{other:?}")))),
        })
        .flatten()
        .collect::<Result<_, _>>()
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

fn make_dust_generation_dtime_update_v8(
    update: TreeInsertionPath<DustGenerationInfoV8>,
    raw: ByteVec,
) -> Result<LedgerEvent, Error> {
    let generation = &update.leaf.1;

    let owner = generation
        .owner
        .serialize()
        .map_err(|error| Error::Serialize("DustPublicKeyV8", error))?;

    let generation_info = dust::DustGenerationInfo {
        night_utxo_hash: generation.nonce.0.0.into(),
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

    let tree_insertion_path = update
        .tagged_serialize()
        .map_err(|error| Error::Serialize("TreeInsertionPath<DustGenerationInfoV8>", error))?;

    Ok(LedgerEvent::dust_generation_dtime_update(
        raw,
        generation_info,
        mt_index,
        tree_insertion_path,
    ))
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
            let owner = UserAddress::from(claim.owner);
            let ledger_intent_hash = {
                // ClaimRewards don't have intents, but UTXOs need an intent hash. We compute this
                // hash the same way that the ledger does internally.
                let output = OutputInstructionUnshieldedV8 {
                    amount: claim.value,
                    target_address: owner,
                    nonce: claim.nonce,
                };
                output.mk_intent_hash(NIGHT)
            };
            let intent_hash = ledger_intent_hash.0.0.into();
            let initial_nonce = make_initial_nonce_v8(OUTPUT_INDEX_ZERO, intent_hash);
            let registered_for_dust_generation =
                registered_for_dust_generation_v8(OUTPUT_INDEX_ZERO, intent_hash, ledger_state);
            let utxo = UtxoV8 {
                value: claim.value,
                owner,
                type_: UnshieldedTokenType::default(),
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
            owner: UserAddress::from(spend.owner.clone()),
            type_: spend.type_,
            intent_hash: spend.intent_hash,
            output_no: spend.output_no,
        };

        UnshieldedUtxo {
            owner: UserAddress::from(spend.owner).0.0.into(),
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

fn make_initial_nonce_v8(output_index: u32, intent_hash: IntentHash) -> Nonce {
    let intent_hash = HashOutput(intent_hash.0);
    let initial_nonce = InitialNonceV8(persistent_commit(&output_index, intent_hash));
    ByteArray(initial_nonce.0.0)
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

// --- ledger v9 mirrors of the above helpers ---
//
// base-crypto, coin-structure and transient-crypto are unified across v8 and
// v9, so the shared domain / cost / merkle types are reused; only the ledger
// (events, dust, structure) types are v9-specific.

fn make_ledger_events_v9<D>(events: Vec<EventV9<D>>) -> Result<Vec<LedgerEvent>, Error>
where
    D: DB,
{
    events
        .into_iter()
        .map(|event| {
            let raw = event
                .tagged_serialize()
                .map_err(|error| Error::Serialize("EventV9", error))?;
            Ok::<_, Error>((event, raw))
        })
        .filter_map_ok(|(event, raw)| match event.content {
            EventDetailsV9::ZswapInput { nullifier, .. } => Some(Ok(LedgerEvent::zswap_input(
                raw,
                nullifier.0.0.to_vec().into(),
            ))),

            EventDetailsV9::ZswapOutput { .. } => Some(Ok(LedgerEvent::zswap_output(raw))),

            EventDetailsV9::ContractDeploy { .. } => None,

            EventDetailsV9::ContractLog {
                address,
                entry_point,
                logged_item,
            } => {
                let attributes = make_contract_event_attributes(&logged_item, entry_point);
                Some(Ok(LedgerEvent::contract_event(
                    raw,
                    address.0.0.to_vec().into(),
                    None,
                    attributes,
                )))
            }

            EventDetailsV9::ParamChange(..) => Some(Ok(LedgerEvent::param_change(raw))),

            EventDetailsV9::DustInitialUtxo {
                output,
                generation,
                generation_index,
                ..
            } => Some(make_dust_initial_utxo_v9(
                output,
                generation,
                generation_index,
                raw,
            )),

            EventDetailsV9::DustGenerationDtimeUpdate { update, .. } => {
                Some(make_dust_generation_dtime_update_v9(update, raw))
            }

            EventDetailsV9::DustSpendProcessed {
                nullifier,
                commitment,
                ..
            } => Some(Ok(LedgerEvent::dust_spend_processed(
                raw,
                nullifier.0.0.to_bytes_le().to_vec().into(),
                commitment.0.0.to_bytes_le().to_vec().into(),
            ))),

            other => Some(Err(Error::UnsupportedEventVariant(format!("{other:?}")))),
        })
        .flatten()
        .collect::<Result<_, _>>()
}

/// Map a v9 `VersionedLogItem` to the corresponding `LedgerEventAttributes`
/// variant based on its `LogEventType` and decode the per-event payload from
/// `StateValue<D>`. The decoder follows the CoIP-442 + MIP-0002 spec exactly.
///
/// Wire format assumptions (verified against the onchain-vm
/// `try_decode_event` path, Compact compiler `serialize<T, n>` circuit, and
/// the `midnight-events.ss` per-event size table):
/// - `VersionedLogItem.data` is `StateValue::Cell(AlignedValue)` with a single `ValueAtom`
///   containing the flat concatenated bytes of the event struct.
/// - `ValueAtom` strips trailing zeros; the decoder pads back to the expected size before slicing.
/// - `Bytes<N>` = N raw bytes.
/// - `Uint<128>` = 16 bytes, little-endian.
/// - `Maybe<T>` = 1 tag byte (0=None, 1=Some) + sizeof(T) value bytes; value is zeroed in the wire
///   when `is_some=false`.
/// - `Either<A,B>` = 1 tag byte (0=Left/User, 1=Right/Contract) + sizeof(max(A,B)) value bytes.
///
/// If the wire shape diverges (non-Cell, wrong size, multi-atom, etc.), the
/// decoder logs a warning and returns the variant with empty/default payload
/// fields, so the event still flows through to the events surface without
/// silent data corruption.
fn make_contract_event_attributes<D>(
    item: &VersionedLogItem<D>,
    entry_point: EntryPointBuf,
) -> LedgerEventAttributes
where
    D: DB,
{
    let version = item.version;
    let entry_point: ByteVec = entry_point.0.into();
    // Per-event minimum atom-bytes lengths after Compact's trailing-zero
    // stripping. Set so that an emission whose only-trailing-zero stripping
    // is the final value field still decodes correctly, while an emission
    // with a missing (truncated) leading field falls back. Crucial for the
    // UnshieldedSpend / UnshieldedReceive spec/Compact divergence: spec is
    // 113 bytes, Compact issue-377 is 81 bytes; min 97 (=113-16, only the
    // u128 amount fully stripped) rejects 81-byte emissions cleanly.
    match item.event_type {
        LogEventType::ShieldedSpend => {
            // nullifier is a 32-byte hash; require all 32 bytes.
            let bytes = extract_flat_bytes(&item.data, SHIELDED_SPEND_SIZE, SHIELDED_SPEND_SIZE);
            let nullifier = bytes
                .and_then(|bytes| take_bytes(&bytes, 0, BYTES_32_SIZE))
                .unwrap_or_default();
            LedgerEventAttributes::ContractShieldedSpend {
                version,
                entry_point,
                nullifier,
            }
        }
        LogEventType::ShieldedReceive => {
            // Canonical MIP-0002 (mips/mip-0002-public-contract-log-emission.md
            // Appendix A on main): (commitment, ciphertext: Maybe<Bytes<512>>,
            // contractAddress: Maybe<ContractAddress>). The CoIP-442 head agrees
            // (commit e537fc9 "Reorder ShieldedReceive fields"). Compact issue-377
            // currently emits the older (commitment, contractAddress, ciphertext)
            // order; that's a Compact-side catch-up issue, not the indexer's.
            //
            // Trailing-zero stripping can strip the entire trailing
            // contractAddress (33 bytes) + the ciphertext value bytes
            // (up to 512), so min = 32 (commitment only).
            let bytes = extract_flat_bytes(&item.data, BYTES_32_SIZE, SHIELDED_RECEIVE_SIZE);
            let commitment = bytes
                .as_deref()
                .and_then(|b| take_bytes(b, 0, BYTES_32_SIZE))
                .unwrap_or_default();
            let ciphertext = bytes
                .as_deref()
                .and_then(|b| take_maybe_bytes(b, BYTES_32_SIZE, 512));
            let receiving_contract_address = bytes
                .as_deref()
                .and_then(|b| take_maybe_bytes(b, BYTES_32_SIZE + MAYBE_512_SIZE, BYTES_32_SIZE));
            LedgerEventAttributes::ContractShieldedReceive {
                version,
                entry_point,
                commitment,
                ciphertext,
                receiving_contract_address,
            }
        }
        LogEventType::ShieldedMint => {
            // (commitment 32, domain_sep 32, amount Maybe<Uint128> 17).
            // Min = 32+32 (commitment+domain_sep) since amount Maybe can be
            // fully stripped (tag byte 0 + 16 zero bytes = 17 strippable).
            let bytes = extract_flat_bytes(&item.data, 2 * BYTES_32_SIZE, SHIELDED_MINT_SIZE);
            let commitment = bytes
                .as_deref()
                .and_then(|b| take_bytes(b, 0, BYTES_32_SIZE))
                .unwrap_or_default();
            let domain_sep = bytes
                .as_deref()
                .and_then(|b| take_bytes(b, BYTES_32_SIZE, BYTES_32_SIZE))
                .unwrap_or_default();
            let amount = bytes
                .as_deref()
                .and_then(|b| take_maybe_uint_128_le(b, 2 * BYTES_32_SIZE));
            LedgerEventAttributes::ContractShieldedMint {
                version,
                entry_point,
                commitment,
                domain_sep,
                amount,
            }
        }
        LogEventType::ShieldedBurn => {
            // (nullifier 32, amount Maybe<Uint128> 17). Min = 32.
            let bytes = extract_flat_bytes(&item.data, BYTES_32_SIZE, SHIELDED_BURN_SIZE);
            let nullifier = bytes
                .as_deref()
                .and_then(|b| take_bytes(b, 0, BYTES_32_SIZE))
                .unwrap_or_default();
            let amount = bytes
                .as_deref()
                .and_then(|b| take_maybe_uint_128_le(b, BYTES_32_SIZE));
            LedgerEventAttributes::ContractShieldedBurn {
                version,
                entry_point,
                nullifier,
                amount,
            }
        }
        LogEventType::UnshieldedSpend => {
            // (sender Either 65, domain_sep 32, token_type 32, amount u128 16). 145 bytes.
            // Min = 129 (= 145 - 16, only the amount u128 fully stripped to zero).
            let bytes = extract_flat_bytes(
                &item.data,
                UNSHIELDED_SPEND_SIZE - UINT_128_SIZE,
                UNSHIELDED_SPEND_SIZE,
            );
            let sender = bytes
                .as_deref()
                .and_then(|b| take_either_address(b, 0))
                .unwrap_or_else(|| AddressOrContract::User(ByteVec::default()));
            let domain_sep = bytes
                .as_deref()
                .and_then(|b| take_bytes(b, EITHER_SIZE, BYTES_32_SIZE))
                .unwrap_or_default();
            let token_type = bytes
                .as_deref()
                .and_then(|b| take_bytes(b, EITHER_SIZE + BYTES_32_SIZE, BYTES_32_SIZE))
                .unwrap_or_default();
            let amount = bytes
                .as_deref()
                .and_then(|b| take_uint_128_le(b, EITHER_SIZE + 2 * BYTES_32_SIZE))
                .unwrap_or_else(|| "0".to_string());
            LedgerEventAttributes::ContractUnshieldedSpend {
                version,
                entry_point,
                sender,
                domain_sep,
                token_type,
                amount,
            }
        }
        LogEventType::UnshieldedReceive => {
            // Same shape and min as UnshieldedSpend (recipient instead of sender).
            let bytes = extract_flat_bytes(
                &item.data,
                UNSHIELDED_RECEIVE_SIZE - UINT_128_SIZE,
                UNSHIELDED_RECEIVE_SIZE,
            );
            let recipient = bytes
                .as_deref()
                .and_then(|b| take_either_address(b, 0))
                .unwrap_or_else(|| AddressOrContract::User(ByteVec::default()));
            let domain_sep = bytes
                .as_deref()
                .and_then(|b| take_bytes(b, EITHER_SIZE, BYTES_32_SIZE))
                .unwrap_or_default();
            let token_type = bytes
                .as_deref()
                .and_then(|b| take_bytes(b, EITHER_SIZE + BYTES_32_SIZE, BYTES_32_SIZE))
                .unwrap_or_default();
            let amount = bytes
                .as_deref()
                .and_then(|b| take_uint_128_le(b, EITHER_SIZE + 2 * BYTES_32_SIZE))
                .unwrap_or_else(|| "0".to_string());
            LedgerEventAttributes::ContractUnshieldedReceive {
                version,
                entry_point,
                recipient,
                domain_sep,
                token_type,
                amount,
            }
        }
        LogEventType::UnshieldedMint => {
            // (domain_sep 32, token_type 32, amount u128 16). 80 bytes.
            // Min = 64 (amount fully stripped to zero).
            let bytes = extract_flat_bytes(
                &item.data,
                UNSHIELDED_MINT_SIZE - UINT_128_SIZE,
                UNSHIELDED_MINT_SIZE,
            );
            let domain_sep = bytes
                .as_deref()
                .and_then(|b| take_bytes(b, 0, BYTES_32_SIZE))
                .unwrap_or_default();
            let token_type = bytes
                .as_deref()
                .and_then(|b| take_bytes(b, BYTES_32_SIZE, BYTES_32_SIZE))
                .unwrap_or_default();
            let amount = bytes
                .as_deref()
                .and_then(|b| take_uint_128_le(b, 2 * BYTES_32_SIZE))
                .unwrap_or_else(|| "0".to_string());
            LedgerEventAttributes::ContractUnshieldedMint {
                version,
                entry_point,
                domain_sep,
                token_type,
                amount,
            }
        }
        LogEventType::UnshieldedBurn => {
            // (sender Either 65, token_type 32, amount u128 16). 113 bytes.
            // Min = 97 (amount fully stripped to zero).
            let bytes = extract_flat_bytes(
                &item.data,
                UNSHIELDED_BURN_SIZE - UINT_128_SIZE,
                UNSHIELDED_BURN_SIZE,
            );
            let sender = bytes
                .as_deref()
                .and_then(|b| take_either_address(b, 0))
                .unwrap_or_else(|| AddressOrContract::User(ByteVec::default()));
            let token_type = bytes
                .as_deref()
                .and_then(|b| take_bytes(b, EITHER_SIZE, BYTES_32_SIZE))
                .unwrap_or_default();
            let amount = bytes
                .as_deref()
                .and_then(|b| take_uint_128_le(b, EITHER_SIZE + BYTES_32_SIZE))
                .unwrap_or_else(|| "0".to_string());
            LedgerEventAttributes::ContractUnshieldedBurn {
                version,
                entry_point,
                sender,
                token_type,
                amount,
            }
        }
        LogEventType::Paused => LedgerEventAttributes::ContractPaused {
            version,
            entry_point,
        },
        LogEventType::Unpaused => LedgerEventAttributes::ContractUnpaused {
            version,
            entry_point,
        },
        LogEventType::Misc => {
            // (name 32, payload 256). Min = 32 (payload all-zero strippable).
            let bytes = extract_flat_bytes(&item.data, BYTES_32_SIZE, MISC_SIZE);
            let name = bytes
                .as_deref()
                .and_then(|b| take_bytes(b, 0, BYTES_32_SIZE))
                .unwrap_or_default();
            let payload = bytes
                .as_deref()
                .and_then(|b| take_bytes(b, BYTES_32_SIZE, 256))
                .unwrap_or_default();
            LedgerEventAttributes::ContractMisc {
                version,
                entry_point,
                name,
                payload,
            }
        }
    }
}

/// Extract a `Vec<u8>` of exactly `max` bytes from a `StateValue::Cell`,
/// padding with trailing zeros if Compact stripped them on the wire. Returns
/// `None` on any structural mismatch:
/// - non-Cell `StateValue`
/// - multi-atom `AlignedValue` (Compact's `serialize<T, n>` produces a single atom for the flat
///   byte payload)
/// - atom longer than `max` (oversize — wrong event type or unexpected layout)
/// - atom shorter than `min` (undersize — likely a different event-struct layout, e.g. spec/Compact
///   divergence on UnshieldedSpend/Receive where Compact issue-377 emits 81 bytes vs the spec's
///   113-byte layout)
///
/// `min` is the per-event minimum atom-byte length after maximum trailing-zero
/// stripping of the last variable-width field. `max` is the canonical full
/// size per CoIP-442 + MIP-0002.
fn extract_flat_bytes<D>(data: &StateValue<D>, min: usize, max: usize) -> Option<Vec<u8>>
where
    D: DB,
{
    let aligned = match data {
        StateValue::Cell(sp) => sp,
        other => {
            let got = std::mem::discriminant(other);
            warn!(got:?; "contract log data: expected StateValue::Cell");
            return None;
        }
    };
    let atoms = aligned.value.0.len();
    if atoms != 1 {
        warn!(atoms; "contract log data: expected single ValueAtom");
        return None;
    }
    let atom_bytes = &aligned.value.0[0].0;
    let atom_len = atom_bytes.len();
    if atom_len > max {
        warn!(atom_len, max; "contract log data: atom length exceeds expected max");
        return None;
    }
    if atom_len < min {
        warn!(atom_len, min; "contract log data: atom length below expected min, likely wrong event-struct layout");
        return None;
    }
    let mut buf = vec![0u8; max];
    buf[..atom_len].copy_from_slice(atom_bytes);
    Some(buf)
}

fn take_bytes(bytes: &[u8], offset: usize, len: usize) -> Option<ByteVec> {
    bytes.get(offset..offset + len).map(|s| s.to_vec().into())
}

fn take_uint_128_le(bytes: &[u8], offset: usize) -> Option<String> {
    let slice: [u8; UINT_128_SIZE] = bytes.get(offset..offset + UINT_128_SIZE)?.try_into().ok()?;
    Some(u128::from_le_bytes(slice).to_string())
}

fn take_maybe_bytes(bytes: &[u8], offset: usize, value_len: usize) -> Option<ByteVec> {
    let tag = *bytes.get(offset)?;
    if tag == 0 {
        return None;
    }
    bytes
        .get(offset + 1..offset + 1 + value_len)
        .map(|s| s.to_vec().into())
}

fn take_maybe_uint_128_le(bytes: &[u8], offset: usize) -> Option<String> {
    let tag = *bytes.get(offset)?;
    if tag == 0 {
        return None;
    }
    let slice: [u8; UINT_128_SIZE] = bytes
        .get(offset + 1..offset + 1 + UINT_128_SIZE)?
        .try_into()
        .ok()?;
    Some(u128::from_le_bytes(slice).to_string())
}

// Decode an `Either<ZswapCoinPublicKey, ContractAddress>` address. Compact serialises `Either<A,B>`
// as the full struct `{ is_left: Boolean, left: A, right: B }`, so on the wire it is 65 bytes:
// `[is_left:1][left:32][right:32]` with both slots present and the inactive one zero-filled
// (`compiler/standard-library.compact`, where `left(v) = { is_left: true, left: v, right: default
// }`). `left` is `ZswapCoinPublicKey` (a user key) and `right` is `ContractAddress`, so `is_left`
// selects the variant and which slot holds the value.
fn take_either_address(bytes: &[u8], offset: usize) -> Option<AddressOrContract> {
    let is_left = *bytes.get(offset)?;
    let left = offset + 1;
    let right = left + BYTES_32_SIZE;
    Some(if is_left == 0 {
        let value = bytes.get(right..right + BYTES_32_SIZE)?.to_vec().into();
        AddressOrContract::Contract(value)
    } else {
        let value = bytes.get(left..left + BYTES_32_SIZE)?.to_vec().into();
        AddressOrContract::User(value)
    })
}

fn make_dust_initial_utxo_v9(
    output: QualifiedDustOutputV9,
    generation: DustGenerationInfoV9,
    generation_index: u64,
    raw: ByteVec,
) -> Result<LedgerEvent, Error> {
    let owner = output
        .owner
        .serialize()
        .map_err(|error| Error::Serialize("DustPublicKeyV9", error))?;

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
        .map_err(|error| Error::Serialize("DustPublicKeyV9", error))?;

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

fn make_dust_generation_dtime_update_v9(
    update: TreeInsertionPathV9<DustGenerationInfoV9>,
    raw: ByteVec,
) -> Result<LedgerEvent, Error> {
    let generation = &update.leaf.1;

    let owner = generation
        .owner
        .serialize()
        .map_err(|error| Error::Serialize("DustPublicKeyV9", error))?;

    let generation_info = dust::DustGenerationInfo {
        night_utxo_hash: generation.nonce.0.0.into(),
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

    let tree_insertion_path = update
        .tagged_serialize()
        .map_err(|error| Error::Serialize("TreeInsertionPath<DustGenerationInfoV9>", error))?;

    Ok(LedgerEvent::dust_generation_dtime_update(
        raw,
        generation_info,
        mt_index,
        tree_insertion_path,
    ))
}

fn make_unshielded_utxos_for_regular_transaction_v9<D>(
    transaction: TransactionV9<D>,
    transaction_result: &TransactionResult,
    ledger_state: &LedgerStateV9<D>,
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
        TransactionV9::Standard(transaction) => {
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
                        extend_unshielded_utxos_v9(
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
                    extend_unshielded_utxos_v9(
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
        TransactionV9::ClaimRewards(claim) => {
            let owner = UserAddressV9::from(claim.owner);
            let ledger_intent_hash = {
                // ClaimRewards don't have intents, but UTXOs need an intent hash. We compute this
                // hash the same way that the ledger does internally.
                let output = OutputInstructionUnshieldedV9 {
                    amount: claim.value,
                    target_address: owner,
                    nonce: claim.nonce,
                };
                output.mk_intent_hash(NIGHT_V9)
            };
            let intent_hash = ledger_intent_hash.0.0.into();
            let initial_nonce = make_initial_nonce_v9(OUTPUT_INDEX_ZERO, intent_hash);
            let registered_for_dust_generation =
                registered_for_dust_generation_v9(OUTPUT_INDEX_ZERO, intent_hash, ledger_state);
            let utxo = UtxoV9 {
                value: claim.value,
                owner,
                type_: UnshieldedTokenTypeV9::default(),
                intent_hash: ledger_intent_hash,
                output_no: OUTPUT_INDEX_ZERO,
            };

            let utxo = UnshieldedUtxo {
                owner: owner.0.0.into(),
                token_type: TokenType::default(), // Native token (all zeros).
                value: claim.value,
                intent_hash,
                output_index: OUTPUT_INDEX_ZERO,
                ctime: ctime_v9(&utxo, ledger_state),
                initial_nonce,
                registered_for_dust_generation,
            };

            (vec![utxo], vec![]) // Creates one UTXO, spends none.
        }
    }
}

fn make_unshielded_utxos_for_system_transaction_v9<D>(
    transaction: SystemTransactionV9,
    ledger_state: &LedgerStateV9<D>,
) -> Vec<UnshieldedUtxo>
where
    D: DB,
{
    match transaction {
        SystemTransactionV9::PayFromTreasuryUnshielded {
            outputs,
            token_type,
        } => outputs
            .iter()
            .enumerate()
            .map(|(index, output)| {
                let ledger_intent_hash = output.clone().mk_intent_hash(token_type);
                let intent_hash = ledger_intent_hash.0.0.into();
                let initial_nonce = make_initial_nonce_v9(index as u32, intent_hash);
                let registered_for_dust_generation =
                    registered_for_dust_generation_v9(index as u32, intent_hash, ledger_state);
                let utxo = UtxoV9 {
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
                    ctime: ctime_v9(&utxo, ledger_state),
                    initial_nonce,
                    registered_for_dust_generation,
                }
            })
            .collect(),

        _ => vec![], // Other system transaction types don't create unshielded UTXOs.
    }
}

fn extend_unshielded_utxos_v9<D>(
    outputs: &mut Vec<UnshieldedUtxo>,
    inputs: &mut Vec<UnshieldedUtxo>,
    segment_id: u16,
    intent: &IntentV9<D>,
    guaranteed: bool,
    ledger_state: &LedgerStateV9<D>,
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
            let initial_nonce = make_initial_nonce_v9(output_index, intent_hash);
            let registered_for_dust_generation =
                registered_for_dust_generation_v9(output_index, intent_hash, ledger_state);
            let utxo = UtxoV9 {
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
                ctime: ctime_v9(&utxo, ledger_state),
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
        let initial_nonce = make_initial_nonce_v9(spend.output_no, intent_hash);
        let registered_for_dust_generation =
            registered_for_dust_generation_v9(spend.output_no, intent_hash, ledger_state);
        let utxo = UtxoV9 {
            value: spend.value,
            owner: UserAddressV9::from(spend.owner.clone()),
            type_: spend.type_,
            intent_hash: spend.intent_hash,
            output_no: spend.output_no,
        };

        UnshieldedUtxo {
            owner: UserAddressV9::from(spend.owner).0.0.into(),
            token_type: spend.type_.0.0.into(),
            value: spend.value,
            intent_hash,
            output_index: spend.output_no,
            ctime: ctime_v9(&utxo, ledger_state),
            initial_nonce,
            registered_for_dust_generation,
        }
    });
    inputs.extend(intent_inputs);
}

fn make_initial_nonce_v9(output_index: u32, intent_hash: IntentHash) -> Nonce {
    let intent_hash = HashOutput(intent_hash.0);
    let initial_nonce = InitialNonceV9(persistent_commit(&output_index, intent_hash));
    ByteArray(initial_nonce.0.0)
}

fn registered_for_dust_generation_v9<D>(
    output_index: u32,
    intent_hash: IntentHash,
    ledger_state: &LedgerStateV9<D>,
) -> bool
where
    D: DB,
{
    let intent_hash = HashOutput(intent_hash.0);
    let initial_nonce = InitialNonceV9(persistent_commit(&output_index, intent_hash));
    ledger_state
        .dust
        .generation
        .night_indices
        .contains_key(&initial_nonce)
}

fn ctime_v9<D>(utxo: &UtxoV9, ledger_state: &LedgerStateV9<D>) -> Option<u64>
where
    D: DB,
{
    ledger_state
        .utxo
        .utxos
        .get(utxo)
        .map(|meta| meta.ctime.to_secs())
}

/// Matches the node's `clamp_and_normalize`: falling back to `NormalizedCost::ZERO` on
/// overflow would drive `overall_price` opposite to the node and compound drift.
fn clamp_and_normalize(
    cost: &SyntheticCost,
    limits: &SyntheticCost,
    context: &str,
) -> NormalizedCost {
    let clamped = SyntheticCost {
        read_time: cost.read_time.min(limits.read_time),
        compute_time: cost.compute_time.min(limits.compute_time),
        block_usage: cost.block_usage.min(limits.block_usage),
        bytes_written: cost.bytes_written.min(limits.bytes_written),
        bytes_churned: cost.bytes_churned.min(limits.bytes_churned),
    };
    if clamped != *cost {
        error!(
            original:? = *cost,
            limits:? = *limits,
            context;
            "block fullness exceeded limits, clamping"
        );
    }
    clamped.normalize(*limits).expect("clamped cost normalises")
}

#[cfg(test)]
mod tests {
    use crate::{
        domain::{
            AddressOrContract, LedgerEventAttributes, LedgerVersion,
            ledger::{
                LedgerState,
                ledger_state::{make_contract_event_attributes, take_either_address},
            },
        },
        error::BoxError,
    };
    use anyhow::Context;
    use midnight_base_crypto_v1::{
        cost_model::SyntheticCost,
        fab::{AlignedValue, Alignment, Value, ValueAtom},
    };
    use midnight_onchain_runtime_v4::{
        ops::{LogEventType, VersionedLogItem},
        state::{EntryPointBuf, StateValue},
    };
    use midnight_storage_core_v1::{arena::Sp, db::InMemoryDB};

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

        let ledger_state = LedgerState::new("undeployed".try_into()?, LedgerVersion::V8)
            .expect("ledger state can be constructed");
        assert_eq!(ledger_state.ledger_version(), LedgerVersion::V8);

        let new_ledger_state = ledger_state
            .clone()
            .translate(LedgerVersion::V8)
            .expect("ledger state v8 can be translated to v8");
        assert_eq!(new_ledger_state, ledger_state);

        let ledger_state_v9 = LedgerState::new("undeployed".try_into()?, LedgerVersion::V9)
            .expect("ledger state v9 can be constructed");
        assert_eq!(ledger_state_v9.ledger_version(), LedgerVersion::V9);

        let new_ledger_state_v9 = ledger_state_v9
            .clone()
            .translate(LedgerVersion::V9)
            .expect("ledger state v9 can be translated to v9");
        assert_eq!(new_ledger_state_v9, ledger_state_v9);

        // Cross-version translations are unsupported in both directions.
        assert!(ledger_state.translate(LedgerVersion::V9).is_err());
        assert!(ledger_state_v9.translate(LedgerVersion::V8).is_err());

        Ok(())
    }

    /// Overflow in any dimension clamps to the corresponding limit; resulting `NormalizedCost`
    /// has each dim = 1.0. Regression guard for GH #1060: previously we used
    /// `.normalize().unwrap_or(NormalizedCost::ZERO)` which flipped the sign of the
    /// price adjustment relative to the node.
    #[test]
    fn test_clamp_and_normalize_overflow_normalises_to_one() {
        use super::clamp_and_normalize;
        use midnight_base_crypto_v1::cost_model::{CostDuration, FixedPoint};

        let limits = SyntheticCost {
            read_time: CostDuration::from_picoseconds(1_000),
            compute_time: CostDuration::from_picoseconds(1_000),
            block_usage: 1_000,
            bytes_written: 1_000,
            bytes_churned: 1_000,
        };
        let overfull = SyntheticCost {
            read_time: limits.read_time,
            compute_time: limits.compute_time,
            block_usage: limits.block_usage + 1,
            bytes_written: limits.bytes_written,
            bytes_churned: limits.bytes_churned,
        };

        let normalized = clamp_and_normalize(&overfull, &limits, "test");
        assert_eq!(normalized.read_time, FixedPoint::ONE);
        assert_eq!(normalized.compute_time, FixedPoint::ONE);
        assert_eq!(normalized.block_usage, FixedPoint::ONE);
        assert_eq!(normalized.bytes_written, FixedPoint::ONE);
        assert_eq!(normalized.bytes_churned, FixedPoint::ONE);
    }

    /// Non-overfull cost normalises to the expected ratios.
    #[test]
    fn test_clamp_and_normalize_below_limits_preserves_ratios() {
        use super::clamp_and_normalize;
        use midnight_base_crypto_v1::cost_model::{CostDuration, FixedPoint};

        let limits = SyntheticCost {
            read_time: CostDuration::from_picoseconds(1_000),
            compute_time: CostDuration::from_picoseconds(1_000),
            block_usage: 1_000,
            bytes_written: 1_000,
            bytes_churned: 1_000,
        };
        let cost = SyntheticCost {
            read_time: CostDuration::from_picoseconds(500),
            compute_time: CostDuration::from_picoseconds(500),
            block_usage: 500,
            bytes_written: 500,
            bytes_churned: 500,
        };

        let normalized = clamp_and_normalize(&cost, &limits, "test");
        let half = FixedPoint::from_u64_div(1, 2);
        assert_eq!(normalized.read_time, half);
        assert_eq!(normalized.compute_time, half);
        assert_eq!(normalized.block_usage, half);
        assert_eq!(normalized.bytes_written, half);
        assert_eq!(normalized.bytes_churned, half);
    }

    #[test]
    fn make_contract_event_attributes_dispatches_each_log_event_type() {
        let entry_point = EntryPointBuf(b"ep".to_vec());
        let dispatch = |t: LogEventType| {
            make_contract_event_attributes(
                &VersionedLogItem::<InMemoryDB> {
                    version: 1,
                    event_type: t,
                    data: StateValue::Null,
                },
                entry_point.clone(),
            )
        };

        assert!(matches!(
            dispatch(LogEventType::ShieldedSpend),
            LedgerEventAttributes::ContractShieldedSpend { .. }
        ));
        assert!(matches!(
            dispatch(LogEventType::ShieldedReceive),
            LedgerEventAttributes::ContractShieldedReceive { .. }
        ));
        assert!(matches!(
            dispatch(LogEventType::ShieldedMint),
            LedgerEventAttributes::ContractShieldedMint { .. }
        ));
        assert!(matches!(
            dispatch(LogEventType::ShieldedBurn),
            LedgerEventAttributes::ContractShieldedBurn { .. }
        ));
        assert!(matches!(
            dispatch(LogEventType::UnshieldedSpend),
            LedgerEventAttributes::ContractUnshieldedSpend { .. }
        ));
        assert!(matches!(
            dispatch(LogEventType::UnshieldedReceive),
            LedgerEventAttributes::ContractUnshieldedReceive { .. }
        ));
        assert!(matches!(
            dispatch(LogEventType::UnshieldedMint),
            LedgerEventAttributes::ContractUnshieldedMint { .. }
        ));
        assert!(matches!(
            dispatch(LogEventType::UnshieldedBurn),
            LedgerEventAttributes::ContractUnshieldedBurn { .. }
        ));
        assert!(matches!(
            dispatch(LogEventType::Paused),
            LedgerEventAttributes::ContractPaused { .. }
        ));
        assert!(matches!(
            dispatch(LogEventType::Unpaused),
            LedgerEventAttributes::ContractUnpaused { .. }
        ));
        assert!(matches!(
            dispatch(LogEventType::Misc),
            LedgerEventAttributes::ContractMisc { .. }
        ));
    }

    #[test]
    fn decodes_shielded_spend_nullifier() {
        let nullifier_bytes = vec![0xAA; 32];
        let item = VersionedLogItem {
            version: 1,
            event_type: LogEventType::ShieldedSpend,
            data: make_cell_data(nullifier_bytes.clone()),
        };
        let attrs = make_contract_event_attributes(&item, EntryPointBuf(b"spend".to_vec()));
        match attrs {
            LedgerEventAttributes::ContractShieldedSpend {
                version,
                entry_point,
                nullifier,
            } => {
                assert_eq!(version, 1);
                assert_eq!(&*entry_point, b"spend");
                assert_eq!(&*nullifier, nullifier_bytes.as_slice());
            }
            other => panic!("unexpected variant {other:?}"),
        }
    }

    #[test]
    fn decodes_shielded_receive_canonical_mip_0002_order() {
        // Canonical layout per merged MIP-0002 (main):
        // (commitment, ciphertext: Maybe<Bytes<512>>, contractAddress: Maybe<ContractAddress>).
        let mut bytes = Vec::with_capacity(578);
        bytes.extend_from_slice(&[0xAA; 32]); // commitment
        bytes.push(1); // ciphertext.is_some = true
        bytes.extend_from_slice(&[0xBB; 512]); // ciphertext value
        bytes.push(1); // contractAddress.is_some = true
        bytes.extend_from_slice(&[0xCC; 32]); // contractAddress value
        assert_eq!(bytes.len(), 578);
        let item = VersionedLogItem {
            version: 1,
            event_type: LogEventType::ShieldedReceive,
            data: make_cell_data(bytes),
        };
        let attrs = make_contract_event_attributes(&item, EntryPointBuf(b"receive".to_vec()));
        match attrs {
            LedgerEventAttributes::ContractShieldedReceive {
                commitment,
                ciphertext,
                receiving_contract_address,
                ..
            } => {
                assert_eq!(&*commitment, &[0xAA; 32]);
                let ct = ciphertext.expect("ciphertext should be Some");
                assert_eq!(ct.len(), 512);
                assert!(ct.iter().all(|&b| b == 0xBB));
                let rca = receiving_contract_address.expect("contractAddress should be Some");
                assert_eq!(&*rca, &[0xCC; 32]);
            }
            other => panic!("unexpected variant {other:?}"),
        }
    }

    #[test]
    fn decodes_shielded_receive_with_both_maybes_none() {
        // Spec-compliant emission with both Maybes None: only commitment +
        // two zero tag bytes on the wire. Trailing-zero stripping reduces
        // atom to 32 bytes (commitment alone). Decoder pads to 578 with
        // zeros; ciphertext and contractAddress tags both 0 → None.
        let bytes = vec![0xDD; 32]; // commitment only
        let item = VersionedLogItem {
            version: 1,
            event_type: LogEventType::ShieldedReceive,
            data: make_cell_data(bytes),
        };
        let attrs = make_contract_event_attributes(&item, EntryPointBuf(b"receive".to_vec()));
        match attrs {
            LedgerEventAttributes::ContractShieldedReceive {
                commitment,
                ciphertext,
                receiving_contract_address,
                ..
            } => {
                assert_eq!(&*commitment, &[0xDD; 32]);
                assert!(ciphertext.is_none());
                assert!(receiving_contract_address.is_none());
            }
            other => panic!("unexpected variant {other:?}"),
        }
    }

    #[test]
    fn decodes_shielded_mint_with_optional_amount_some() {
        let mut bytes = Vec::with_capacity(81);
        bytes.extend_from_slice(&[0xC1; 32]); // commitment
        bytes.extend_from_slice(&[0xD2; 32]); // domain_sep
        bytes.push(1); // amount.is_some = true
        bytes.extend_from_slice(&12345u128.to_le_bytes()); // amount value
        let item = VersionedLogItem {
            version: 1,
            event_type: LogEventType::ShieldedMint,
            data: make_cell_data(bytes.clone()),
        };
        let attrs = make_contract_event_attributes(&item, EntryPointBuf(b"mint".to_vec()));
        match attrs {
            LedgerEventAttributes::ContractShieldedMint {
                commitment,
                domain_sep,
                amount,
                ..
            } => {
                assert_eq!(&*commitment, &[0xC1; 32]);
                assert_eq!(&*domain_sep, &[0xD2; 32]);
                assert_eq!(amount.as_deref(), Some("12345"));
            }
            other => panic!("unexpected variant {other:?}"),
        }
    }

    #[test]
    fn decodes_shielded_burn_with_optional_amount_none() {
        let mut bytes = Vec::with_capacity(49);
        bytes.extend_from_slice(&[0xBB; 32]); // nullifier
        bytes.push(0); // amount.is_some = false
        bytes.extend_from_slice(&[0u8; 16]); // amount value (zeroed)
        let item = VersionedLogItem {
            version: 1,
            event_type: LogEventType::ShieldedBurn,
            data: make_cell_data(bytes),
        };
        let attrs = make_contract_event_attributes(&item, EntryPointBuf(b"burn".to_vec()));
        match attrs {
            LedgerEventAttributes::ContractShieldedBurn {
                nullifier, amount, ..
            } => {
                assert_eq!(&*nullifier, &[0xBB; 32]);
                assert_eq!(amount, None);
            }
            other => panic!("unexpected variant {other:?}"),
        }
    }

    #[test]
    fn decodes_unshielded_spend_user_sender() {
        // is_left = 1 → left variant = user; value from the left slot.
        let mut bytes = Vec::with_capacity(145);
        bytes.push(1); // is_left
        bytes.extend_from_slice(&[0xCC; 32]); // left slot (user key)
        bytes.extend_from_slice(&[0x00; 32]); // right slot (unused)
        bytes.extend_from_slice(&[0xDD; 32]); // domain_sep
        bytes.extend_from_slice(&[0xEE; 32]); // token_type
        bytes.extend_from_slice(&500u128.to_le_bytes()); // amount
        let item = VersionedLogItem {
            version: 1,
            event_type: LogEventType::UnshieldedSpend,
            data: make_cell_data(bytes),
        };
        let attrs = make_contract_event_attributes(&item, EntryPointBuf(b"u_spend".to_vec()));
        match attrs {
            LedgerEventAttributes::ContractUnshieldedSpend {
                sender,
                domain_sep,
                token_type,
                amount,
                ..
            } => {
                assert!(matches!(sender, AddressOrContract::User(_)));
                if let AddressOrContract::User(bytes) = sender {
                    assert_eq!(&*bytes, &[0xCC; 32]);
                }
                assert_eq!(&*domain_sep, &[0xDD; 32]);
                assert_eq!(&*token_type, &[0xEE; 32]);
                assert_eq!(amount, "500");
            }
            other => panic!("unexpected variant {other:?}"),
        }
    }

    #[test]
    fn decodes_unshielded_spend_contract_sender() {
        // is_left = 0 → right variant = contract; value from the right slot.
        let mut bytes = Vec::with_capacity(145);
        bytes.push(0); // is_left
        bytes.extend_from_slice(&[0x00; 32]); // left slot (unused)
        bytes.extend_from_slice(&[0xCC; 32]); // right slot (contract address)
        bytes.extend_from_slice(&[0xDD; 32]); // domain_sep
        bytes.extend_from_slice(&[0xEE; 32]); // token_type
        bytes.extend_from_slice(&500u128.to_le_bytes()); // amount
        let item = VersionedLogItem {
            version: 1,
            event_type: LogEventType::UnshieldedSpend,
            data: make_cell_data(bytes),
        };
        let attrs = make_contract_event_attributes(&item, EntryPointBuf(b"u_spend".to_vec()));
        match attrs {
            LedgerEventAttributes::ContractUnshieldedSpend {
                sender,
                domain_sep,
                token_type,
                amount,
                ..
            } => {
                assert!(matches!(sender, AddressOrContract::Contract(_)));
                if let AddressOrContract::Contract(bytes) = sender {
                    assert_eq!(&*bytes, &[0xCC; 32]);
                }
                assert_eq!(&*domain_sep, &[0xDD; 32]);
                assert_eq!(&*token_type, &[0xEE; 32]);
                assert_eq!(amount, "500");
            }
            other => panic!("unexpected variant {other:?}"),
        }
    }

    #[test]
    fn decodes_unshielded_receive_contract_recipient() {
        // 145-byte Receive (Either 65 + domain_sep 32 + token_type 32 + amount 16); is_left = 0.
        let mut bytes = Vec::with_capacity(145);
        bytes.push(0); // is_left
        bytes.extend_from_slice(&[0x00; 32]); // left slot (unused)
        bytes.extend_from_slice(&[0xCC; 32]); // right slot (contract address)
        bytes.extend_from_slice(&[0xDD; 32]); // domain_sep
        bytes.extend_from_slice(&[0xEE; 32]); // token_type
        bytes.extend_from_slice(&500u128.to_le_bytes()); // amount
        let item = VersionedLogItem {
            version: 1,
            event_type: LogEventType::UnshieldedReceive,
            data: make_cell_data(bytes),
        };
        let attrs = make_contract_event_attributes(&item, EntryPointBuf(b"u_recv".to_vec()));
        match attrs {
            LedgerEventAttributes::ContractUnshieldedReceive {
                recipient,
                domain_sep,
                token_type,
                amount,
                ..
            } => {
                assert!(matches!(recipient, AddressOrContract::Contract(_)));
                if let AddressOrContract::Contract(bytes) = recipient {
                    assert_eq!(&*bytes, &[0xCC; 32]);
                }
                assert_eq!(&*domain_sep, &[0xDD; 32]);
                assert_eq!(&*token_type, &[0xEE; 32]);
                assert_eq!(amount, "500");
            }
            other => panic!("unexpected variant {other:?}"),
        }
    }

    #[test]
    fn decodes_unshielded_burn() {
        // 113-byte Burn (Either 65 + token_type 32 + amount 16). #1279 rejected this as 81 bytes
        // and returned empty fields; it must now decode.
        let mut bytes = Vec::with_capacity(113);
        bytes.push(1); // is_left → user
        bytes.extend_from_slice(&[0xCC; 32]); // left slot (user key)
        bytes.extend_from_slice(&[0x00; 32]); // right slot (unused)
        bytes.extend_from_slice(&[0xEE; 32]); // token_type
        bytes.extend_from_slice(&500u128.to_le_bytes()); // amount
        let item = VersionedLogItem {
            version: 1,
            event_type: LogEventType::UnshieldedBurn,
            data: make_cell_data(bytes),
        };
        let attrs = make_contract_event_attributes(&item, EntryPointBuf(b"u_burn".to_vec()));
        match attrs {
            LedgerEventAttributes::ContractUnshieldedBurn {
                sender,
                token_type,
                amount,
                ..
            } => {
                assert!(matches!(sender, AddressOrContract::User(_)));
                if let AddressOrContract::User(bytes) = sender {
                    assert_eq!(&*bytes, &[0xCC; 32]);
                }
                assert_eq!(&*token_type, &[0xEE; 32]);
                assert_eq!(amount, "500");
            }
            other => panic!("unexpected variant {other:?}"),
        }
    }

    #[test]
    fn decodes_unshielded_mint_amount_le() {
        let mut bytes = Vec::with_capacity(80);
        bytes.extend_from_slice(&[0x11; 32]); // domain_sep
        bytes.extend_from_slice(&[0x22; 32]); // token_type
        bytes.extend_from_slice(&1_000_000u128.to_le_bytes());
        let item = VersionedLogItem {
            version: 1,
            event_type: LogEventType::UnshieldedMint,
            data: make_cell_data(bytes),
        };
        let attrs = make_contract_event_attributes(&item, EntryPointBuf(b"u_mint".to_vec()));
        match attrs {
            LedgerEventAttributes::ContractUnshieldedMint {
                domain_sep,
                token_type,
                amount,
                ..
            } => {
                assert_eq!(&*domain_sep, &[0x11; 32]);
                assert_eq!(&*token_type, &[0x22; 32]);
                assert_eq!(amount, "1000000");
            }
            other => panic!("unexpected variant {other:?}"),
        }
    }

    #[test]
    fn decodes_misc_name_and_payload() {
        let mut bytes = Vec::with_capacity(288);
        bytes.extend_from_slice(&[0x55; 32]); // name
        bytes.extend_from_slice(&[0x66; 256]); // payload
        let item = VersionedLogItem {
            version: 1,
            event_type: LogEventType::Misc,
            data: make_cell_data(bytes),
        };
        let attrs = make_contract_event_attributes(&item, EntryPointBuf(b"misc".to_vec()));
        match attrs {
            LedgerEventAttributes::ContractMisc { name, payload, .. } => {
                assert_eq!(&*name, &[0x55; 32]);
                assert_eq!(payload.len(), 256);
                assert!(payload.iter().all(|&b| b == 0x66));
            }
            other => panic!("unexpected variant {other:?}"),
        }
    }

    #[test]
    fn falls_back_to_empty_when_data_exceeds_max() {
        // Atom longer than the canonical max for the event type. Decoder
        // logs warning + falls back to empty payload fields.
        let bytes = vec![0xFF; 200]; // larger than the expected 145-byte spec size
        let item = VersionedLogItem {
            version: 1,
            event_type: LogEventType::UnshieldedSpend,
            data: make_cell_data(bytes),
        };
        let attrs = make_contract_event_attributes(&item, EntryPointBuf(b"u_spend".to_vec()));
        match attrs {
            LedgerEventAttributes::ContractUnshieldedSpend {
                sender,
                token_type,
                amount,
                ..
            } => {
                assert!(matches!(sender, AddressOrContract::User(b) if b.is_empty()));
                assert!(token_type.is_empty());
                assert_eq!(amount, "0");
            }
            other => panic!("unexpected variant {other:?}"),
        }
    }

    #[test]
    fn falls_back_to_empty_when_data_below_min() {
        // A payload shorter than the 129-byte min (= 145 - 16 amount strip) is rejected and falls
        // back to empty rather than misinterpreting shifted fields.
        let bytes = vec![0xAA; 81]; // below the 129-byte min
        let item = VersionedLogItem {
            version: 1,
            event_type: LogEventType::UnshieldedSpend,
            data: make_cell_data(bytes),
        };
        let attrs = make_contract_event_attributes(&item, EntryPointBuf(b"u_spend".to_vec()));
        match attrs {
            LedgerEventAttributes::ContractUnshieldedSpend {
                sender,
                token_type,
                amount,
                ..
            } => {
                assert!(matches!(sender, AddressOrContract::User(b) if b.is_empty()));
                assert!(token_type.is_empty());
                assert_eq!(amount, "0");
            }
            other => panic!("unexpected variant {other:?}"),
        }
    }

    #[test]
    fn decodes_unshielded_spend_amount_zero_fully_stripped() {
        // amount=0 strips the trailing u128 (16 zeros), leaving 129 bytes (Either 65 + domain_sep
        // 32 + token_type 32). The decoder pads back to 145 and decodes correctly.
        let mut bytes = Vec::with_capacity(129);
        bytes.push(1); // is_left → user
        bytes.extend_from_slice(&[0x11; 32]); // left slot (user key)
        bytes.extend_from_slice(&[0x00; 32]); // right slot (unused)
        bytes.extend_from_slice(&[0x22; 32]); // domain_sep
        bytes.extend_from_slice(&[0x33; 32]); // token_type
        // amount = 0, all 16 bytes stripped by ValueAtom
        assert_eq!(bytes.len(), 129);
        let item = VersionedLogItem {
            version: 1,
            event_type: LogEventType::UnshieldedSpend,
            data: make_cell_data(bytes),
        };
        let attrs = make_contract_event_attributes(&item, EntryPointBuf(b"u_spend".to_vec()));
        match attrs {
            LedgerEventAttributes::ContractUnshieldedSpend {
                sender,
                domain_sep,
                token_type,
                amount,
                ..
            } => {
                assert!(matches!(sender, AddressOrContract::User(_)));
                if let AddressOrContract::User(b) = sender {
                    assert_eq!(&*b, &[0x11; 32]);
                }
                assert_eq!(&*domain_sep, &[0x22; 32]);
                assert_eq!(&*token_type, &[0x33; 32]);
                assert_eq!(amount, "0");
            }
            other => panic!("unexpected variant {other:?}"),
        }
    }

    #[test]
    fn take_either_address_maps_is_left_tag() {
        // 65-byte Either: [is_left][left:32][right:32].
        // is_left = 1 → user (left slot); is_left = 0 → contract (right slot).
        let mut left = vec![1u8];
        left.extend_from_slice(&[0xAB; 32]); // left slot
        left.extend_from_slice(&[0xCD; 32]); // right slot
        match take_either_address(&left, 0) {
            Some(AddressOrContract::User(b)) => assert_eq!(&*b, &[0xAB; 32]),
            other => panic!("expected user, got {other:?}"),
        }

        let mut right = vec![0u8];
        right.extend_from_slice(&[0xAB; 32]); // left slot
        right.extend_from_slice(&[0xCD; 32]); // right slot
        match take_either_address(&right, 0) {
            Some(AddressOrContract::Contract(b)) => assert_eq!(&*b, &[0xCD; 32]),
            other => panic!("expected contract, got {other:?}"),
        }
    }

    #[test]
    fn falls_back_to_empty_when_data_is_not_cell() {
        let item = VersionedLogItem::<InMemoryDB> {
            version: 1,
            event_type: LogEventType::ShieldedSpend,
            data: StateValue::Null,
        };
        let attrs = make_contract_event_attributes(&item, EntryPointBuf(b"spend".to_vec()));
        match attrs {
            LedgerEventAttributes::ContractShieldedSpend { nullifier, .. } => {
                assert!(nullifier.is_empty());
            }
            other => panic!("unexpected variant {other:?}"),
        }
    }

    /// Build a `StateValue::Cell(AlignedValue)` carrying the given flat-byte
    /// payload, matching the wire shape produced by Compact's
    /// `serialize<T, n>` lowering of `emit(StructValue)`. The decoder only
    /// reads `aligned.value.0[0].0`, so the alignment field is left empty.
    fn make_cell_data(bytes: Vec<u8>) -> StateValue {
        let aligned = AlignedValue {
            value: Value(vec![ValueAtom(bytes)]),
            alignment: Alignment(vec![]),
        };
        StateValue::Cell(Sp::new(aligned))
    }
}
