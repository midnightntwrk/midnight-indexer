// This file is part of midnight-indexer.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::domain::{
    ByteVec, DustNonce, DustOwner, NightUtxoNonce,
    dust::{
        DustCommitment, DustEvent, DustEventDetails, DustGenerationInfo, DustNullifier,
        DustParameters, QualifiedDustOutput,
    },
    ledger::{SystemTransaction, TransactionHash},
};
use midnight_ledger_v6::structure::{
    CNightGeneratesDustActionType, CNightGeneratesDustEvent,
    SystemTransaction as LedgerSystemTransactionV6,
};
use serde::Serialize;

/// Serialized parameter update data.
pub type SerializedParameterUpdate = ByteVec;

/// Serialized treasury payment outputs.
pub type SerializedTreasuryOutputs = ByteVec;

/// Serialized NIGHT distribution outputs.
pub type SerializedNightOutputs = ByteVec;

/// Processed data from a system transaction.
#[derive(Debug)]
pub struct ProcessedSystemTransaction {
    pub dust_events: Vec<DustEvent>,
    pub reserve_distribution: Option<u128>,
    pub parameter_update: Option<SerializedParameterUpdate>,
    pub night_distribution: Option<(String, SerializedNightOutputs)>,
    pub treasury_income: Option<(u128, String)>,
    pub treasury_payment_shielded: Option<SerializedTreasuryOutputs>,
    pub treasury_payment_unshielded: Option<SerializedTreasuryOutputs>,
}

impl SystemTransaction {
    /// Process a system transaction and extract relevant domain data.
    pub fn process(&self, tx_hash: &TransactionHash) -> ProcessedSystemTransaction {
        match self {
            SystemTransaction::V6(ledger_tx) => process_v6_system_transaction(ledger_tx, tx_hash),
        }
    }
}

fn process_v6_system_transaction(
    ledger_tx: &LedgerSystemTransactionV6,
    tx_hash: &TransactionHash,
) -> ProcessedSystemTransaction {
    let mut result = ProcessedSystemTransaction {
        dust_events: Vec::new(),
        reserve_distribution: None,
        parameter_update: None,
        night_distribution: None,
        treasury_income: None,
        treasury_payment_shielded: None,
        treasury_payment_unshielded: None,
    };

    match ledger_tx {
        LedgerSystemTransactionV6::CNightGeneratesDustUpdate { events } => {
            result.dust_events = convert_cnight_events_to_dust_events(events, tx_hash);
        }

        LedgerSystemTransactionV6::DistributeReserve(amount) => {
            result.reserve_distribution = Some(*amount);
        }

        LedgerSystemTransactionV6::OverwriteParameters(params) => {
            // Serialize actual parameters as JSON.
            // Since LedgerParameters doesn't implement Serialize, we extract key fields.
            #[derive(Serialize)]
            struct ParameterData {
                night_dust_ratio: u64,
                generation_decay_rate: u32,
                dust_grace_period_seconds: u64,
            }
            let param_data = ParameterData {
                night_dust_ratio: params.dust.night_dust_ratio,
                generation_decay_rate: params.dust.generation_decay_rate,
                dust_grace_period_seconds: params.dust.dust_grace_period.as_seconds() as u64,
            };
            result.parameter_update = Some(
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
            // Convert claim kind to string representation.
            let claim_type = format!("{:?}", claim_kind);
            let dist_data = DistributionData {
                output_count: outputs.len(),
                claim_type: claim_type.to_string(),
                total_amount: total,
            };
            result.night_distribution = Some((
                claim_type.to_string(),
                serde_json::to_vec(&dist_data)
                    .unwrap_or_else(|_| b"night_distribution".to_vec())
                    .into(),
            ));
        }

        LedgerSystemTransactionV6::PayBlockRewardsToTreasury { amount } => {
            result.treasury_income = Some((*amount, "block_rewards".to_string()));
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
            result.treasury_payment_shielded = Some(
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
            result.treasury_payment_unshielded = Some(
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

    result
}

/// Convert CNightGeneratesDust events to domain DUST events.
fn convert_cnight_events_to_dust_events(
    events: &[CNightGeneratesDustEvent],
    tx_hash: &TransactionHash,
) -> Vec<DustEvent> {
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
