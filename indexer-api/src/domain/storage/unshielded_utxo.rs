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
    UnshieldedUtxo,
    storage::{
        NoopStorage, block::BlockStorage, contract_action::ContractActionStorage,
        transaction::TransactionStorage, wallet::WalletStorage,
    },
};
use indexer_common::domain::{BlockHash, Identifier, TransactionHash, UnshieldedAddress};
use sqlx::Error;
use std::fmt::Debug;

/// Storage abstraction for unshielded UTXO operations.
#[trait_variant::make(Send)]
pub trait UnshieldedUtxoStorage
where
    Self: BlockStorage
        + ContractActionStorage
        + TransactionStorage
        + WalletStorage
        + Debug
        + Clone
        + Send
        + Sync
        + 'static,
{
    /// Get all unshielded UTXOs for a given address.
    async fn get_unshielded_utxos_by_address(
        &self,
        address: &UnshieldedAddress,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error>;

    /// Get unshielded UTXOs created by a specific transaction.
    async fn get_unshielded_utxos_created_by_transaction(
        &self,
        transaction_id: u64,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error>;

    /// Get unshielded UTXOs spent by a specific transaction.
    async fn get_unshielded_utxos_spent_by_transaction(
        &self,
        transaction_id: u64,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error>;

    /// Get unshielded UTXOs created in a specific transaction for a specific address.
    async fn get_unshielded_utxos_created_in_transaction_for_address(
        &self,
        address: &UnshieldedAddress,
        transaction_id: u64,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error>;

    /// Get unshielded UTXOs spent in a specific transaction for a specific address.
    async fn get_unshielded_utxos_spent_in_transaction_for_address(
        &self,
        address: &UnshieldedAddress,
        transaction_id: u64,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error>;

    /// Get unshielded UTXOs for an address from blocks with height >= the given value.
    async fn get_unshielded_utxos_by_address_from_height(
        &self,
        address: &UnshieldedAddress,
        height: u32,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error>;

    /// Get unshielded UTXOs for an address created in the specific block identified by the given
    /// hash.
    async fn get_unshielded_utxos_by_address_from_block_hash(
        &self,
        address: &UnshieldedAddress,
        block_hash: &BlockHash,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error>;

    /// Get unshielded UTXOs for an address from a transaction with the given hash.
    async fn get_unshielded_utxos_by_address_from_transaction_hash(
        &self,
        address: &UnshieldedAddress,
        transaction_hash: &TransactionHash,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error>;

    /// Get unshielded UTXOs for an address from a transaction with the given identifier.
    async fn get_unshielded_utxos_by_address_from_transaction_identifier(
        &self,
        address: &UnshieldedAddress,
        identifier: &Identifier,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error>;

    /// Get the highest end indices for unshielded address progress calculation.
    /// Returns a tuple of:
    /// - the highest end index from all transactions
    /// - the highest end index for transactions involving the given unshielded address
    async fn get_highest_indices_for_address(
        &self,
        address: &UnshieldedAddress,
    ) -> Result<(Option<u64>, Option<u64>), sqlx::Error>;
}

#[allow(unused_variables)]
impl UnshieldedUtxoStorage for NoopStorage {
    #[cfg_attr(coverage, coverage(off))]
    async fn get_unshielded_utxos_by_address(
        &self,
        address: &UnshieldedAddress,
    ) -> Result<Vec<UnshieldedUtxo>, Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_unshielded_utxos_created_by_transaction(
        &self,
        transaction_id: u64,
    ) -> Result<Vec<UnshieldedUtxo>, Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_unshielded_utxos_spent_by_transaction(
        &self,
        transaction_id: u64,
    ) -> Result<Vec<UnshieldedUtxo>, Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_unshielded_utxos_created_in_transaction_for_address(
        &self,
        address: &UnshieldedAddress,
        transaction_id: u64,
    ) -> Result<Vec<UnshieldedUtxo>, Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_unshielded_utxos_spent_in_transaction_for_address(
        &self,
        address: &UnshieldedAddress,
        transaction_id: u64,
    ) -> Result<Vec<UnshieldedUtxo>, Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_unshielded_utxos_by_address_from_height(
        &self,
        address: &UnshieldedAddress,
        height: u32,
    ) -> Result<Vec<UnshieldedUtxo>, Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_unshielded_utxos_by_address_from_block_hash(
        &self,
        address: &UnshieldedAddress,
        block_hash: &BlockHash,
    ) -> Result<Vec<UnshieldedUtxo>, Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_unshielded_utxos_by_address_from_transaction_hash(
        &self,
        address: &UnshieldedAddress,
        transaction_hash: &TransactionHash,
    ) -> Result<Vec<UnshieldedUtxo>, Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_unshielded_utxos_by_address_from_transaction_identifier(
        &self,
        address: &UnshieldedAddress,
        identifier: &Identifier,
    ) -> Result<Vec<UnshieldedUtxo>, Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_highest_indices_for_address(
        &self,
        address: &UnshieldedAddress,
    ) -> Result<(Option<u64>, Option<u64>), Error> {
        unimplemented!()
    }
}
