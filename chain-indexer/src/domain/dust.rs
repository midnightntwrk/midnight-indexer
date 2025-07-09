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

use crate::domain::{Transaction, storage::Storage};
use indexer_common::domain::{
    ByteArray,
    dust::{DustEvent, DustEventDetails, DustGenerationInfo, DustUtxo},
};
use log::debug;
use thiserror::Error;

// Type aliases for event grouping to improve readability.
type InitialUtxoEvent<'a> = (
    &'a indexer_common::domain::dust::QualifiedDustOutput,
    &'a DustGenerationInfo,
    u64,
);
type GenerationUpdateEvent<'a> = (&'a DustGenerationInfo, u64);
type SpendEvent = (ByteArray<32>, ByteArray<32>, u128);

#[derive(Error, Debug)]
pub enum DustProcessingError {
    #[error("Database error during DUST processing")]
    Database(#[from] sqlx::Error),

    #[error("Invalid DUST event data: {0}")]
    InvalidEventData(String),

    #[error("DUST generation info not found for index {0}")]
    GenerationInfoNotFound(u64),
}

/// Process all DUST events from a transaction and update the database.
pub async fn process_dust_events(
    transaction: &Transaction,
    storage: &impl Storage,
) -> Result<(), DustProcessingError> {
    if transaction.dust_events.is_empty() {
        return Ok(());
    }

    debug!(
        dust_event_count = transaction.dust_events.len(),
        transaction_hash:% = transaction.hash;
        "processing DUST events"
    );

    // Group events by type for efficient processing.
    let (initial_utxos, generation_updates, spend_events) =
        group_events_by_type(&transaction.dust_events);

    // Process initial DUST UTXOs.
    if !initial_utxos.is_empty() {
        process_initial_utxos(storage, initial_utxos).await?;
    }

    // Process generation time updates.
    if !generation_updates.is_empty() {
        // Update the dtime for each generation info.
        for (generation, generation_index) in generation_updates {
            storage
                .update_dust_generation_dtime(generation_index, generation.dtime)
                .await?;
        }
    }

    // Process DUST spends.
    if !spend_events.is_empty() {
        // Mark each DUST UTXO as spent.
        for (commitment, nullifier, _v_fee) in spend_events {
            storage
                .mark_dust_utxo_spent(commitment, nullifier, transaction.id)
                .await?;
        }
    }

    // Save all events.
    storage
        .save_dust_events(&transaction.dust_events, transaction.id)
        .await?;

    debug!(
        transaction_hash:% = transaction.hash;
        "successfully processed DUST events"
    );

    Ok(())
}

fn group_events_by_type(
    events: &[DustEvent],
) -> (
    Vec<InitialUtxoEvent>,
    Vec<GenerationUpdateEvent>,
    Vec<SpendEvent>,
) {
    events.iter().fold(
        (Vec::new(), Vec::new(), Vec::new()),
        |(mut initial, mut updates, mut spends), event| {
            match &event.event_details {
                DustEventDetails::DustInitialUtxo {
                    output,
                    generation,
                    generation_index,
                } => {
                    initial.push((output, generation, *generation_index));
                }

                DustEventDetails::DustGenerationDtimeUpdate {
                    generation,
                    generation_index,
                } => {
                    updates.push((generation, *generation_index));
                }

                DustEventDetails::DustSpendProcessed {
                    commitment,
                    nullifier,
                    v_fee,
                    ..
                } => {
                    spends.push((*commitment, *nullifier, *v_fee));
                }
            }

            (initial, updates, spends)
        },
    )
}

async fn process_initial_utxos(
    storage: &impl Storage,
    initial_utxos: Vec<InitialUtxoEvent<'_>>,
) -> Result<(), DustProcessingError> {
    let (generation_infos, dust_utxos) = initial_utxos
        .into_iter()
        .map(|(output, generation, generation_index)| {
            let generation_info = *generation;

            let dust_utxo = DustUtxo {
                // TODO: Calculate proper commitment from output fields once ledger API provides it.
                // For now using owner as placeholder which is incorrect.
                commitment: indexer_common::domain::ByteArray(output.owner.0),
                nullifier: None, // Not spent yet.
                initial_value: output.initial_value,
                owner: output.owner,
                nonce: output.nonce,
                seq: output.seq,
                ctime: output.ctime,
                generation_info_id: Some(generation_index),
                spent_at_transaction_id: None,
            };

            (generation_info, dust_utxo)
        })
        .unzip::<_, _, Vec<_>, Vec<_>>();

    // Save generation info first.
    if !generation_infos.is_empty() {
        storage.save_dust_generation_info(&generation_infos).await?;
    }

    // Then save DUST UTXOs.
    if !dust_utxos.is_empty() {
        storage.save_dust_utxos(&dust_utxos).await?;
    }

    Ok(())
}
