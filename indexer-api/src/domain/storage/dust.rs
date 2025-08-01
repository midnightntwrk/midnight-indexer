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
        DustNullifierTransactionEvent, DustSystemState, RegistrationAddress,
        RegistrationUpdateEvent,
    },
    storage::{BlockStorage, NoopStorage},
};
use futures::{Stream, stream};
use indexer_common::domain::{CardanoStakeKey, DustAddress, DustMerkleRoot, DustPrefix};
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
    async fn get_dust_nullifier_transactions(
        &self,
        prefixes: &[DustPrefix],
        min_prefix_length: u32,
        from_block: u32,
        batch_size: NonZeroU32,
    ) -> Result<
        impl Stream<Item = Result<DustNullifierTransactionEvent, sqlx::Error>> + Send,
        sqlx::Error,
    >;

    /// Stream DUST commitments filtered by prefix.
    async fn get_dust_commitments(
        &self,
        commitment_prefixes: &[DustPrefix],
        start_index: u64,
        min_prefix_length: u32,
        batch_size: NonZeroU32,
    ) -> Result<impl Stream<Item = Result<DustCommitmentEvent, sqlx::Error>> + Send, sqlx::Error>;

    /// Stream registration updates for multiple addresses.
    async fn get_registration_updates(
        &self,
        addresses: &[RegistrationAddress],
        batch_size: NonZeroU32,
    ) -> Result<impl Stream<Item = Result<RegistrationUpdateEvent, sqlx::Error>> + Send, sqlx::Error>;

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
}

#[allow(unused_variables)]
impl DustStorage for NoopStorage {
    async fn get_current_dust_state(&self) -> Result<DustSystemState, sqlx::Error> {
        unimplemented!("NoopStorage")
    }

    async fn get_dust_generation_status(
        &self,
        cardano_stake_keys: &[CardanoStakeKey],
    ) -> Result<Vec<DustGenerationStatus>, sqlx::Error> {
        unimplemented!("NoopStorage")
    }

    async fn get_dust_merkle_root(
        &self,
        tree_type: DustMerkleTreeType,
        timestamp: u64,
    ) -> Result<Option<DustMerkleRoot>, sqlx::Error> {
        unimplemented!("NoopStorage")
    }

    fn get_dust_generations(
        &self,
        dust_address: &DustAddress,
        from_generation_index: u64,
        from_merkle_index: u64,
        only_active: bool,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<DustGenerationEvent, sqlx::Error>> + Send {
        stream::empty()
    }

    async fn get_dust_nullifier_transactions(
        &self,
        prefixes: &[DustPrefix],
        min_prefix_length: u32,
        from_block: u32,
        batch_size: NonZeroU32,
    ) -> Result<
        impl Stream<Item = Result<DustNullifierTransactionEvent, sqlx::Error>> + Send,
        sqlx::Error,
    > {
        Ok(stream::empty())
    }

    async fn get_dust_commitments(
        &self,
        commitment_prefixes: &[DustPrefix],
        start_index: u64,
        min_prefix_length: u32,
        batch_size: NonZeroU32,
    ) -> Result<impl Stream<Item = Result<DustCommitmentEvent, sqlx::Error>> + Send, sqlx::Error>
    {
        Ok(stream::empty())
    }

    async fn get_registration_updates(
        &self,
        addresses: &[RegistrationAddress],
        batch_size: NonZeroU32,
    ) -> Result<impl Stream<Item = Result<RegistrationUpdateEvent, sqlx::Error>> + Send, sqlx::Error>
    {
        Ok(stream::empty())
    }

    async fn get_highest_generation_index_for_dust_address(
        &self,
        dust_address: &DustAddress,
    ) -> Result<Option<u64>, sqlx::Error> {
        unimplemented!("NoopStorage")
    }

    async fn get_active_generation_count_for_dust_address(
        &self,
        dust_address: &DustAddress,
    ) -> Result<u32, sqlx::Error> {
        unimplemented!("NoopStorage")
    }
}
