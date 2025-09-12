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

use indexer_common::domain::dust::{
    DustCommitment, DustEvent, DustEventDetails, DustGenerationInfo, DustMerklePathEntry,
    DustNullifier, QualifiedDustOutput,
};

/// Storage operations needed for processing DUST events.
#[derive(Debug)]
pub struct DustEventStorageOperations {
    pub generation_saves: Vec<GenerationSave>,
    pub utxo_saves: Vec<UtxoSave>,
    pub tree_updates: Vec<TreeUpdate>,
    pub spent_marks: Vec<SpentMark>,
    pub dtime_update: Option<DtimeUpdate>,
}

#[derive(Debug)]
pub struct GenerationSave {
    pub generation_info: DustGenerationInfo,
    pub generation_index: u64,
}

#[derive(Debug)]
pub struct UtxoSave {
    pub output: QualifiedDustOutput,
    pub generation_info_id: i64,
}

#[derive(Debug)]
pub struct TreeUpdate {
    pub generation_index: u64,
    pub merkle_path: Vec<DustMerklePathEntry>,
    pub block_height: u32,
}

#[derive(Debug)]
pub struct SpentMark {
    pub commitment: DustCommitment,
    pub nullifier: DustNullifier,
    pub transaction_id: i64,
}

#[derive(Debug)]
pub struct DtimeUpdate {
    pub dtime: u64,
    pub generation_index: u64,
}

/// Process DUST events and determine what storage operations are needed.
pub fn process_dust_events(
    dust_events: &[DustEvent],
    transaction_id: i64,
    block_height: u32,
) -> DustEventStorageOperations {
    let mut operations = DustEventStorageOperations {
        generation_saves: Vec::new(),
        utxo_saves: Vec::new(),
        tree_updates: Vec::new(),
        spent_marks: Vec::new(),
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
                operations.generation_saves.push(GenerationSave {
                    generation_info: *generation_info,
                    generation_index: *generation_index,
                });
                // Note: generation_info_id will be determined during storage.
                operations.utxo_saves.push(UtxoSave {
                    output: *output,
                    generation_info_id: 0, // Placeholder, will be set during actual save.
                });
            }

            DustEventDetails::DustGenerationDtimeUpdate {
                generation_info,
                generation_index,
                merkle_path,
            } => {
                generation_dtime_and_index = Some((generation_info.dtime, *generation_index));
                operations.tree_updates.push(TreeUpdate {
                    generation_index: *generation_index,
                    merkle_path: merkle_path.clone(),
                    block_height,
                });
            }

            DustEventDetails::DustSpendProcessed {
                commitment,
                nullifier,
                ..
            } => {
                operations.spent_marks.push(SpentMark {
                    commitment: *commitment,
                    nullifier: *nullifier,
                    transaction_id,
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
        operations.dtime_update = Some(DtimeUpdate {
            dtime,
            generation_index: index,
        });
    }

    operations
}
