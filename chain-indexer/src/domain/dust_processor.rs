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
use indexer_common::domain::dust::{DustEvent, DustEventDetails, DustGenerationInfo, DustUtxo};
use log::{info, warn};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DustProcessingError {
    #[error("Database error during DUST processing")]
    Database(#[from] sqlx::Error),
    #[error("Invalid DUST event data: {0}")]
    InvalidEventData(String),
    #[error("DUST generation info not found for index {0}")]
    GenerationInfoNotFound(u64),
}

pub struct DustEventProcessor;

impl DustEventProcessor {
    /// Process all DUST events from a transaction and update the database
    pub async fn process_transaction_dust_events(
        storage: &impl Storage,
        transaction: &Transaction,
    ) -> Result<(), DustProcessingError> {
        if transaction.dust_events.is_empty() {
            return Ok(());
        }

        info!(
            "Processing {} DUST events for transaction {}",
            transaction.dust_events.len(),
            const_hex::encode(transaction.hash.0)
        );

        // Group events by type for efficient processing
        let mut initial_utxos = Vec::new();
        let mut generation_updates = Vec::new();
        let mut spend_events = Vec::new();

        for event in &transaction.dust_events {
            match &event.event_details {
                DustEventDetails::DustInitialUtxo {
                    output,
                    generation,
                    generation_index,
                } => {
                    initial_utxos.push((event, output, generation, *generation_index));
                }
                DustEventDetails::DustGenerationDtimeUpdate {
                    generation,
                    generation_index,
                } => {
                    generation_updates.push((event, generation, *generation_index));
                }
                DustEventDetails::DustSpendProcessed {
                    commitment,
                    nullifier,
                    v_fee,
                    ..
                } => {
                    spend_events.push((event, commitment, nullifier, *v_fee));
                }
                _ => {
                    // Handle any future event types
                    warn!("Unhandled DUST event type");
                }
            }
        }

        // Process initial DUST UTXOs
        if !initial_utxos.is_empty() {
            Self::process_initial_utxos(storage, &initial_utxos, transaction.id).await?;
        }

        // Process generation time updates
        if !generation_updates.is_empty() {
            Self::process_generation_updates(storage, &generation_updates).await?;
        }

        // Process DUST spends
        if !spend_events.is_empty() {
            Self::process_dust_spends(storage, &spend_events, transaction.id as i64).await?;
        }

        // Save all events for audit trail
        storage
            .save_dust_events(&transaction.dust_events, transaction.id as i64)
            .await?;

        info!(
            "Successfully processed DUST events for transaction {}",
            const_hex::encode(transaction.hash.0)
        );

        Ok(())
    }

    async fn process_initial_utxos(
        storage: &impl Storage,
        initial_utxos: &[(
            &DustEvent,
            &indexer_common::domain::dust::QualifiedDustOutput,
            &DustGenerationInfo,
            u64,
        )],
        _transaction_id: u64,
    ) -> Result<(), DustProcessingError> {
        let mut generation_infos = Vec::new();
        let mut dust_utxos = Vec::new();

        for (_event, output, generation, generation_index) in initial_utxos {
            // Create generation info entry
            let generation_info = DustGenerationInfo {
                value: generation.value,
                owner: generation.owner,
                nonce: generation.nonce,
                ctime: generation.ctime,
                dtime: generation.dtime,
            };
            generation_infos.push(generation_info);

            // Create DUST UTXO entry
            let dust_utxo = DustUtxo {
                // TODO: Calculate proper commitment from output fields once ledger API provides it.
                // For now using owner as placeholder which is incorrect.
                commitment: indexer_common::domain::ByteArray(output.owner.0),
                nullifier: None, // Not spent yet
                initial_value: output.initial_value,
                owner: output.owner,
                nonce: output.nonce,
                seq: output.seq,
                ctime: output.ctime,
                generation_info_id: Some(*generation_index),
                spent_at_transaction_id: None,
            };
            dust_utxos.push(dust_utxo);
        }

        // Save generation info first
        if !generation_infos.is_empty() {
            storage.save_dust_generation_info(&generation_infos).await?;
        }

        // Then save DUST UTXOs
        if !dust_utxos.is_empty() {
            storage.save_dust_utxos(&dust_utxos).await?;
        }

        Ok(())
    }

    async fn process_generation_updates(
        storage: &impl Storage,
        generation_updates: &[(&DustEvent, &DustGenerationInfo, u64)],
    ) -> Result<(), DustProcessingError> {
        for (_event, generation, generation_index) in generation_updates {
            // Update the dtime for this generation info
            storage
                .update_dust_generation_dtime(*generation_index, generation.dtime)
                .await?;
        }

        Ok(())
    }

    async fn process_dust_spends(
        storage: &impl Storage,
        spend_events: &[(
            &DustEvent,
            &indexer_common::domain::ByteArray<32>,
            &indexer_common::domain::ByteArray<32>,
            u128,
        )],
        transaction_id: i64,
    ) -> Result<(), DustProcessingError> {
        for (_event, commitment, nullifier, _v_fee) in spend_events {
            // Mark the DUST UTXO as spent
            storage
                .mark_dust_utxo_spent(&commitment.0, &nullifier.0, transaction_id)
                .await?;
        }

        Ok(())
    }

    /// Process system transactions that might contain registration changes
    /// This is called for transactions received from the node's native-token-observation pallet
    pub async fn process_system_transaction_registrations(
        _storage: &impl Storage,
        _transaction: &Transaction,
    ) -> Result<(), DustProcessingError> {
        // Note: This would be implemented once we have the system transaction format
        // from Justin's node work. For now, we'll process registrations via events
        // or direct node communication.

        // Placeholder for future implementation when node system transactions
        // include registration data
        warn!("System transaction registration processing not yet implemented");

        Ok(())
    }
}

