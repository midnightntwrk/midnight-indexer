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
    dust::{
        DustCommitmentEvent, DustGenerationEvent, DustGenerationStatus, DustMerkleTreeType,
        DustNullifierTransactionEvent, DustSystemState, RegistrationAddress, RegistrationUpdate,
    },
    storage::{BlockStorage, NoopStorage},
};
use futures::{Stream, stream};
use indexer_common::domain::{
    CardanoStakeKey, DustAddress, DustMerkleRoot, DustPrefix, TransactionHash,
    dust::{DustEvent, DustEventVariant},
};
use std::num::NonZeroU32;

/// DUST storage abstraction.
#[trait_variant::make(Send)]
pub trait DustStorage: BlockStorage {
    /// Get current DUST system state.
    async fn get_current_dust_state(&self) -> Result<DustSystemState, sqlx::Error>;

    /// Get DUST generation status for specific stake keys.
    async fn get_dust_generation_status(
        &self,
        cardano_stake_keys: &[CardanoStakeKey],
    ) -> Result<Vec<DustGenerationStatus>, sqlx::Error>;

    /// Get historical Merkle tree root for a specific timestamp.
    async fn get_dust_merkle_root(
        &self,
        tree_type: DustMerkleTreeType,
        timestamp: u64,
    ) -> Result<Option<DustMerkleRoot>, sqlx::Error>;

    /// Stream DUST generations for a specific address.
    fn get_dust_generations(
        &self,
        dust_address: &DustAddress,
        from_generation_index: u64,
        from_merkle_index: u64,
        only_active: bool,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<DustGenerationEvent, sqlx::Error>> + Send;

    /// Stream transactions containing DUST nullifiers.
    fn get_dust_nullifier_transactions(
        &self,
        prefixes: &[DustPrefix],
        min_prefix_length: u32,
        from_block: u32,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<DustNullifierTransactionEvent, sqlx::Error>> + Send;

    /// Stream DUST commitments filtered by prefix.
    fn get_dust_commitments(
        &self,
        commitment_prefixes: &[DustPrefix],
        start_index: u64,
        min_prefix_length: u32,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<DustCommitmentEvent, sqlx::Error>> + Send;

    /// Stream registration updates for multiple addresses.
    fn get_registration_updates(
        &self,
        addresses: &[RegistrationAddress],
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<RegistrationUpdate, sqlx::Error>> + Send;

    /// Get highest generation index for a DUST address.
    async fn get_highest_generation_index_for_dust_address(
        &self,
        dust_address: &DustAddress,
    ) -> Result<Option<u64>, sqlx::Error>;

    /// Get count of active generations for a DUST address.
    async fn get_active_generation_count_for_dust_address(
        &self,
        dust_address: &DustAddress,
    ) -> Result<u32, sqlx::Error>;

    /// Get DUST events by transaction hash.
    async fn get_dust_events_by_transaction(
        &self,
        transaction_hash: TransactionHash,
    ) -> Result<Vec<DustEvent>, sqlx::Error>;

    /// Get recent DUST events with optional filtering.
    async fn get_recent_dust_events(
        &self,
        limit: u32,
        event_variant: Option<DustEventVariant>,
    ) -> Result<Vec<DustEvent>, sqlx::Error>;
}

impl DustStorage for NoopStorage {
    async fn get_current_dust_state(&self) -> Result<DustSystemState, sqlx::Error> {
        unimplemented!("NoopStorage is only for schema export")
    }

    async fn get_dust_generation_status(
        &self,
        _cardano_stake_keys: &[CardanoStakeKey],
    ) -> Result<Vec<DustGenerationStatus>, sqlx::Error> {
        unimplemented!("NoopStorage is only for schema export")
    }

    async fn get_dust_merkle_root(
        &self,
        _tree_type: DustMerkleTreeType,
        _timestamp: u64,
    ) -> Result<Option<DustMerkleRoot>, sqlx::Error> {
        unimplemented!("NoopStorage is only for schema export")
    }

    fn get_dust_generations(
        &self,
        _dust_address: &DustAddress,
        _from_generation_index: u64,
        _from_merkle_index: u64,
        _only_active: bool,
        _batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<DustGenerationEvent, sqlx::Error>> + Send {
        stream::empty()
    }

    fn get_dust_nullifier_transactions(
        &self,
        _prefixes: &[DustPrefix],
        _min_prefix_length: u32,
        _from_block: u32,
        _batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<DustNullifierTransactionEvent, sqlx::Error>> + Send {
        stream::empty()
    }

    fn get_dust_commitments(
        &self,
        _commitment_prefixes: &[DustPrefix],
        _start_index: u64,
        _min_prefix_length: u32,
        _batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<DustCommitmentEvent, sqlx::Error>> + Send {
        stream::empty()
    }

    fn get_registration_updates(
        &self,
        _addresses: &[RegistrationAddress],
        _batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<RegistrationUpdate, sqlx::Error>> + Send {
        stream::empty()
    }

    async fn get_highest_generation_index_for_dust_address(
        &self,
        _dust_address: &DustAddress,
    ) -> Result<Option<u64>, sqlx::Error> {
        unimplemented!("NoopStorage is only for schema export")
    }

    async fn get_active_generation_count_for_dust_address(
        &self,
        _dust_address: &DustAddress,
    ) -> Result<u32, sqlx::Error> {
        unimplemented!("NoopStorage is only for schema export")
    }

    async fn get_dust_events_by_transaction(
        &self,
        _transaction_hash: TransactionHash,
    ) -> Result<Vec<DustEvent>, sqlx::Error> {
        unimplemented!("NoopStorage is only for schema export")
    }

    async fn get_recent_dust_events(
        &self,
        _limit: u32,
        _event_variant: Option<DustEventVariant>,
    ) -> Result<Vec<DustEvent>, sqlx::Error> {
        unimplemented!("NoopStorage is only for schema export")
    }
}
