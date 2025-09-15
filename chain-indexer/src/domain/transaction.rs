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

use crate::domain::{ContractAction, dust::DustEvent, dust_processing::ProcessedDustEvents, node};
use indexer_common::domain::{
    ByteArray, ByteVec, DustCommitment, DustNonce, DustNullifier, DustOwner, NightUtxoNonce,
    ProtocolVersion,
    dust::{DustEventDetails, DustGenerationInfo, DustParameters, QualifiedDustOutput},
    ledger::{
        SerializedTransaction, SerializedTransactionIdentifier, SerializedZswapStateRoot,
        SystemTransaction as DomainSystemTransaction, TransactionHash, TransactionResult,
        UnshieldedUtxo,
    },
};
use serde::Serialize;
use sqlx::{FromRow, Type};
use std::fmt::Debug;
use thiserror::Error;

/// Serialized parameter update data.
pub type SerializedParameterUpdate = ByteVec;

/// Serialized treasury payment outputs.
pub type SerializedTreasuryOutputs = ByteVec;

/// Serialized NIGHT distribution outputs.
pub type SerializedNightOutputs = ByteVec;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transaction {
    Regular(Box<RegularTransaction>),
    System(Box<SystemTransaction>),
}

impl Transaction {
    pub fn variant(&self) -> TransactionVariant {
        match self {
            Transaction::Regular(_) => TransactionVariant::Regular,
            Transaction::System(_) => TransactionVariant::System,
        }
    }

    pub fn hash(&self) -> TransactionHash {
        match self {
            Transaction::Regular(transaction) => transaction.hash,
            Transaction::System(transaction) => transaction.hash,
        }
    }

    pub fn protocol_version(&self) -> ProtocolVersion {
        match self {
            Transaction::Regular(transaction) => transaction.protocol_version,
            Transaction::System(transaction) => transaction.protocol_version,
        }
    }

    pub fn raw(&self) -> &[u8] {
        match self {
            Transaction::Regular(transaction) => &transaction.raw,
            Transaction::System(transaction) => &transaction.raw,
        }
    }
}

impl TryFrom<node::Transaction> for Transaction {
    type Error = SystemTransactionError;

    fn try_from(transaction: node::Transaction) -> Result<Self, Self::Error> {
        match transaction {
            node::Transaction::Regular(regular_transaction) => {
                Ok(Transaction::Regular(Box::new(regular_transaction.into())))
            }

            node::Transaction::System(system_transaction) => Ok(Transaction::System(Box::new(
                system_transaction.try_into()?,
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegularTransaction {
    // These fields come from node::Transaction.
    pub hash: TransactionHash,
    pub protocol_version: ProtocolVersion,
    pub raw: SerializedTransaction,
    pub identifiers: Vec<SerializedTransactionIdentifier>,
    pub contract_actions: Vec<ContractAction>,
    pub paid_fees: u128,
    pub estimated_fees: u128,

    // These fields come from applying the node transactions to the ledger state.
    pub transaction_result: TransactionResult,
    pub merkle_tree_root: SerializedZswapStateRoot,
    pub start_index: u64,
    pub end_index: u64,
    pub created_unshielded_utxos: Vec<UnshieldedUtxo>,
    pub spent_unshielded_utxos: Vec<UnshieldedUtxo>,
    pub dust_events: Vec<DustEvent>,
    pub dust_operations: ProcessedDustEvents,
}

impl From<node::RegularTransaction> for RegularTransaction {
    fn from(transaction: node::RegularTransaction) -> Self {
        Self {
            hash: transaction.hash,
            protocol_version: transaction.protocol_version,
            identifiers: transaction.identifiers,
            raw: transaction.raw,
            contract_actions: transaction.contract_actions,
            paid_fees: transaction.paid_fees,
            estimated_fees: transaction.estimated_fees,
            transaction_result: Default::default(),
            merkle_tree_root: Default::default(),
            start_index: Default::default(),
            end_index: Default::default(),
            created_unshielded_utxos: Default::default(),
            spent_unshielded_utxos: Default::default(),
            dust_events: Vec::default(),
            dust_operations: ProcessedDustEvents {
                generations: Vec::new(),
                utxos: Vec::new(),
                merkle_tree_updates: Vec::new(),
                spends: Vec::new(),
                dtime_update: None,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemTransaction {
    pub hash: TransactionHash,
    pub protocol_version: ProtocolVersion,
    pub raw: SerializedTransaction,
    // DUST events from system transactions (e.g., CNightGeneratesDustUpdate).
    pub dust_events: Vec<DustEvent>,
    pub dust_operations: ProcessedDustEvents,
    // Additional processed data from system transactions.
    pub reserve_distribution: Option<u128>,
    pub parameter_update: Option<SerializedParameterUpdate>,
    pub night_distribution: Option<(String, SerializedNightOutputs)>,
    pub treasury_income: Option<(u128, String)>,
    pub treasury_payment_shielded: Option<SerializedTreasuryOutputs>,
    pub treasury_payment_unshielded: Option<SerializedTreasuryOutputs>,
}

/// Error type for system transaction processing.
#[derive(Debug, Error)]
pub enum SystemTransactionError {
    #[error("cannot deserialize system transaction: {0}")]
    DeserializationError(String),

    #[error("cannot process system transaction: {0}")]
    ProcessingError(String),
}

impl TryFrom<node::SystemTransaction> for SystemTransaction {
    type Error = SystemTransactionError;

    fn try_from(transaction: node::SystemTransaction) -> Result<Self, Self::Error> {
        // Deserialize the raw transaction to process it.
        let domain_tx =
            DomainSystemTransaction::deserialize(&transaction.raw, transaction.protocol_version)
                .map_err(|error| SystemTransactionError::DeserializationError(error.to_string()))?;

        // Process the transaction to extract metadata.
        let (
            dust_events,
            reserve_distribution,
            parameter_update,
            night_distribution,
            treasury_income,
            treasury_payment_shielded,
            treasury_payment_unshielded,
        ) = process_system_transaction(&domain_tx, &transaction.hash)?;

        // Process DUST events to determine storage operations.
        let dust_operations = crate::domain::process_dust_events(&dust_events);

        Ok(Self {
            hash: transaction.hash,
            protocol_version: transaction.protocol_version,
            raw: transaction.raw,
            dust_events,
            dust_operations,
            reserve_distribution,
            parameter_update,
            night_distribution,
            treasury_income,
            treasury_payment_shielded,
            treasury_payment_unshielded,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Type)]
#[cfg_attr(feature = "cloud", sqlx(type_name = "TRANSACTION_VARIANT"))]
pub enum TransactionVariant {
    Regular,
    System,
}

/// All serialized transactions from a single block along with metadata needed for ledger state
/// application.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct BlockTransactions {
    pub transactions: Vec<(TransactionVariant, SerializedTransaction)>,

    #[sqlx(try_from = "i64")]
    pub protocol_version: ProtocolVersion,

    pub block_parent_hash: ByteArray<32>,

    #[sqlx(try_from = "i64")]
    pub block_timestamp: u64,
}

/// Result type for system transaction processing.
type SystemTransactionProcessingResult = (
    Vec<DustEvent>,
    Option<u128>,
    Option<SerializedParameterUpdate>,
    Option<(String, SerializedNightOutputs)>,
    Option<(u128, String)>,
    Option<SerializedTreasuryOutputs>,
    Option<SerializedTreasuryOutputs>,
);

/// Process a system transaction to extract all relevant metadata.
fn process_system_transaction(
    domain_tx: &DomainSystemTransaction,
    tx_hash: &TransactionHash,
) -> Result<SystemTransactionProcessingResult, SystemTransactionError> {
    use indexer_common::domain::ledger::SystemTransaction;
    use midnight_ledger_v6::structure::SystemTransaction as LedgerSystemTransactionV6;

    match domain_tx {
        SystemTransaction::V6(ledger_tx) => {
            let mut dust_events = Vec::new();
            let mut reserve_distribution = None;
            let mut parameter_update = None;
            let mut night_distribution = None;
            let mut treasury_income = None;
            let mut treasury_payment_shielded = None;
            let mut treasury_payment_unshielded = None;

            match ledger_tx {
                LedgerSystemTransactionV6::CNightGeneratesDustUpdate { events } => {
                    dust_events = convert_cnight_events_to_dust_events(events, tx_hash);
                }

                LedgerSystemTransactionV6::DistributeReserve(amount) => {
                    reserve_distribution = Some(*amount);
                }

                LedgerSystemTransactionV6::OverwriteParameters(params) => {
                    // Serialize actual parameters as JSON.
                    #[derive(Serialize)]
                    struct ParameterData {
                        night_dust_ratio: u64,
                        generation_decay_rate: u32,
                        dust_grace_period_seconds: u64,
                    }
                    let param_data = ParameterData {
                        night_dust_ratio: params.dust.night_dust_ratio,
                        generation_decay_rate: params.dust.generation_decay_rate,
                        dust_grace_period_seconds: params.dust.dust_grace_period.as_seconds()
                            as u64,
                    };
                    parameter_update = Some(
                        serde_json::to_vec(&param_data)
                            .unwrap_or_else(|_| b"parameter_update".to_vec())
                            .into(),
                    );
                }

                LedgerSystemTransactionV6::DistributeNight(claim_kind, outputs) => {
                    // Serialize distribution data as JSON.
                    #[derive(Serialize)]
                    struct DistributionData {
                        output_count: usize,
                        claim_type: String,
                        total_amount: u128,
                    }
                    let total: u128 = outputs.iter().map(|o| o.amount).sum();
                    let claim_type = format!("{:?}", claim_kind);
                    let dist_data = DistributionData {
                        output_count: outputs.len(),
                        claim_type: claim_type.clone(),
                        total_amount: total,
                    };
                    night_distribution = Some((
                        claim_type,
                        serde_json::to_vec(&dist_data)
                            .unwrap_or_else(|_| b"night_distribution".to_vec())
                            .into(),
                    ));
                }

                LedgerSystemTransactionV6::PayBlockRewardsToTreasury { amount } => {
                    treasury_income = Some((*amount, "block_rewards".to_string()));
                }

                LedgerSystemTransactionV6::PayFromTreasuryShielded {
                    outputs,
                    nonce,
                    token_type,
                } => {
                    // Serialize shielded outputs as JSON.
                    #[derive(Serialize)]
                    struct ShieldedOutputData {
                        output_count: usize,
                        payment_type: String,
                        nonce: Vec<u8>,
                        token_type: String,
                    }
                    let output_data = ShieldedOutputData {
                        output_count: outputs.len(),
                        payment_type: "shielded".to_string(),
                        nonce: nonce.0.to_vec(),
                        token_type: format!("{:?}", token_type),
                    };
                    treasury_payment_shielded = Some(
                        serde_json::to_vec(&output_data)
                            .unwrap_or_else(|_| b"treasury_shielded".to_vec())
                            .into(),
                    );
                }

                LedgerSystemTransactionV6::PayFromTreasuryUnshielded {
                    outputs,
                    token_type,
                } => {
                    // Serialize unshielded outputs as JSON.
                    #[derive(Serialize)]
                    struct UnshieldedOutputData {
                        output_count: usize,
                        payment_type: String,
                        total_amount: u128,
                        token_type: String,
                    }
                    let total: u128 = outputs.iter().map(|o| o.amount).sum();
                    let output_data = UnshieldedOutputData {
                        output_count: outputs.len(),
                        payment_type: "unshielded".to_string(),
                        total_amount: total,
                        token_type: format!("{:?}", token_type),
                    };
                    treasury_payment_unshielded = Some(
                        serde_json::to_vec(&output_data)
                            .unwrap_or_else(|_| b"treasury_unshielded".to_vec())
                            .into(),
                    );
                }

                // LedgerSystemTransactionV6 is #[non_exhaustive].
                #[allow(unreachable_patterns)]
                _ => {
                    log::warn!(
                        "encountered new system transaction variant in tx {}: {:?}",
                        tx_hash,
                        std::any::type_name_of_val(&ledger_tx)
                    );
                }
            }

            Ok((
                dust_events,
                reserve_distribution,
                parameter_update,
                night_distribution,
                treasury_income,
                treasury_payment_shielded,
                treasury_payment_unshielded,
            ))
        }
    }
}

/// Convert CNightGeneratesDust events to domain DUST events.
fn convert_cnight_events_to_dust_events(
    events: &[midnight_ledger_v6::structure::CNightGeneratesDustEvent],
    tx_hash: &TransactionHash,
) -> Vec<DustEvent> {
    use midnight_ledger_v6::structure::CNightGeneratesDustActionType;

    events
        .iter()
        .enumerate()
        .map(|(index, event)| {
            let owner_bytes = event.owner.0.as_le_bytes();
            let owner =
                DustOwner::try_from(owner_bytes).expect("dust public key should be 32 bytes");
            let nonce = DustNonce::from(event.nonce.0.0);
            let event_details = match event.action {
                CNightGeneratesDustActionType::Create => DustEventDetails::DustInitialUtxo {
                    output: QualifiedDustOutput {
                        initial_value: event.value,
                        owner,
                        nonce,
                        seq: 0,
                        ctime: event.time.to_secs(),
                        backing_night: NightUtxoNonce::default(),
                        mt_index: 0,
                    },
                    generation_info: DustGenerationInfo {
                        value: event.value,
                        owner,
                        nonce,
                        ctime: event.time.to_secs(),
                        dtime: u64::MAX,
                        night_utxo_hash: NightUtxoNonce::default(),
                    },
                    generation_index: index as u64,
                },

                CNightGeneratesDustActionType::Destroy => DustEventDetails::DustSpendProcessed {
                    commitment: DustCommitment::default(),
                    commitment_index: 0,
                    nullifier: DustNullifier::default(),
                    v_fee: 0,
                    time: event.time.to_secs(),
                    params: DustParameters::default(),
                },
            };

            DustEvent {
                transaction_hash: *tx_hash,
                logical_segment: index as u16,
                physical_segment: index as u16,
                event_details,
            }
        })
        .collect()
}
