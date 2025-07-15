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
    DustCommitmentEvent, DustGenerationEvent, DustGenerationStatus, DustNullifierTransactionEvent,
    DustSystemState, RegistrationUpdateEvent,
    storage::{BlockStorage, TransactionStorage},
};
use futures::Stream;
use indexer_common::domain::{AddressType, DustMerkleTreeType};
use std::{fmt::Debug, num::NonZeroU32};

/// Storage abstraction for DUST-related queries.
#[trait_variant::make(Send)]
pub trait DustStorage
where
    Self: BlockStorage + TransactionStorage + Debug + Clone + Send + Sync + 'static,
{
    /// Get current DUST system state.
    async fn get_current_dust_state(&self) -> Result<DustSystemState, sqlx::Error>;

    /// Get DUST generation status for multiple stake keys.
    async fn get_dust_generation_status_batch(
        &self,
        cardano_stake_keys: &[String],
    ) -> Result<Vec<DustGenerationStatus>, sqlx::Error>;

    /// Get historical Merkle tree root for a specific timestamp.
    async fn get_dust_merkle_root_at_timestamp(
        &self,
        tree_type: DustMerkleTreeType,
        timestamp: i64,
    ) -> Result<Option<Vec<u8>>, sqlx::Error>;

    /// Get DUST generation events with merkle updates.
    fn get_dust_generations(
        &self,
        dust_address: &str,
        from_generation_index: i64,
        from_merkle_index: i64,
        only_active: bool,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<DustGenerationEvent, sqlx::Error>>;

    /// Get transactions containing DUST nullifiers.
    fn get_dust_nullifier_transactions(
        &self,
        prefixes: &[String],
        min_prefix_length: usize,
        from_block: i64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<DustNullifierTransactionEvent, sqlx::Error>>;

    /// Get DUST commitments with merkle updates.
    fn get_dust_commitments(
        &self,
        commitment_prefixes: &[String],
        min_prefix_length: usize,
        start_index: i64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<DustCommitmentEvent, sqlx::Error>>;

    /// Get registration updates for multiple address types.
    fn get_registration_updates(
        &self,
        addresses: &[(AddressType, String)],
        from_timestamp: i64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<RegistrationUpdateEvent, sqlx::Error>>;
}
