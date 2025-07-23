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

use crate::domain::storage::{BlockStorage, NoopStorage};
use futures::{Stream, stream};
use std::num::NonZeroU32;

/// DUST storage abstraction.
#[trait_variant::make(Send)]
pub trait DustStorage: BlockStorage {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Get current DUST system state.
    async fn get_current_dust_state(&self) -> Result<crate::domain::DustSystemState, Self::Error>;

    /// Get DUST generation status for specific stake keys.
    async fn get_dust_generation_status(
        &self,
        cardano_stake_keys: &[String],
    ) -> Result<Vec<crate::domain::DustGenerationStatus>, Self::Error>;

    /// Get historical Merkle tree root for a specific timestamp.
    async fn get_dust_merkle_root(
        &self,
        tree_type: crate::domain::DustMerkleTreeType,
        timestamp: i32,
    ) -> Result<Option<Vec<u8>>, Self::Error>;

    /// Stream DUST generations for a specific address.
    async fn get_dust_generations(
        &self,
        dust_address: &str,
        from_generation_index: i64,
        from_merkle_index: i64,
        only_active: bool,
        batch_size: NonZeroU32,
    ) -> Result<
        impl Stream<Item = Result<crate::domain::DustGenerationEvent, Self::Error>> + Send,
        Self::Error,
    >;

    /// Stream transactions containing DUST nullifiers.
    async fn get_dust_nullifier_transactions(
        &self,
        prefixes: &[String],
        min_prefix_length: i32,
        from_block: i32,
        batch_size: NonZeroU32,
    ) -> Result<
        impl Stream<Item = Result<crate::domain::DustNullifierTransactionEvent, Self::Error>> + Send,
        Self::Error,
    >;

    /// Stream DUST commitments filtered by prefix.
    async fn get_dust_commitments(
        &self,
        commitment_prefixes: &[String],
        start_index: i32,
        min_prefix_length: i32,
        batch_size: NonZeroU32,
    ) -> Result<
        impl Stream<Item = Result<crate::domain::DustCommitmentEvent, Self::Error>> + Send,
        Self::Error,
    >;

    /// Stream registration updates for multiple addresses.
    async fn get_registration_updates(
        &self,
        addresses: &[crate::domain::RegistrationAddress],
        batch_size: NonZeroU32,
    ) -> Result<
        impl Stream<Item = Result<crate::domain::RegistrationUpdateEvent, Self::Error>> + Send,
        Self::Error,
    >;
}

impl DustStorage for NoopStorage {
    type Error = std::io::Error;

    async fn get_current_dust_state(&self) -> Result<crate::domain::DustSystemState, Self::Error> {
        unimplemented!("NoopStorage")
    }

    async fn get_dust_generation_status(
        &self,
        _cardano_stake_keys: &[String],
    ) -> Result<Vec<crate::domain::DustGenerationStatus>, Self::Error> {
        unimplemented!("NoopStorage")
    }

    async fn get_dust_merkle_root(
        &self,
        _tree_type: crate::domain::DustMerkleTreeType,
        _timestamp: i32,
    ) -> Result<Option<Vec<u8>>, Self::Error> {
        unimplemented!("NoopStorage")
    }

    async fn get_dust_generations(
        &self,
        _dust_address: &str,
        _from_generation_index: i64,
        _from_merkle_index: i64,
        _only_active: bool,
        _batch_size: NonZeroU32,
    ) -> Result<
        impl Stream<Item = Result<crate::domain::DustGenerationEvent, Self::Error>> + Send,
        Self::Error,
    > {
        Ok(stream::empty())
    }

    async fn get_dust_nullifier_transactions(
        &self,
        _prefixes: &[String],
        _min_prefix_length: i32,
        _from_block: i32,
        _batch_size: NonZeroU32,
    ) -> Result<
        impl Stream<Item = Result<crate::domain::DustNullifierTransactionEvent, Self::Error>> + Send,
        Self::Error,
    > {
        Ok(stream::empty())
    }

    async fn get_dust_commitments(
        &self,
        _commitment_prefixes: &[String],
        _start_index: i32,
        _min_prefix_length: i32,
        _batch_size: NonZeroU32,
    ) -> Result<
        impl Stream<Item = Result<crate::domain::DustCommitmentEvent, Self::Error>> + Send,
        Self::Error,
    > {
        Ok(stream::empty())
    }

    async fn get_registration_updates(
        &self,
        _addresses: &[crate::domain::RegistrationAddress],
        _batch_size: NonZeroU32,
    ) -> Result<
        impl Stream<Item = Result<crate::domain::RegistrationUpdateEvent, Self::Error>> + Send,
        Self::Error,
    > {
        Ok(stream::empty())
    }
}
