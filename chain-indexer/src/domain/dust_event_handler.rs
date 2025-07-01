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

use crate::domain::{Block, DustMerkleTreeManager, Transaction, storage::Storage};
use indexer_common::domain::{
    ByteArray, ByteVec,
    dust::{
        DustEvent, DustEventDetails, DustGenerationInfo, DustParameters, DustRegistration, DustUtxo,
    },
};
use log::{debug, info, warn};
use std::collections::HashMap;
use thiserror::Error;

/// Type alias for dust spend tuple to reduce complexity.
type DustSpendTuple<'a> = (
    u64,
    &'a DustEvent,
    &'a ByteArray<32>,
    u64,
    &'a ByteArray<32>,
    u128,
    u64,
    &'a DustParameters,
);

/// Errors that can occur during DUST event handling.
#[derive(Debug, Error)]
pub enum DustEventError {
    #[error("database error")]
    Database(#[from] sqlx::Error),

    #[error("merkle tree error")]
    MerkleTree(#[from] crate::domain::MerkleTreeError),

    #[error("invalid event data: {0}")]
    InvalidEventData(String),
}

/// Enhanced DUST event handler that processes events following the conceptual architecture.
pub struct DustEventHandler<S: Storage> {
    storage: S,
    merkle_tree_manager: DustMerkleTreeManager<S>,
    #[allow(dead_code)]
    dust_parameters: DustParameters,
}

impl<S: Storage> DustEventHandler<S> {
    /// Create a new DUST event handler.
    pub fn new(storage: S, merkle_tree_batch_size: usize) -> Self {
        let merkle_tree_manager =
            DustMerkleTreeManager::new(storage.clone(), merkle_tree_batch_size);

        // TODO(sean): Load parameters from configuration or chain state.
        let dust_parameters = DustParameters {
            night_dust_ratio: 10,
            generation_decay_rate: 3600,
            dust_grace_period: 300,
        };

        Self {
            storage,
            merkle_tree_manager,
            dust_parameters,
        }
    }

    /// Process all DUST-related events from a block.
    ///
    /// Assumes ledger provides events in correct order within transactions.
    pub async fn process_block_dust_events(&self, block: &Block) -> Result<(), DustEventError> {
        if !block
            .transactions
            .iter()
            .any(|tx| !tx.dust_events.is_empty())
        {
            return Ok(());
        }

        info!(
            block_height = block.height,
            block_hash:% = block.hash;
            "processing DUST events for block"
        );

        // Collect all events from the block grouped by type.
        let mut initial_utxos = vec![];
        let mut generation_updates = vec![];
        let mut dust_spends = vec![];
        let mut commitment_updates = vec![];
        let mut generation_tree_updates = vec![];

        for transaction in &block.transactions {
            if transaction.dust_events.is_empty() {
                continue;
            }

            debug!(
                transaction_hash:% = transaction.hash,
                events_count = transaction.dust_events.len();
                "processing transaction DUST events"
            );

            for event in &transaction.dust_events {
                match &event.event_details {
                    DustEventDetails::DustInitialUtxo {
                        output,
                        generation,
                        generation_index,
                    } => {
                        initial_utxos.push((
                            transaction.id,
                            event,
                            output,
                            generation,
                            *generation_index,
                        ));

                        // Track for Merkle tree updates.
                        // Assumes ledger provides proper commitment calculation.
                        // TEMPORARY: Mock placeholder - will be replaced with real data from ledger-5.0.0-alpha.3+.
                        // This mock commitment will be deleted once we have a node image with proper DUST events.
                        let commitment = ByteVec::from(vec![0u8; 32]); // TODO(sean): Get from event.
                        commitment_updates.push((*generation_index, commitment.clone()));

                        // TEMPORARY: Mock placeholder - will be replaced with real serialization from ledger-5.0.0-alpha.3+.
                        // This mock generation data will be deleted once we have a node image with proper DUST events.
                        let generation_data = ByteVec::from(vec![0u8; 64]); // TODO(sean): Serialize generation.
                        generation_tree_updates.push((*generation_index, generation_data));
                    }

                    DustEventDetails::DustGenerationDtimeUpdate {
                        generation,
                        generation_index,
                    } => {
                        generation_updates.push((event, generation, *generation_index));
                    }

                    DustEventDetails::DustSpendProcessed {
                        commitment,
                        commitment_index,
                        nullifier,
                        v_fee,
                        time,
                        params,
                    } => {
                        dust_spends.push((
                            transaction.id,
                            event,
                            commitment,
                            *commitment_index,
                            nullifier,
                            *v_fee,
                            *time,
                            params,
                        ));
                    }

                    _ => {
                        warn!(
                            event_type:? = event.event_details;
                            "unhandled DUST event type"
                        );
                    }
                }
            }
        }

        // Process events in logical order.

        // 1. Process initial DUST UTXOs (new DUST created).
        if !initial_utxos.is_empty() {
            self.process_initial_utxos(&initial_utxos).await?;
        }

        // 2. Process generation updates (Night spent, decay starts).
        if !generation_updates.is_empty() {
            self.process_generation_updates(&generation_updates).await?;
        }

        // 3. Process DUST spends.
        if !dust_spends.is_empty() {
            self.process_dust_spends(&dust_spends).await?;
        }

        // 4. Update Merkle trees.
        if !commitment_updates.is_empty() {
            // Assumes ledger provides tree root for this block.
            // TEMPORARY: Mock placeholder - will be replaced with real root from block in ledger-5.0.0-alpha.3+.
            // This mock root will be deleted once we have a node image with proper DUST support.
            let commitment_root = ByteVec::from(vec![0u8; 32]); // TODO(sean): Get from block.
            self.merkle_tree_manager
                .update_commitment_tree(commitment_updates, block.height, commitment_root)
                .await?;
        }

        if !generation_tree_updates.is_empty() {
            // Assumes ledger provides tree root for this block.
            // TEMPORARY: Mock placeholder - will be replaced with real root from block in ledger-5.0.0-alpha.3+.
            // This mock root will be deleted once we have a node image with proper DUST support.
            let generation_root = ByteVec::from(vec![0u8; 32]); // TODO(sean): Get from block.
            self.merkle_tree_manager
                .update_generation_tree(generation_tree_updates, block.height, generation_root)
                .await?;
        }

        Ok(())
    }

    /// Process system transactions that contain registration changes.
    ///
    /// Assumes node creates system transactions with registration data.
    pub async fn process_registration_system_transaction(
        &self,
        transaction: &Transaction,
    ) -> Result<(), DustEventError> {
        // TODO(sean): Extract registration data from system transaction once format is defined.
        // System transactions will contain:
        // - Cardano address (Night holder)
        // - DUST address (recipient)
        // - Registration action (add/remove)

        warn!(
            transaction_hash:% = transaction.hash;
            "registration system transaction processing not yet implemented"
        );

        Ok(())
    }

    async fn process_initial_utxos(
        &self,
        initial_utxos: &[(
            u64,
            &DustEvent,
            &indexer_common::domain::dust::QualifiedDustOutput,
            &DustGenerationInfo,
            u64,
        )],
    ) -> Result<(), DustEventError> {
        let mut generation_infos: Vec<&DustGenerationInfo> = vec![];
        let mut dust_utxos = vec![];

        for (_transaction_id, _event, output, generation, generation_index) in initial_utxos {
            // Store generation info.
            generation_infos.push(*generation);

            // Create DUST UTXO.
            let dust_utxo = DustUtxo {
                // TODO(sean): Calculate proper commitment once ledger provides it.
                commitment: ByteArray(output.owner.0),
                nullifier: None,
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

        // Save to storage.
        if !generation_infos.is_empty() {
            self.storage
                .save_dust_generation_info(&generation_infos)
                .await?;
        }

        if !dust_utxos.is_empty() {
            self.storage.save_dust_utxos(&dust_utxos).await?;
        }

        info!(
            generation_count = generation_infos.len(),
            utxo_count = dust_utxos.len();
            "processed initial DUST UTXOs"
        );

        Ok(())
    }

    async fn process_generation_updates(
        &self,
        generation_updates: &[(&DustEvent, &DustGenerationInfo, u64)],
    ) -> Result<(), DustEventError> {
        for (_event, generation, generation_index) in generation_updates {
            // Update dtime to mark when backing Night was spent.
            self.storage
                .update_dust_generation_dtime(*generation_index, generation.dtime)
                .await?;
        }

        info!(
            update_count = generation_updates.len();
            "processed DUST generation updates"
        );

        Ok(())
    }

    async fn process_dust_spends(
        &self,
        dust_spends: &[DustSpendTuple<'_>],
    ) -> Result<(), DustEventError> {
        for (
            transaction_id,
            _event,
            commitment,
            _commitment_index,
            nullifier,
            v_fee,
            _time,
            _params,
        ) in dust_spends
        {
            // Mark DUST UTXO as spent.
            self.storage
                .mark_dust_utxo_spent(&commitment.0, &nullifier.0, *transaction_id as i64)
                .await?;

            debug!(
                v_fee,
                commitment:% = const_hex::encode(commitment.0),
                nullifier:% = const_hex::encode(nullifier.0);
                "processed DUST spend"
            );
        }

        info!(
            spend_count = dust_spends.len();
            "processed DUST spends"
        );

        Ok(())
    }
}

/// Process registration changes for cNIGHT to DUST address mappings.
///
/// Assumes system transactions contain validated registration data.
pub async fn process_registrations<S: Storage>(
    storage: &S,
    registrations: Vec<DustRegistration>,
) -> Result<(), DustEventError> {
    if registrations.is_empty() {
        return Ok(());
    }

    // Validate "one mapping only" rule.
    let mut active_mappings: HashMap<ByteVec, ByteArray<32>> = HashMap::new();
    let mut validated_registrations = vec![];

    for registration in registrations {
        let cardano_addr = registration.cardano_address.clone();

        // Check if this Cardano address already has an active mapping.
        if let Some(existing_dust_addr) = active_mappings.get(&cardano_addr) {
            if registration.is_valid && existing_dust_addr != &registration.dust_address {
                // Invalidate the existing mapping.
                warn!(
                    cardano_address:% = const_hex::encode(&cardano_addr),
                    old_dust_address:% = const_hex::encode(existing_dust_addr.0),
                    new_dust_address:% = const_hex::encode(registration.dust_address.0);
                    "replacing existing DUST registration"
                );
            }
        }

        if registration.is_valid {
            active_mappings.insert(cardano_addr, registration.dust_address);
        } else {
            active_mappings.remove(&cardano_addr);
        }

        validated_registrations.push(registration);
    }

    // Save validated registrations.
    storage
        .save_cnight_registrations(&validated_registrations)
        .await?;

    info!(
        registration_count = validated_registrations.len(),
        active_mappings = active_mappings.len();
        "processed cNIGHT registrations"
    );

    Ok(())
}
