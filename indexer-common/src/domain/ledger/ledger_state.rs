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
    ApplyRegularTransactionOutcome, ApplySystemTransactionOutcome, ByteArray, ByteVec, IntentHash,
    LedgerEvent, NetworkId, Nonce, PROTOCOL_VERSION_000_020_000, ProtocolVersion,
    SerializedContractAddress, SerializedLedgerParameters, SerializedLedgerState,
    SerializedTransaction, SerializedZswapState, SerializedZswapStateRoot, TokenType,
    TransactionResult, UnshieldedUtxo,
    dust::{self},
    ledger::{
        Error, IntentV7_0_0, SerializableV7_0_0Ext, TaggedSerializableV7_0_0Ext, TransactionV7_0_0,
    },
};
use fastrace::trace;
use itertools::Itertools;
use midnight_base_crypto_v7_0_0::{
    cost_model::{
        FixedPoint as FixedPointV7_0_0, NormalizedCost as NormalizedCostV7_0_0,
        SyntheticCost as SyntheticCostV7_0_0,
    },
    hash::{HashOutput as HashOutputV7_0_0, persistent_commit as persistent_commit_v7_0_0},
    time::Timestamp as TimestampV7_0_0,
};
use midnight_coin_structure_v7_0_0::{
    coin::{
        NIGHT as NIGHTV7_0_0, UnshieldedTokenType as UnshieldedTokenTypeV7_0_0,
        UserAddress as UserAddressV7_0_0,
    },
    contract::ContractAddress as ContractAddressV7_0_0,
};
use midnight_ledger_v7_0_0::{
    dust::{
        DustGenerationInfo as DustGenerationInfoV7_0_0, InitialNonce as InitialNonceV7_0_0,
        QualifiedDustOutput as QualifiedDustOutputV7_0_0,
    },
    events::{Event as EventV7_0_0, EventDetails as EventDetailsV7_0_0},
    semantics::{
        TransactionContext as TransactionContextV7_0_0,
        TransactionResult as TransactionResultV7_0_0,
    },
    structure::{
        LedgerParameters as LedgerParametersV7_0_0, LedgerState as LedgerStateV7_0_0,
        OutputInstructionUnshielded as OutputInstructionUnshieldedV7_0_0,
        SystemTransaction as SystemTransactionV7_0_0, Utxo as UtxoV7_0_0,
    },
    verify::WellFormedStrictness as WellFormedStrictnessV7_0_0,
};
use midnight_onchain_runtime_v7_0_0::context::BlockContext as BlockContextV7_0_0;
use midnight_serialize_v7_0_0::{
    Deserializable as DeserializableV7_0_0, tagged_deserialize as tagged_deserialize_v7_0_0,
};
use midnight_storage_v7_0_0::DefaultDB as DefaultDBV7_0_0;
use midnight_transient_crypto_v7_0_0::merkle_tree::{
    MerkleTreeCollapsedUpdate as MerkleTreeCollapsedUpdateV7_0_0,
    MerkleTreeDigest as MerkleTreeDigestV7_0_0, TreeInsertionPath as TreeInsertionPathV7_0_0,
};
use midnight_zswap_v7_0_0::ledger::State as ZswapStateV7_0_0;
use std::{collections::HashSet, ops::Deref, sync::LazyLock};

const OUTPUT_INDEX_ZERO: u32 = 0;

static STRICTNESS_V7_0_0: LazyLock<WellFormedStrictnessV7_0_0> = LazyLock::new(|| {
    let mut strictness = WellFormedStrictnessV7_0_0::default();
    strictness.enforce_balancing = false;
    strictness
});

/// Facade for `LedgerState` from `midnight_ledger` across supported (protocol) versions.
#[derive(Debug, Clone)]
pub enum LedgerState {
    V7_0_0 {
        ledger_state: LedgerStateV7_0_0<DefaultDBV7_0_0>,
        block_fullness: SyntheticCostV7_0_0,
    },
}

impl LedgerState {
    #[allow(missing_docs)]
    pub fn new(network_id: NetworkId) -> Self {
        Self::V7_0_0 {
            ledger_state: LedgerStateV7_0_0::new(network_id),
            block_fullness: Default::default(),
        }
    }

    /// Deserialize the given serialized ledger state using the given protocol version.
    #[trace(properties = { "protocol_version": "{protocol_version}" })]
    pub fn deserialize(
        ledger_state: impl AsRef<[u8]>,
        protocol_version: ProtocolVersion,
    ) -> Result<Self, Error> {
        if protocol_version.is_compatible(PROTOCOL_VERSION_000_020_000) {
            let ledger_state = tagged_deserialize_v7_0_0(&mut ledger_state.as_ref())
                .map_err(|error| Error::Deserialize("LedgerStateV7_0_0", error))?;
            Ok(Self::V7_0_0 {
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
            Self::V7_0_0 { ledger_state, .. } => ledger_state
                .tagged_serialize_v7_0_0()
                .map_err(|error| Error::Serialize("LedgerStateV7_0_0", error)),
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
    ) -> Result<ApplyRegularTransactionOutcome, Error> {
        match self {
            Self::V7_0_0 {
                ledger_state,
                block_fullness,
            } => {
                let transaction =
                    tagged_deserialize_v7_0_0::<TransactionV7_0_0>(&mut transaction.as_ref())
                        .map_err(|error| Error::Deserialize("LedgerTransactionV7_0_0", error))?;

                let cx = TransactionContextV7_0_0 {
                    ref_state: ledger_state.clone(),
                    block_context: BlockContextV7_0_0 {
                        tblock: timestamp_v7_0_0(block_timestamp),
                        tblock_err: 30,
                        parent_block_hash: HashOutputV7_0_0(block_parent_hash.0),
                    },
                    whitelist: None,
                };

                let cost = transaction
                    .cost(&ledger_state.parameters, true)
                    .map_err(|error| Error::TransactionCost(error.into()))?;
                let verified_ledger_transaction = transaction
                    .well_formed(&cx.ref_state, *STRICTNESS_V7_0_0, cx.block_context.tblock)
                    .map_err(|error| Error::MalformedTransaction(error.into()))?;
                let (ledger_state, transaction_result) =
                    ledger_state.apply(&verified_ledger_transaction, &cx);

                let (transaction_result, events, should_count_cost) = match transaction_result {
                    TransactionResultV7_0_0::Success(events) => {
                        (TransactionResult::Success, events, true)
                    }

                    TransactionResultV7_0_0::PartialSuccess(segments, events) => {
                        let segments = segments
                            .into_iter()
                            .map(|(id, result)| (id, result.is_ok()))
                            .collect::<Vec<_>>();
                        (TransactionResult::PartialSuccess(segments), events, true)
                    }

                    TransactionResultV7_0_0::Failure(_) => {
                        (TransactionResult::Failure, vec![], false)
                    }
                };

                // Only count cost for successful/partial transactions (match node behavior)
                let block_fullness = if should_count_cost {
                    *block_fullness + cost
                } else {
                    *block_fullness
                };

                let (created_unshielded_utxos, spent_unshielded_utxos) =
                    make_unshielded_utxos_for_regular_transaction_v7_0_0(
                        transaction,
                        &transaction_result,
                        &ledger_state,
                    );

                let ledger_events = make_ledger_events_v7_0_0(events)?;

                *self = Self::V7_0_0 {
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
            Self::V7_0_0 {
                ledger_state,
                block_fullness,
            } => {
                let transaction =
                    tagged_deserialize_v7_0_0::<SystemTransactionV7_0_0>(&mut transaction.as_ref())
                        .map_err(|error| Error::Deserialize("SystemTransactionV7_0_0", error))?;

                let cost = transaction.cost(&ledger_state.parameters);
                let (ledger_state, events) = ledger_state
                    .apply_system_tx(&transaction, timestamp_v7_0_0(block_timestamp))
                    .map_err(|error| Error::SystemTransaction(error.into()))?;
                let block_fullness = *block_fullness + cost;

                let created_unshielded_utxos =
                    make_unshielded_utxos_for_system_transaction_v7_0_0(transaction, &ledger_state);

                let ledger_events = make_ledger_events_v7_0_0(events)?;

                *self = Self::V7_0_0 {
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
            Self::V7_0_0 { ledger_state, .. } => ledger_state.zswap.first_free,
        }
    }

    /// Get the merkle tree root of the zswap state.
    pub fn zswap_merkle_tree_root(&self) -> ZswapStateRoot {
        match self {
            Self::V7_0_0 { ledger_state, .. } => {
                let root = ledger_state
                    .zswap
                    .coin_coms
                    .rehash()
                    .root()
                    .expect("zswap merkle tree root should exist");
                ZswapStateRoot::V7_0_0(root)
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
            Self::V7_0_0 { ledger_state, .. } => {
                let address = ContractAddressV7_0_0::deserialize(&mut address.as_ref(), 0)
                    .map_err(|error| Error::Deserialize("ContractAddressV7_0_0", error))?;

                let mut contract_zswap_state = ZswapStateV7_0_0::new();
                contract_zswap_state.coin_coms = ledger_state.zswap.filter(&[address]);

                contract_zswap_state
                    .tagged_serialize_v7_0_0()
                    .map_err(|error| Error::Serialize("ZswapStateV7_0_0", error))
            }
        }
    }

    /// Get the serialized merkle-tree collapsed update for the given indices.
    pub fn collapsed_update(&self, start_index: u64, end_index: u64) -> Result<ByteVec, Error> {
        match self {
            Self::V7_0_0 { ledger_state, .. } => MerkleTreeCollapsedUpdateV7_0_0::new(
                &ledger_state.zswap.coin_coms,
                start_index,
                end_index,
            )
            .map_err(|error| Error::InvalidUpdate(error.into()))?
            .tagged_serialize_v7_0_0()
            .map_err(|error| Error::Serialize("MerkleTreeCollapsedUpdateV7_0_0", error)),
        }
    }

    /// To be called after applying transactions.
    pub fn finalize_apply_transactions(
        &mut self,
        block_timestamp: u64,
    ) -> Result<LedgerParameters, Error> {
        match self {
            Self::V7_0_0 {
                ledger_state,
                block_fullness,
            } => {
                let timestamp = timestamp_v7_0_0(block_timestamp);
                let normalized_fullness = block_fullness
                    .normalize(ledger_state.parameters.limits.block_limits)
                    .unwrap_or(NormalizedCostV7_0_0::ZERO);
                let overall_fullness = FixedPointV7_0_0::max(
                    FixedPointV7_0_0::max(
                        FixedPointV7_0_0::max(
                            normalized_fullness.read_time,
                            normalized_fullness.compute_time,
                        ),
                        normalized_fullness.block_usage,
                    ),
                    FixedPointV7_0_0::max(
                        normalized_fullness.bytes_written,
                        normalized_fullness.bytes_churned,
                    ),
                );

                let ledger_state = ledger_state
                    .post_block_update(timestamp, normalized_fullness, overall_fullness)
                    .map_err(|error| Error::BlockLimitExceeded(error.into()))?;

                let ledger_parameters = ledger_state.parameters.deref().to_owned();

                *self = Self::V7_0_0 {
                    ledger_state,
                    block_fullness: Default::default(),
                };

                Ok(LedgerParameters::V7_0_0(ledger_parameters))
            }
        }
    }
}

/// Facade for ledger parameters across supported (protocol) versions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerParameters {
    V7_0_0(LedgerParametersV7_0_0),
}

impl LedgerParameters {
    /// Serialize these ledger parameters.
    #[trace]
    pub fn serialize(&self) -> Result<SerializedLedgerParameters, Error> {
        match self {
            Self::V7_0_0(parameters) => parameters
                .tagged_serialize_v7_0_0()
                .map_err(|error| Error::Serialize("SerializedLedgerParametersV7_0_0", error)),
        }
    }
}

/// Facade for zswap state root across supported (protocol) versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZswapStateRoot {
    V7_0_0(MerkleTreeDigestV7_0_0),
}

impl ZswapStateRoot {
    /// Untagged deserialize the given serialized zswap state root using the given protocol version.
    #[trace(properties = { "protocol_version": "{protocol_version}" })]
    pub fn deserialize(
        zswap_state_root: impl AsRef<[u8]>,
        protocol_version: ProtocolVersion,
    ) -> Result<Self, Error> {
        if protocol_version.is_compatible(PROTOCOL_VERSION_000_020_000) {
            let digest = MerkleTreeDigestV7_0_0::deserialize(&mut zswap_state_root.as_ref(), 0)
                .map_err(|error| Error::Deserialize("MerkleTreeDigestV7_0_0", error))?;
            Ok(Self::V7_0_0(digest))
        } else {
            Err(Error::InvalidProtocolVersion(protocol_version))
        }
    }

    /// Serialize this zswap state root.
    #[trace]
    pub fn serialize(&self) -> Result<SerializedZswapStateRoot, Error> {
        match self {
            Self::V7_0_0(digest) => digest
                .serialize_v7_0_0()
                .map_err(|error| Error::Serialize("MerkleTreeDigestV7_0_0", error)),
        }
    }
}

fn timestamp_v7_0_0(block_timestamp: u64) -> TimestampV7_0_0 {
    TimestampV7_0_0::from_secs(block_timestamp / 1000)
}

fn make_ledger_events_v7_0_0(
    events: Vec<EventV7_0_0<DefaultDBV7_0_0>>,
) -> Result<Vec<LedgerEvent>, Error> {
    events
        .into_iter()
        .map(|event| {
            let raw = event
                .tagged_serialize_v7_0_0()
                .map_err(|error| Error::Serialize("EventV7_0_0", error))?;
            Ok::<_, Error>((event, raw))
        })
        .filter_map_ok(|(event, raw)| match event.content {
            EventDetailsV7_0_0::ZswapInput { .. } => Some(Ok(LedgerEvent::zswap_input(raw))),

            EventDetailsV7_0_0::ZswapOutput { .. } => Some(Ok(LedgerEvent::zswap_output(raw))),

            EventDetailsV7_0_0::ContractDeploy { .. } => None,

            EventDetailsV7_0_0::ContractLog { .. } => None,

            EventDetailsV7_0_0::ParamChange(..) => Some(Ok(LedgerEvent::param_change(raw))),

            EventDetailsV7_0_0::DustInitialUtxo {
                output,
                generation,
                generation_index,
                ..
            } => Some(make_dust_initial_utxo_v7_0_0(
                output,
                generation,
                generation_index,
                raw,
            )),

            EventDetailsV7_0_0::DustGenerationDtimeUpdate { update, .. } => {
                Some(make_dust_generation_dtime_update_v7_0_0(update, raw))
            }

            EventDetailsV7_0_0::DustSpendProcessed { .. } => {
                Some(Ok(LedgerEvent::dust_spend_processed(raw)))
            }

            other => panic!("unexpected EventDetailsV7_0_0 variant {other:?}"),
        })
        .flatten()
        .collect::<Result<_, _>>()
}

fn make_dust_initial_utxo_v7_0_0(
    output: QualifiedDustOutputV7_0_0,
    generation: DustGenerationInfoV7_0_0,
    generation_index: u64,
    raw: ByteVec,
) -> Result<LedgerEvent, Error> {
    let owner = output
        .owner
        .serialize_v7_0_0()
        .map_err(|error| Error::Serialize("DustPublicKeyV7_0_0", error))?;

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
        .serialize_v7_0_0()
        .map_err(|error| Error::Serialize("DustPublicKeyV7_0_0", error))?;

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

fn make_dust_generation_dtime_update_v7_0_0(
    update: TreeInsertionPathV7_0_0<DustGenerationInfoV7_0_0>,
    raw: ByteVec,
) -> Result<LedgerEvent, Error> {
    let generation = &update.leaf.1;

    let owner = generation
        .owner
        .serialize_v7_0_0()
        .map_err(|error| Error::Serialize("DustPublicKeyV7_0_0", error))?;

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

fn make_unshielded_utxos_for_regular_transaction_v7_0_0(
    transaction: TransactionV7_0_0,
    transaction_result: &TransactionResult,
    ledger_state: &LedgerStateV7_0_0<DefaultDBV7_0_0>,
) -> (Vec<UnshieldedUtxo>, Vec<UnshieldedUtxo>) {
    // Skip UTXO creation entirely for failed transactions, because no state changes occurred on the
    // ledger.
    if matches!(transaction_result, TransactionResult::Failure) {
        return (vec![], vec![]);
    }

    match transaction {
        TransactionV7_0_0::Standard(transaction) => {
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
                        extend_unshielded_utxos_v7_0_0(
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
                    extend_unshielded_utxos_v7_0_0(
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
        TransactionV7_0_0::ClaimRewards(claim) => {
            let owner = UserAddressV7_0_0::from(claim.owner);
            let ledger_intent_hash = {
                // ClaimRewards don't have intents, but UTXOs need an intent hash. We compute this
                // hash the same way that the ledger does internally.
                let output = OutputInstructionUnshieldedV7_0_0 {
                    amount: claim.value,
                    target_address: owner,
                    nonce: claim.nonce,
                };
                output.mk_intent_hash(NIGHTV7_0_0)
            };
            let intent_hash = ledger_intent_hash.0.0.into();
            let initial_nonce = make_initial_nonce_v7_0_0(OUTPUT_INDEX_ZERO, intent_hash);
            let registered_for_dust_generation =
                registered_for_dust_generation_v7_0_0(OUTPUT_INDEX_ZERO, intent_hash, ledger_state);
            let utxo = UtxoV7_0_0 {
                value: claim.value,
                owner,
                type_: UnshieldedTokenTypeV7_0_0::default(),
                intent_hash: ledger_intent_hash,
                output_no: OUTPUT_INDEX_ZERO,
            };

            let utxo = UnshieldedUtxo {
                owner: owner.0.0.into(),
                token_type: TokenType::default(), // Native token (all zeros).
                value: claim.value,
                intent_hash,
                output_index: OUTPUT_INDEX_ZERO,
                ctime: ctime_v7_0_0(&utxo, ledger_state),
                initial_nonce,
                registered_for_dust_generation,
            };

            (vec![utxo], vec![]) // Creates one UTXO, spends none.
        }
    }
}

fn make_unshielded_utxos_for_system_transaction_v7_0_0(
    transaction: SystemTransactionV7_0_0,
    ledger_state: &LedgerStateV7_0_0<DefaultDBV7_0_0>,
) -> Vec<UnshieldedUtxo> {
    match transaction {
        SystemTransactionV7_0_0::PayFromTreasuryUnshielded {
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
                    let initial_nonce = make_initial_nonce_v7_0_0(index as u32, intent_hash);
                    let registered_for_dust_generation = registered_for_dust_generation_v7_0_0(
                        index as u32,
                        intent_hash,
                        ledger_state,
                    );
                    let utxo = UtxoV7_0_0 {
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
                        ctime: ctime_v7_0_0(&utxo, ledger_state),
                        initial_nonce,
                        registered_for_dust_generation,
                    }
                })
                .collect()
        }

        _ => vec![], // Other system transaction types don't create unshielded UTXOs.
    }
}

fn extend_unshielded_utxos_v7_0_0(
    outputs: &mut Vec<UnshieldedUtxo>,
    inputs: &mut Vec<UnshieldedUtxo>,
    segment_id: u16,
    intent: &IntentV7_0_0,
    guaranteed: bool,
    ledger_state: &LedgerStateV7_0_0<DefaultDBV7_0_0>,
) {
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
            let initial_nonce = make_initial_nonce_v7_0_0(output_index, intent_hash);
            let registered_for_dust_generation =
                registered_for_dust_generation_v7_0_0(output_index, intent_hash, ledger_state);
            let utxo = UtxoV7_0_0 {
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
                ctime: ctime_v7_0_0(&utxo, ledger_state),
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
        let initial_nonce = make_initial_nonce_v7_0_0(spend.output_no, intent_hash);
        let registered_for_dust_generation =
            registered_for_dust_generation_v7_0_0(spend.output_no, intent_hash, ledger_state);
        let utxo = UtxoV7_0_0 {
            value: spend.value,
            owner: UserAddressV7_0_0::from(spend.owner.clone()),
            type_: spend.type_,
            intent_hash: spend.intent_hash,
            output_no: spend.output_no,
        };

        UnshieldedUtxo {
            owner: UserAddressV7_0_0::from(spend.owner).0.0.into(),
            token_type: spend.type_.0.0.into(),
            value: spend.value,
            intent_hash,
            output_index: spend.output_no,
            ctime: ctime_v7_0_0(&utxo, ledger_state),
            initial_nonce,
            registered_for_dust_generation,
        }
    });
    inputs.extend(intent_inputs);
}

fn make_initial_nonce_v7_0_0(output_index: u32, intent_hash: IntentHash) -> Nonce {
    let intent_hash_v7_0_0 = HashOutputV7_0_0(intent_hash.0);
    let initial_nonce =
        InitialNonceV7_0_0(persistent_commit_v7_0_0(&output_index, intent_hash_v7_0_0));
    ByteArray(initial_nonce.0.0)
}

fn registered_for_dust_generation_v7_0_0(
    output_index: u32,
    intent_hash: IntentHash,
    ledger_state: &LedgerStateV7_0_0<DefaultDBV7_0_0>,
) -> bool {
    let intent_hash_v7_0_0 = HashOutputV7_0_0(intent_hash.0);
    let initial_nonce =
        InitialNonceV7_0_0(persistent_commit_v7_0_0(&output_index, intent_hash_v7_0_0));
    ledger_state
        .dust
        .generation
        .night_indices
        .contains_key(&initial_nonce)
}

fn ctime_v7_0_0(
    utxo: &UtxoV7_0_0,
    ledger_state: &LedgerStateV7_0_0<DefaultDBV7_0_0>,
) -> Option<u64> {
    ledger_state
        .utxo
        .utxos
        .get(utxo)
        .map(|meta| meta.ctime.to_secs())
}

#[cfg(test)]
mod tests {
    use crate::domain::{
        NetworkId, TransactionResult,
        ledger::{
            TransactionV7_0_0, ledger_state::make_unshielded_utxos_for_regular_transaction_v7_0_0,
        },
    };
    use midnight_ledger_v7_0_0::structure::{
        LedgerState as LedgerStateV7_0_0, StandardTransaction as StandardTransactionV7_0_0,
    };
    use midnight_transient_crypto_v7_0_0::curve::EmbeddedFr;

    #[test]
    fn test_make_unshielded_utxos_v7_0_0() {
        let network_id = NetworkId::try_from("undeployed").unwrap();

        let transaction = StandardTransactionV7_0_0 {
            network_id: network_id.to_string(),
            intents: Default::default(),
            guaranteed_coins: Default::default(),
            fallible_coins: Default::default(),
            binding_randomness: EmbeddedFr::from_le_bytes(&[0u8; 32]).unwrap(),
        };
        let ledger_transaction = TransactionV7_0_0::Standard(transaction);

        let ledger_state = LedgerStateV7_0_0::new(network_id);

        let (created, spent) = make_unshielded_utxos_for_regular_transaction_v7_0_0(
            ledger_transaction.clone(),
            &TransactionResult::Failure,
            &ledger_state,
        );
        assert!(created.is_empty());
        assert!(spent.is_empty());

        let (created, spent) = make_unshielded_utxos_for_regular_transaction_v7_0_0(
            ledger_transaction,
            &TransactionResult::Success,
            &ledger_state,
        );
        assert!(created.is_empty());
        assert!(spent.is_empty());
    }
}
