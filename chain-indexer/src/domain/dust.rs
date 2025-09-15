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

pub use indexer_common::domain::dust::{
    DustEvent, DustEventDetails, DustEventType, DustGenerationInfo, DustMerklePathEntry,
    DustParameters, QualifiedDustOutput,
};

use indexer_common::domain::{
    CardanoStakeKey, DustAddress, DustCommitment, DustNullifier, DustUtxoId,
};
use thiserror::Error;

/// Domain representation of DUST registration events from the NativeTokenObservation pallet.
#[derive(Debug, Clone, PartialEq)]
pub enum DustRegistrationEvent {
    /// Cardano address registered with DUST address.
    Registration {
        cardano_address: CardanoStakeKey,
        dust_address: DustAddress,
    },

    /// Cardano address deregistered from DUST address.
    Deregistration {
        cardano_address: CardanoStakeKey,
        dust_address: DustAddress,
    },

    /// UTXO mapping added for registration.
    MappingAdded {
        cardano_address: CardanoStakeKey,
        dust_address: DustAddress,
        utxo_id: DustUtxoId,
    },

    /// UTXO mapping removed from registration.
    MappingRemoved {
        cardano_address: CardanoStakeKey,
        dust_address: DustAddress,
        utxo_id: DustUtxoId,
    },
}

#[derive(Error, Debug)]
pub enum DustProcessingError {
    #[error("Database error during DUST processing")]
    Database(#[from] sqlx::Error),

    #[error("Invalid DUST event data: {0}")]
    InvalidEventData(String),

    #[error("DUST generation info not found for index {0}")]
    GenerationInfoNotFound(u64),
}

/// Processed DUST events ready for persistence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessedDustEvents {
    pub generations: Vec<DustGeneration>,
    pub utxos: Vec<DustUtxo>,
    pub merkle_tree_updates: Vec<DustMerkleTreeUpdate>,
    pub spends: Vec<DustSpend>,
    pub dtime_update: Option<DustDtimeUpdate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DustGeneration {
    pub generation_info: DustGenerationInfo,
    pub generation_index: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DustUtxo {
    pub output: QualifiedDustOutput,
    pub generation_index: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DustMerkleTreeUpdate {
    pub generation_index: u64,
    pub merkle_path: Vec<DustMerklePathEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DustSpend {
    pub commitment: DustCommitment,
    pub nullifier: DustNullifier,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DustDtimeUpdate {
    pub dtime: u64,
    pub generation_index: u64,
}

/// Extract operations from DUST events.
/// This processing happens in the domain layer.
pub fn extract_dust_operations(dust_events: &[DustEvent]) -> ProcessedDustEvents {
    let mut result = ProcessedDustEvents {
        generations: Vec::new(),
        utxos: Vec::new(),
        merkle_tree_updates: Vec::new(),
        spends: Vec::new(),
        dtime_update: None,
    };

    let mut generation_dtime_and_index = None;

    for dust_event in dust_events {
        match &dust_event.event_details {
            DustEventDetails::DustInitialUtxo {
                output,
                generation_info,
                generation_index,
            } => {
                result.generations.push(DustGeneration {
                    generation_info: *generation_info,
                    generation_index: *generation_index,
                });
                result.utxos.push(DustUtxo {
                    output: *output,
                    generation_index: *generation_index,
                });
            }

            DustEventDetails::DustGenerationDtimeUpdate {
                generation_info,
                generation_index,
                merkle_path,
            } => {
                generation_dtime_and_index = Some((generation_info.dtime, *generation_index));
                result.merkle_tree_updates.push(DustMerkleTreeUpdate {
                    generation_index: *generation_index,
                    merkle_path: merkle_path.clone(),
                });
            }

            DustEventDetails::DustSpendProcessed {
                commitment,
                nullifier,
                ..
            } => {
                result.spends.push(DustSpend {
                    commitment: *commitment,
                    nullifier: *nullifier,
                });
            }

            // Registration events are handled at block level.
            DustEventDetails::DustRegistration { .. }
            | DustEventDetails::DustDeregistration { .. }
            | DustEventDetails::DustMappingAdded { .. }
            | DustEventDetails::DustMappingRemoved { .. } => {
                // Intentionally empty - already handled at block level.
            }
        }
    }

    if let Some((dtime, index)) = generation_dtime_and_index {
        result.dtime_update = Some(DustDtimeUpdate {
            dtime,
            generation_index: index,
        });
    }

    result
}
