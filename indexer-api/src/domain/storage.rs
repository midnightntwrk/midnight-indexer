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
    Block, BlockHash, ContractAction, Transaction, TransactionHash, UnshieldedUtxo,
};
use futures::{stream, Stream};
use indexer_common::domain::{
    ContractAddress, Identifier, SessionId, UnshieldedAddress, ViewingKey,
};
use sqlx::Error;
use std::num::NonZeroU32;

/// Storage abstraction.
#[trait_variant::make(Send)]
pub trait Storage
where
    Self: Clone + Send + Sync + 'static,
{
    /// Get the latest [Block].
    async fn get_latest_block(&self) -> Result<Option<Block>, sqlx::Error>;

    /// Get a [Block] for the given hash.
    async fn get_block_by_hash(&self, hash: &BlockHash) -> Result<Option<Block>, sqlx::Error>;

    /// Get a [Block] for the given block height.
    async fn get_block_by_height(&self, height: u32) -> Result<Option<Block>, sqlx::Error>;

    /// Get a stream of all [Block]s starting at the given height.
    fn get_blocks(
        &self,
        from_height: u32,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<Block, sqlx::Error>> + Send;

    /// Get [Transaction] for the given tx_db_id
    async fn get_transaction_by_db_id(
        &self,
        tx_db_id: u64,
    ) -> Result<Option<Transaction>, sqlx::Error>;

    /// Get [Transaction]s for the given hash, ordered descendingly by ID. Transaction hashes are
    /// unique for successful transactions, yet not for failed ones.
    async fn get_transactions_by_hash(
        &self,
        hash: &TransactionHash,
    ) -> Result<Vec<Transaction>, sqlx::Error>;

    /// Get a [Transaction] for the given identifier.
    async fn get_transaction_by_identifier(
        &self,
        identifier: &Identifier,
    ) -> Result<Option<Transaction>, sqlx::Error>;

    /// Get the latest [ContractAction] for the given address.
    async fn get_latest_contract_action_by_address(
        &self,
        address: &ContractAddress,
    ) -> Result<Option<ContractAction>, sqlx::Error>;

    /// Get a [ContractAction] for the given address and block hash.
    async fn get_contract_action_by_address_and_block_hash(
        &self,
        address: &ContractAddress,
        hash: &BlockHash,
    ) -> Result<Option<ContractAction>, sqlx::Error>;

    /// Get a [ContractAction] for the given address and block height.
    async fn get_contract_action_by_address_and_block_height(
        &self,
        address: &ContractAddress,
        height: u32,
    ) -> Result<Option<ContractAction>, sqlx::Error>;

    /// Get a [ContractAction] for the given address and transaction hash. Only the unique
    /// successful transaction identified by the given hash is considered.
    async fn get_contract_action_by_address_and_transaction_hash(
        &self,
        address: &ContractAddress,
        hash: &TransactionHash,
    ) -> Result<Option<ContractAction>, sqlx::Error>;

    /// Get a [ContractAction] for the given address and transaction identifier.
    async fn get_contract_action_by_address_and_transaction_identifier(
        &self,
        address: &ContractAddress,
        identifier: &Identifier,
    ) -> Result<Option<ContractAction>, sqlx::Error>;

    /// Get a stream of [ContractAction]s for the given address starting at the given block height
    /// and contract_action ID.
    fn get_contract_actions_by_address(
        &self,
        address: &ContractAddress,
        from_block_height: u32,
        from_contract_action_id: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<ContractAction, sqlx::Error>> + Send;

    /// Get the last processed transaction end index for the given [SessionId].
    async fn get_last_end_index_for_wallet(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<u64>, sqlx::Error>;

    /// Get the last relevant transaction end index for the given [SessionId].
    async fn get_last_relevant_end_index_for_wallet(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<u64>, sqlx::Error>;

    /// Get a stream of all [Transaction]s relevant for a wallet, starting at the given index.
    fn get_relevant_transactions(
        &self,
        session_id: &SessionId,
        from_index: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<Transaction, sqlx::Error>> + Send;

    /// Connect a wallet, i.e. add it to the active ones.
    async fn connect_wallet(&self, viewing_key: &ViewingKey) -> Result<(), sqlx::Error>;

    /// Get all unshielded UTXOs owned by the given address (from the API domain module).
    async fn get_unshielded_utxos_by_address(
        &self,
        address: &UnshieldedAddress,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error>;

    /// Gets all UTXOs that were created in a specific transaction and owned by a specific address.
    ///
    /// # Parameters
    /// * `tx_id` - The database ID of the transaction (not hash or identifier)
    /// * `address` - The unshielded address to filter UTXOs by
    ///
    /// # Returns
    /// A vector of UTXOs created in the specified transaction for the address.
    /// Returns an empty vector if no matching UTXOs are found.
    async fn get_unshielded_utxos_by_address_created_in_tx(
        &self,
        transaction_id: u64,
        address: &UnshieldedAddress,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error>;

    async fn get_unshielded_utxos_by_address_spent_in_tx(
        &self,
        transaction_id: u64,
        address: &UnshieldedAddress,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error>;

    /// All UTXOs for `address` whose creating-block height is **>= start_height**.
    async fn get_unshielded_utxos_by_address_from_height(
        &self,
        address: &UnshieldedAddress,
        start_height: u32,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error>;

    /// All UTXOs for `address` whose creating-block hash matches `block_hash`.
    async fn get_unshielded_utxos_by_address_from_block_hash(
        &self,
        address: &UnshieldedAddress,
        block_hash: &BlockHash,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error>;

    /// All UTXOs for `address` created in the transaction whose **hash** is `tx_hash`.
    async fn get_unshielded_utxos_by_address_from_tx_hash(
        &self,
        address: &UnshieldedAddress,
        tx_hash: &TransactionHash,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error>;

    /// All UTXOs for `address` created in the transaction identified by `identifier`.
    async fn get_unshielded_utxos_by_address_from_tx_identifier(
        &self,
        address: &UnshieldedAddress,
        identifier: &Identifier,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error>;

    /// Get all transactions that create or spend unshielded UTXOs for the given address.
    ///
    /// # Arguments
    /// * `address` - The unshielded address to filter by
    ///
    /// # Returns
    /// A vector of transactions where either:
    /// - The transaction created UTXOs owned by this address
    /// - The transaction spent UTXOs owned by this address
    ///
    /// Transactions are returned in descending order by database ID.
    async fn get_transactions_involving_unshielded(
        &self,
        address: &UnshieldedAddress,
    ) -> Result<Vec<Transaction>, sqlx::Error>;

    /// Disconnect a wallet, i.e. remove it from the active ones.
    async fn disconnect_wallet(&self, session_id: &SessionId) -> Result<(), sqlx::Error>;

    /// Set the wallet active at the current timestamp to avoid timing out.
    async fn set_wallet_active(&self, session_id: &SessionId) -> Result<(), sqlx::Error>;
}

/// Just needed as a type argument for `infra::api::export_schema` which should not depend on any
/// features like "cloud" and hence types like `infra::postgres::PostgresStorage` cannot be used.
/// Once traits with async functions are object safe, this can go away and be replaced with
/// `Box<dyn Storage>` at the type level.
#[derive(Debug, Clone, Default)]
pub struct NoopStorage;

impl Storage for NoopStorage {
    #[cfg_attr(coverage, coverage(off))]
    async fn get_latest_block(&self) -> Result<Option<Block>, sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_block_by_hash(&self, _hash: &BlockHash) -> Result<Option<Block>, sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_block_by_height(&self, _height: u32) -> Result<Option<Block>, sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    fn get_blocks(
        &self,
        _from_height: u32,
        _batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<Block, sqlx::Error>> {
        stream::empty()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_transactions_by_hash(
        &self,
        _hash: &TransactionHash,
    ) -> Result<Vec<Transaction>, sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_transaction_by_identifier(
        &self,
        _identifier: &Identifier,
    ) -> Result<Option<Transaction>, sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_latest_contract_action_by_address(
        &self,
        _address: &ContractAddress,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_contract_action_by_address_and_block_hash(
        &self,
        _address: &ContractAddress,
        _hash: &BlockHash,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_contract_action_by_address_and_block_height(
        &self,
        _address: &ContractAddress,
        _height: u32,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_contract_action_by_address_and_transaction_hash(
        &self,
        _address: &ContractAddress,
        _hash: &TransactionHash,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_contract_action_by_address_and_transaction_identifier(
        &self,
        _address: &ContractAddress,
        _identifier: &Identifier,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        unimplemented!()
    }

    fn get_contract_actions_by_address(
        &self,
        _address: &ContractAddress,
        _from_block_height: u32,
        _from_contract_id: u64,
        _batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<ContractAction, sqlx::Error>> + Send {
        stream::empty()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_last_end_index_for_wallet(
        &self,
        _session_id: &SessionId,
    ) -> Result<Option<u64>, sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_last_relevant_end_index_for_wallet(
        &self,
        _session_id: &SessionId,
    ) -> Result<Option<u64>, sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    fn get_relevant_transactions(
        &self,
        _session_id: &SessionId,
        _from_index: u64,
        _batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<Transaction, sqlx::Error>> + Send {
        stream::empty()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn connect_wallet(&self, _viewing_key: &ViewingKey) -> Result<(), sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn disconnect_wallet(&self, _session_id: &SessionId) -> Result<(), sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn set_wallet_active(&self, _session_id: &SessionId) -> Result<(), sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_unshielded_utxos_by_address(
        &self,
        _address: &UnshieldedAddress,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        unimplemented!()
    }

    async fn get_transactions_involving_unshielded(
        &self,
        _address: &UnshieldedAddress,
    ) -> Result<Vec<Transaction>, sqlx::Error> {
        unimplemented!()
    }

    async fn get_unshielded_utxos_by_address_from_height(
        &self,
        _address: &UnshieldedAddress,
        _start_height: u32,
    ) -> Result<Vec<UnshieldedUtxo>, Error> {
        unimplemented!()
    }

    async fn get_unshielded_utxos_by_address_from_block_hash(
        &self,
        _address: &UnshieldedAddress,
        _block_hash: &BlockHash,
    ) -> Result<Vec<UnshieldedUtxo>, Error> {
        unimplemented!()
    }

    async fn get_unshielded_utxos_by_address_from_tx_hash(
        &self,
        _address: &UnshieldedAddress,
        _tx_hash: &TransactionHash,
    ) -> Result<Vec<UnshieldedUtxo>, Error> {
        unimplemented!()
    }

    async fn get_unshielded_utxos_by_address_from_tx_identifier(
        &self,
        _address: &UnshieldedAddress,
        _identifier: &Identifier,
    ) -> Result<Vec<UnshieldedUtxo>, Error> {
        unimplemented!()
    }

    async fn get_transaction_by_db_id(&self, _tx_db_id: u64) -> Result<Option<Transaction>, Error> {
        unimplemented!()
    }

    async fn get_unshielded_utxos_by_address_created_in_tx(
        &self,
        _transaction_id: u64,
        _address: &UnshieldedAddress,
    ) -> Result<Vec<UnshieldedUtxo>, Error> {
        unimplemented!()
    }

    async fn get_unshielded_utxos_by_address_spent_in_tx(
        &self,
        _transaction_id: u64,
        _address: &UnshieldedAddress,
    ) -> Result<Vec<UnshieldedUtxo>, Error> {
        unimplemented!()
    }
}
