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

use crate::domain::{
    ApplyRegularTransactionResult, ByteArray, ByteVec, DustOutput, IntentHash, LedgerEvent,
    NetworkId, Nonce, PROTOCOL_VERSION_000_016_000, ProtocolVersion, RawTokenType,
    SerializedContractAddress, SerializedLedgerParameters, SerializedLedgerState,
    SerializedTransaction, SerializedZswapState, SerializedZswapStateRoot, TransactionResult,
    UnshieldedUtxo,
    ledger::{Error, IntentV6, SerializableV6Ext, TaggedSerializableV6Ext, TransactionV6},
};
use fastrace::trace;
use itertools::Itertools;
use midnight_base_crypto_v6::{
    cost_model::SyntheticCost as SyntheticCostV6,
    hash::{HashOutput as HashOutputV6, persistent_commit as persistent_commit_v6},
    time::Timestamp as TimestampV6,
};
use midnight_coin_structure_v6::{
    coin::{NIGHT as NIGHTV6, Nonce as NonceV6, UserAddress as UserAddressV6},
    contract::ContractAddress as ContractAddressV6,
};
use midnight_ledger_v6::{
    dust::InitialNonce as InitialNonceV6,
    events::{Event as EventV6, EventDetails as EventDetailsV6},
    semantics::{
        TransactionContext as TransactionContextV6, TransactionResult as TransactionResultV6,
    },
    structure::{
        LedgerParameters as LedgerParametersV6, LedgerState as LedgerStateV6,
        OutputInstructionUnshielded as OutputInstructionUnshieldedV6,
        SystemTransaction as LedgerSystemTransactionV6,
    },
    verify::WellFormedStrictness as WellFormedStrictnessV6,
};
use midnight_onchain_runtime_v6::context::BlockContext as BlockContextV6;
use midnight_serialize_v6::{
    Deserializable as DeserializableV6, tagged_deserialize as tagged_deserialize_v6,
};
use midnight_storage_v6::DefaultDB as DefaultDBV6;
use midnight_transient_crypto_v6::merkle_tree::{
    MerkleTreeCollapsedUpdate as MerkleTreeCollapsedUpdateV6,
    MerkleTreeDigest as MerkleTreeDigestV6,
};
use midnight_zswap_v6::ledger::State as ZswapStateV6;
use std::{collections::HashSet, ops::Deref, sync::LazyLock};

static STRICTNESS_V6: LazyLock<WellFormedStrictnessV6> = LazyLock::new(|| {
    let mut strictness = WellFormedStrictnessV6::default();
    strictness.enforce_balancing = false;
    strictness
});

/// Facade for `LedgerState` from `midnight_ledger` across supported (protocol) versions.
#[derive(Debug, Clone)]
pub enum LedgerState {
    V6 {
        ledger_state: LedgerStateV6<DefaultDBV6>,
        block_fullness: SyntheticCostV6,
    },
}

impl LedgerState {
    #[allow(missing_docs)]
    pub fn new(network_id: NetworkId) -> Self {
        Self::V6 {
            ledger_state: LedgerStateV6::new(network_id),
            block_fullness: Default::default(),
        }
    }

    /// Deserialize the given serialized ledger state using the given protocol version.
    #[trace(properties = { "protocol_version": "{protocol_version}" })]
    pub fn deserialize(
        ledger_state: impl AsRef<[u8]>,
        protocol_version: ProtocolVersion,
    ) -> Result<Self, Error> {
        if protocol_version.is_compatible(PROTOCOL_VERSION_000_016_000) {
            let ledger_state = tagged_deserialize_v6(&mut ledger_state.as_ref())
                .map_err(|error| Error::Io("cannot deserialize LedgerStateV6", error))?;
            Ok(Self::V6 {
                ledger_state,
                block_fullness: Default::default(),
            })
        } else {
            Err(Error::InvalidProtocolVersion(protocol_version))
        }
    }

    /// Serialize this ledger state.
    #[trace]
    pub fn serialize(&self) -> Result<SerializedLedgerState, Error> {
        match self {
            Self::V6 { ledger_state, .. } => ledger_state
                .tagged_serialize_v6()
                .map_err(|error| Error::Io("cannot serialize LedgerStateV6", error)),
        }
    }

    /// Apply the given serialized regular transaction to this ledger state and return the
    /// transaction result as well as the created and spent unshielded UTXOs.
    #[trace]
    pub fn apply_regular_transaction(
        &mut self,
        transaction: &SerializedTransaction,
        block_parent_hash: ByteArray<32>,
        block_timestamp: u64,
    ) -> Result<ApplyRegularTransactionResult, Error> {
        match self {
            Self::V6 {
                ledger_state,
                block_fullness,
            } => {
                let ledger_transaction = tagged_deserialize_v6::<TransactionV6>(
                    &mut transaction.as_ref(),
                )
                .map_err(|error| Error::Io("cannot deserialize LedgerTransactionV6", error))?;

                let cx = TransactionContextV6 {
                    ref_state: ledger_state.clone(),
                    block_context: BlockContextV6 {
                        tblock: timestamp_v6(block_timestamp),
                        tblock_err: 30,
                        parent_block_hash: HashOutputV6(block_parent_hash.0),
                    },
                    whitelist: None,
                };

                let cost = ledger_transaction
                    .cost(&ledger_state.parameters, true)
                    .map_err(|error| Error::TransactionCost(error.into()))?;
                let verified_ledger_transaction = ledger_transaction
                    .well_formed(&cx.ref_state, *STRICTNESS_V6, cx.block_context.tblock)
                    .map_err(|error| Error::MalformedTransaction(error.into()))?;
                let (ledger_state, transaction_result) =
                    ledger_state.apply(&verified_ledger_transaction, &cx);
                let block_fullness = *block_fullness + cost;

                let (transaction_result, events) = match transaction_result {
                    TransactionResultV6::Success(events) => (TransactionResult::Success, events),

                    TransactionResultV6::PartialSuccess(segments, events) => {
                        let segments = segments
                            .into_iter()
                            .map(|(id, result)| (id, result.is_ok()))
                            .collect::<Vec<_>>();
                        (TransactionResult::PartialSuccess(segments), events)
                    }

                    TransactionResultV6::Failure(_) => (TransactionResult::Failure, vec![]),
                };

                let ledger_events = make_ledger_events_v6(events)?;

                let (created_unshielded_utxos, spent_unshielded_utxos) = make_unshielded_utxos_v6(
                    ledger_transaction,
                    &transaction_result,
                    &ledger_state,
                );

                *self = Self::V6 {
                    ledger_state,
                    block_fullness,
                };

                Ok(ApplyRegularTransactionResult {
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
    ) -> Result<Vec<LedgerEvent>, Error> {
        match self {
            Self::V6 {
                ledger_state,
                block_fullness,
            } => {
                let ledger_transaction =
                    tagged_deserialize_v6::<LedgerSystemTransactionV6>(&mut transaction.as_ref())
                        .map_err(|error| {
                        Error::Io("cannot deserialize LedgerSystemTransactionV6", error)
                    })?;

                let cost = ledger_transaction.cost(&ledger_state.parameters);
                let (ledger_state, events) = ledger_state
                    .apply_system_tx(&ledger_transaction, timestamp_v6(block_timestamp))
                    .map_err(|error| Error::SystemTransaction(error.into()))?;
                let block_fullness = *block_fullness + cost;

                let ledger_events = make_ledger_events_v6(events)?;

                *self = Self::V6 {
                    ledger_state,
                    block_fullness,
                };

                Ok(ledger_events)
            }
        }
    }

    /// Get the first free index of the zswap state.
    pub fn zswap_first_free(&self) -> u64 {
        match self {
            Self::V6 { ledger_state, .. } => ledger_state.zswap.first_free,
        }
    }

    /// Get the merkle tree root of the zswap state.
    pub fn zswap_merkle_tree_root(&self) -> ZswapStateRoot {
        match self {
            Self::V6 { ledger_state, .. } => {
                let root = ledger_state
                    .zswap
                    .coin_coms
                    .rehash()
                    .root()
                    .expect("zswap merkle tree root should exist");
                ZswapStateRoot::V6(root)
            }
        }
    }

    /// Extract the zswap state for the given contract address.
    pub fn extract_contract_zswap_state(
        &self,
        address: &SerializedContractAddress,
    ) -> Result<SerializedZswapState, Error> {
        match self {
            Self::V6 { ledger_state, .. } => {
                let address = tagged_deserialize_v6::<ContractAddressV6>(&mut address.as_ref())
                    .map_err(|error| Error::Io("cannot deserialize ContractAddressV6", error))?;

                let mut contract_zswap_state = ZswapStateV6::new();
                contract_zswap_state.coin_coms = ledger_state.zswap.filter(&[address]);

                contract_zswap_state
                    .tagged_serialize_v6()
                    .map_err(|error| Error::Io("cannot serialize ZswapStateV6", error))
            }
        }
    }

    /// Get the serialized merkle-tree collapsed update for the given indices.
    pub fn collapsed_update(&self, start_index: u64, end_index: u64) -> Result<ByteVec, Error> {
        match self {
            Self::V6 { ledger_state, .. } => MerkleTreeCollapsedUpdateV6::new(
                &ledger_state.zswap.coin_coms,
                start_index,
                end_index,
            )
            .map_err(|error| Error::InvalidUpdate(error.into()))?
            .tagged_serialize_v6()
            .map_err(|error| Error::Io("cannot serialize MerkleTreeCollapsedUpdateV6", error)),
        }
    }

    /// To be called after applying transactions.
    pub fn post_apply_transactions(
        &mut self,
        block_timestamp: u64,
    ) -> Result<LedgerParameters, Error> {
        match self {
            Self::V6 {
                ledger_state,
                block_fullness,
            } => {
                let timestamp = timestamp_v6(block_timestamp);
                let ledger_state = ledger_state
                    .post_block_update(timestamp, *block_fullness)
                    .map_err(|error| Error::BlockLimitExceeded(error.into()))?;

                let ledger_parameters = ledger_state.parameters.deref().to_owned();

                *self = Self::V6 {
                    ledger_state,
                    block_fullness: Default::default(),
                };

                Ok(LedgerParameters::V6(ledger_parameters))
            }
        }
    }
}

/// Facade for ledger parameters across supported (protocol) versions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerParameters {
    V6(LedgerParametersV6),
}

impl LedgerParameters {
    /// Serialize these ledger parameters.
    #[trace]
    pub fn serialize(&self) -> Result<SerializedLedgerParameters, Error> {
        match self {
            Self::V6(parameters) => parameters
                .serialize_v6()
                .map_err(|error| Error::Io("cannot serialize SerializedLedgerParametersV6", error)),
        }
    }
}

/// Facade for zswap state root across supported (protocol) versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZswapStateRoot {
    V6(MerkleTreeDigestV6),
}

impl ZswapStateRoot {
    /// Deserialize the given serialized zswap state root using the given protocol version.
    #[trace(properties = { "protocol_version": "{protocol_version}" })]
    pub fn deserialize(
        zswap_state_root: impl AsRef<[u8]>,
        protocol_version: ProtocolVersion,
    ) -> Result<Self, Error> {
        if protocol_version.is_compatible(PROTOCOL_VERSION_000_016_000) {
            let digest = MerkleTreeDigestV6::deserialize(&mut zswap_state_root.as_ref(), 0)
                .map_err(|error| Error::Io("cannot deserialize MerkleTreeDigestV6", error))?;
            Ok(Self::V6(digest))
        } else {
            Err(Error::InvalidProtocolVersion(protocol_version))
        }
    }

    /// Serialize this zswap state root.
    #[trace]
    pub fn serialize(&self) -> Result<SerializedZswapStateRoot, Error> {
        match self {
            Self::V6(digest) => digest
                .serialize_v6()
                .map_err(|error| Error::Io("cannot serialize zswap merkle tree root", error)),
        }
    }
}

fn timestamp_v6(block_timestamp: u64) -> TimestampV6 {
    TimestampV6::from_secs(block_timestamp / 1000)
}

fn make_ledger_events_v6(events: Vec<EventV6<DefaultDBV6>>) -> Result<Vec<LedgerEvent>, Error> {
    events
        .into_iter()
        .map(|event| {
            let raw = event
                .tagged_serialize_v6()
                .map_err(|error| Error::Io("cannot serialize EventV6", error))?;
            Ok((event, raw))
        })
        .filter_map_ok(|(event, raw)| match event.content {
            EventDetailsV6::ZswapInput { .. } => Some(LedgerEvent::zswap_input(raw)),

            EventDetailsV6::ZswapOutput { .. } => Some(LedgerEvent::zswap_output(raw)),

            EventDetailsV6::ContractDeploy { .. } => None,

            EventDetailsV6::ContractLog { .. } => None,

            EventDetailsV6::ParamChange(..) => Some(LedgerEvent::param_change(raw)),

            EventDetailsV6::DustInitialUtxo { output, .. } => {
                let output = DustOutput {
                    nonce: output.nonce.0.to_bytes_le().into(),
                };
                Some(LedgerEvent::dust_initial_utxo(raw, output))
            }

            EventDetailsV6::DustGenerationDtimeUpdate { .. } => {
                Some(LedgerEvent::dust_generation_dtime_update(raw))
            }

            EventDetailsV6::DustSpendProcessed { .. } => {
                Some(LedgerEvent::dust_spend_processed(raw))
            }

            other => panic!("unexpected EventDetailsV6 variant {other:?}"),
        })
        .collect::<Result<_, _>>()
}

fn make_unshielded_utxos_v6(
    ledger_transaction: TransactionV6,
    transaction_result: &TransactionResult,
    ledger_state: &LedgerStateV6<DefaultDBV6>,
) -> (Vec<UnshieldedUtxo>, Vec<UnshieldedUtxo>) {
    match ledger_transaction {
        TransactionV6::Standard(transaction) => {
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
                        extend_unshielded_utxos_v6(
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
                    extend_unshielded_utxos_v6(
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

        TransactionV6::ClaimRewards(claim) => {
            // ClaimRewards creates a single unshielded UTXO for the claimed amount.
            let owner = UserAddressV6::from(claim.owner.clone());
            let intent_hash =
                compute_claim_rewards_intent_hash_v6(&owner, claim.value, &claim.nonce);
            let output_index = 0;

            let initial_nonce = make_initial_nonce_v6(output_index, intent_hash);
            let registered_for_dust_generation =
                registered_for_dust_generation_v6(output_index, intent_hash, ledger_state);

            let utxo = UnshieldedUtxo {
                owner: owner.0.0.into(),
                token_type: RawTokenType::default(), // Native token (all zeros).
                value: claim.value,
                intent_hash,
                output_index,
                initial_nonce,
                registered_for_dust_generation,
            };

            (vec![utxo], vec![]) // Creates one UTXO, spends none.
        }
    }
}

fn extend_unshielded_utxos_v6(
    outputs: &mut Vec<UnshieldedUtxo>,
    inputs: &mut Vec<UnshieldedUtxo>,
    segment_id: u16,
    intent: &IntentV6,
    guaranteed: bool,
    ledger_state: &LedgerStateV6<DefaultDBV6>,
) {
    let intent_hash = intent
        .erase_proofs()
        .erase_signatures()
        .intent_hash(segment_id);
    let intent_hash = intent_hash.0.0.into();

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
            let initial_nonce = make_initial_nonce_v6(output_index, intent_hash);
            let registered_for_dust_generation =
                registered_for_dust_generation_v6(output_index, intent_hash, ledger_state);

            UnshieldedUtxo {
                owner: output.owner.0.0.into(),
                token_type: output.type_.0.0.into(),
                value: output.value,
                intent_hash,
                output_index,
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
        let initial_nonce = make_initial_nonce_v6(spend.output_no, intent_hash);
        let registered_for_dust_generation =
            registered_for_dust_generation_v6(spend.output_no, intent_hash, ledger_state);

        UnshieldedUtxo {
            owner: UserAddressV6::from(spend.owner).0.0.into(),
            token_type: spend.type_.0.0.into(),
            value: spend.value,
            intent_hash,
            output_index: spend.output_no,
            initial_nonce,
            registered_for_dust_generation,
        }
    });
    inputs.extend(intent_inputs);
}

fn make_initial_nonce_v6(output_index: u32, intent_hash: IntentHash) -> Nonce {
    let intent_hash_v6 = HashOutputV6(intent_hash.0);
    let initial_nonce = InitialNonceV6(persistent_commit_v6(&output_index, intent_hash_v6));
    ByteArray(initial_nonce.0.0)
}

fn registered_for_dust_generation_v6(
    output_index: u32,
    intent_hash: IntentHash,
    ledger_state: &LedgerStateV6<DefaultDBV6>,
) -> bool {
    let intent_hash_v6 = HashOutputV6(intent_hash.0);
    let initial_nonce = InitialNonceV6(persistent_commit_v6(&output_index, intent_hash_v6));
    ledger_state
        .dust
        .generation
        .night_indices
        .contains_key(&initial_nonce)
}

/// Compute a pseudo-intent-hash for ClaimRewards transactions.
/// ClaimRewards don't have intents, but we need a unique hash for database constraints.
/// We use a hash of (owner || value || nonce) to ensure uniqueness.
fn compute_claim_rewards_intent_hash_v6(
    owner: &UserAddressV6,
    value: u128,
    nonce: &NonceV6,
) -> IntentHash {
    let output = OutputInstructionUnshieldedV6 {
        amount: value,
        target_address: *owner,
        nonce: *nonce,
    };

    ByteArray(output.mk_intent_hash(NIGHTV6).0.0)
}
