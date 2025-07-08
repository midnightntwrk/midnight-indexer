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

use crate::domain::{Block, BlockInfo, BlockTransactions};
use futures::Stream;
use indexer_common::domain::{
    DustCommitment, DustNullifier, DustOwner, RawTransaction, UnshieldedUtxo,
    dust::{DustEvent, DustGenerationInfo, DustRegistration, DustUtxo},
};
use std::num::NonZeroU32;

/// Storage abstraction.
#[trait_variant::make(Send)]
pub trait Storage
where
    Self: Clone + Send + Sync + 'static,
{
    /// Get the hash and height of the highest stored block.
    async fn get_highest_block(&self) -> Result<Option<BlockInfo>, sqlx::Error>;

    /// Get the number of stored transactions.
    async fn get_transaction_count(&self) -> Result<u64, sqlx::Error>;

    /// Get the number of stored contract actions: deploys, calls, updates.
    async fn get_contract_action_count(&self) -> Result<(u64, u64, u64), sqlx::Error>;

    /// Save the given block, update transaction IDs, and return the max transaction ID.
    async fn save_block(&self, block: &mut Block) -> Result<Option<u64>, sqlx::Error>;

    /// Save the given unshielded UTXOs.
    async fn save_unshielded_utxos(
        &self,
        utxos: &[UnshieldedUtxo],
        transaction_id: &i64,
        spent: bool,
    ) -> Result<(), sqlx::Error>;

    /// Get all transactions with additional block data for the given block height.
    async fn get_block_transactions(
        &self,
        block_height: u32,
    ) -> Result<BlockTransactions, sqlx::Error>;

    // DUST-specific storage methods
    /// Save DUST events from transaction processing.
    async fn save_dust_events(
        &self,
        events: impl AsRef<[DustEvent]> + Send,
        transaction_id: u64,
    ) -> Result<(), sqlx::Error>;

    /// Save DUST UTXOs.
    async fn save_dust_utxos(
        &self,
        utxos: impl AsRef<[DustUtxo]> + Send,
    ) -> Result<(), sqlx::Error>;

    /// Save DUST generation information.
    async fn save_dust_generation_info(
        &self,
        generation_info: impl AsRef<[DustGenerationInfo]> + Send,
    ) -> Result<(), sqlx::Error>;

    /// Save cNIGHT registrations.
    async fn save_cnight_registrations(
        &self,
        registrations: impl AsRef<[DustRegistration]> + Send,
    ) -> Result<(), sqlx::Error>;

    /// Get DUST generation info by owner address.
    fn get_dust_generation_info_by_owner(
        &self,
        owner: DustOwner,
        generation_info_id: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<DustGenerationInfo, sqlx::Error>> + Send;

    /// Get DUST UTXOs by owner address.
    fn get_dust_utxos_by_owner(
        &self,
        owner: DustOwner,
        utxo_id: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<DustUtxo, sqlx::Error>> + Send;

    /// Search for transactions by nullifier prefix (privacy-preserving) and return pairs of
    /// transaction ID and raw transaction contents.
    fn search_transactions_by_nullifier_prefix(
        &self,
        prefix: &str,
        after_block: Option<u32>,
        transaction_id: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<(u64, RawTransaction), sqlx::Error>> + Send;

    /// Update DUST generation dtime when Night UTXO is spent.
    async fn update_dust_generation_dtime(
        &self,
        generation_index: u64,
        dtime: u64,
    ) -> Result<(), sqlx::Error>;

    /// Mark DUST UTXO as spent.
    async fn mark_dust_utxo_spent(
        &self,
        commitment: DustCommitment,
        nullifier: DustNullifier,
        transaction_id: u64,
    ) -> Result<(), sqlx::Error>;
}
