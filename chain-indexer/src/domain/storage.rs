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
use indexer_common::domain::{
    UnshieldedUtxo,
    dust::{DustEvent, DustGenerationInfo, DustRegistration, DustUtxo},
};

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
    /// Save DUST events from transaction processing
    async fn save_dust_events(
        &self,
        events: &[DustEvent],
        transaction_id: i64,
    ) -> Result<(), sqlx::Error>;

    /// Save DUST UTXOs
    async fn save_dust_utxos(&self, utxos: &[DustUtxo]) -> Result<(), sqlx::Error>;

    /// Save DUST generation information
    async fn save_dust_generation_info(
        &self,
        generation_info: &[&DustGenerationInfo],
    ) -> Result<(), sqlx::Error>;

    /// Save cNIGHT registrations
    async fn save_cnight_registrations(
        &self,
        registrations: &[DustRegistration],
    ) -> Result<(), sqlx::Error>;

    /// Get DUST generation info by owner address
    async fn get_dust_generation_info_by_owner(
        &self,
        owner: &[u8],
    ) -> Result<Vec<DustGenerationInfo>, sqlx::Error>;

    /// Get DUST UTXOs by owner address  
    async fn get_dust_utxos_by_owner(&self, owner: &[u8]) -> Result<Vec<DustUtxo>, sqlx::Error>;

    /// Search for transactions by nullifier prefix (privacy-preserving)
    async fn search_transactions_by_nullifier_prefix(
        &self,
        prefix: &str,
        after_block: Option<u32>,
    ) -> Result<Vec<(i64, Vec<u8>)>, sqlx::Error>; // (transaction_id, nullifier)

    /// Update DUST generation dtime when Night UTXO is spent
    async fn update_dust_generation_dtime(
        &self,
        generation_index: u64,
        dtime: u64,
    ) -> Result<(), sqlx::Error>;

    /// Mark DUST UTXO as spent
    async fn mark_dust_utxo_spent(
        &self,
        commitment: &[u8],
        nullifier: &[u8],
        transaction_id: i64,
    ) -> Result<(), sqlx::Error>;

    /// Update Merkle tree state
    async fn update_merkle_tree_state(
        &self,
        tree_type: crate::domain::MerkleTreeType,
        block_height: u32,
        root: &[u8],
        tree_data: &[u8],
    ) -> Result<(), sqlx::Error>;

    /// Get Merkle tree collapsed update
    async fn get_merkle_tree_collapsed_update(
        &self,
        tree_type: crate::domain::MerkleTreeType,
        start_index: u64,
        end_index: u64,
    ) -> Result<Vec<u8>, sqlx::Error>;
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use indexer_common::domain::ByteArray;

    #[derive(Debug, Clone, Default)]
    pub struct NoopStorage;

    impl Storage for NoopStorage {
        async fn get_highest_block(&self) -> Result<Option<BlockInfo>, sqlx::Error> {
            Ok(None)
        }

        async fn get_transaction_count(&self) -> Result<u64, sqlx::Error> {
            Ok(0)
        }

        async fn get_contract_action_count(&self) -> Result<(u64, u64, u64), sqlx::Error> {
            Ok((0, 0, 0))
        }

        async fn save_block(&self, _block: &mut Block) -> Result<Option<u64>, sqlx::Error> {
            Ok(None)
        }

        async fn save_unshielded_utxos(
            &self,
            _utxos: &[UnshieldedUtxo],
            _transaction_id: &i64,
            _spent: bool,
        ) -> Result<(), sqlx::Error> {
            Ok(())
        }

        async fn get_block_transactions(
            &self,
            _block_height: u32,
        ) -> Result<BlockTransactions, sqlx::Error> {
            Ok(BlockTransactions {
                transactions: vec![],
                protocol_version: indexer_common::domain::ProtocolVersion(0),
                block_parent_hash: ByteArray([0u8; 32]),
                block_timestamp: 0,
            })
        }

        async fn save_dust_events(
            &self,
            _events: &[DustEvent],
            _transaction_id: i64,
        ) -> Result<(), sqlx::Error> {
            Ok(())
        }

        async fn save_dust_utxos(&self, _utxos: &[DustUtxo]) -> Result<(), sqlx::Error> {
            Ok(())
        }

        async fn save_dust_generation_info(
            &self,
            _generation_info: &[&DustGenerationInfo],
        ) -> Result<(), sqlx::Error> {
            Ok(())
        }

        async fn save_cnight_registrations(
            &self,
            _registrations: &[DustRegistration],
        ) -> Result<(), sqlx::Error> {
            Ok(())
        }

        async fn get_dust_generation_info_by_owner(
            &self,
            _owner: &[u8],
        ) -> Result<Vec<DustGenerationInfo>, sqlx::Error> {
            Ok(vec![])
        }

        async fn get_dust_utxos_by_owner(
            &self,
            _owner: &[u8],
        ) -> Result<Vec<DustUtxo>, sqlx::Error> {
            Ok(vec![])
        }

        async fn search_transactions_by_nullifier_prefix(
            &self,
            _prefix: &str,
            _after_block: Option<u32>,
        ) -> Result<Vec<(i64, Vec<u8>)>, sqlx::Error> {
            Ok(vec![])
        }

        async fn update_dust_generation_dtime(
            &self,
            _generation_index: u64,
            _dtime: u64,
        ) -> Result<(), sqlx::Error> {
            Ok(())
        }

        async fn mark_dust_utxo_spent(
            &self,
            _commitment: &[u8],
            _nullifier: &[u8],
            _transaction_id: i64,
        ) -> Result<(), sqlx::Error> {
            Ok(())
        }

        async fn update_merkle_tree_state(
            &self,
            _tree_type: crate::domain::MerkleTreeType,
            _block_height: u32,
            _root: &[u8],
            _tree_data: &[u8],
        ) -> Result<(), sqlx::Error> {
            Ok(())
        }

        async fn get_merkle_tree_collapsed_update(
            &self,
            _tree_type: crate::domain::MerkleTreeType,
            _start_index: u64,
            _end_index: u64,
        ) -> Result<Vec<u8>, sqlx::Error> {
            Ok(vec![])
        }
    }
}
