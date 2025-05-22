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

use crate::domain::{Block, BlockHash, ContractAction, Transaction, UnshieldedUtxo,};
use futures::{Stream, stream};
use indexer_common::domain::{ContractAddress, Identifier, SessionId, TransactionHash, UnshieldedAddress, ViewingKey};
use std::{fmt::Debug, num::NonZeroU32};

/// Filter criteria for unshielded UTXOs queries
#[derive(Debug, Clone)]
pub enum UnshieldedUtxoFilter<'a> {
    /// All UTXOs (no filter)
    All,
    /// UTXOs created by a specific transaction
    CreatedByTx(u64),
    /// UTXOs spent by a specific transaction
    SpentByTx(u64),
    /// UTXOs created in a specific transaction for a specific address
    CreatedInTxForAddress(u64),
    /// UTXOs spent in a specific transaction for a specific address
    SpentInTxForAddress(u64),
    /// UTXOs created/spent in blocks with height >= the given value
    FromHeight(u32),
    /// UTXOs created/spent in a specific block
    FromBlockHash(&'a BlockHash),
    /// UTXOs created/spent in a transaction with given hash
    FromTxHash(&'a TransactionHash),
    /// UTXOs created/spent in a transaction with given identifier
    FromTxIdentifier(&'a Identifier),
}

/// Storage abstraction.
#[trait_variant::make(Send)]
pub trait Storage
where
    Self: Debug + Clone + Send + Sync + 'static,
{
    /// Get the latest [Block].
    async fn get_latest_block(&self) -> Result<Option<Block>, sqlx::Error>;

    /// Get a [Block] for the given hash.
    async fn get_block_by_hash(&self, hash: BlockHash) -> Result<Option<Block>, sqlx::Error>;

    /// Get a [Block] for the given block height.
    async fn get_block_by_height(&self, height: u32) -> Result<Option<Block>, sqlx::Error>;

    /// Get a stream of all [Block]s starting at the given height.
    fn get_blocks(
        &self,
        height: u32,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<Block, sqlx::Error>> + Send;

    /// Get a [Transaction] for the given ID.
    async fn get_transaction_by_id(&self, id: u64) -> Result<Transaction, sqlx::Error>;

    /// Get the [Transaction]s for the block with the given ID.
    async fn get_transactions_by_block_id(&self, id: u64) -> Result<Vec<Transaction>, sqlx::Error>;

    /// Get [Transaction]s for the given hash, ordered descendingly by ID. Transaction hashes are
    /// unique for successful transactions, yet not for failed ones.
    async fn get_transactions_by_hash(
        &self,
        hash: TransactionHash,
    ) -> Result<Vec<Transaction>, sqlx::Error>;

    /// Get [Transaction]s for the given identifier. Identifiers are not unique.
    async fn get_transactions_by_identifier(
        &self,
        identifier: &Identifier,
    ) -> Result<Vec<Transaction>, sqlx::Error>;

    /// Get the contract deploy for the given address.
    async fn get_contract_deploy_by_address(
        &self,
        address: &ContractAddress,
    ) -> Result<Option<ContractAction>, sqlx::Error>;

    /// Get the latest [ContractAction] for the given address.
    async fn get_contract_action_by_address(
        &self,
        address: &ContractAddress,
    ) -> Result<Option<ContractAction>, sqlx::Error>;

    /// Get a [ContractAction] for the given address and block hash.
    async fn get_contract_action_by_address_and_block_hash(
        &self,
        address: &ContractAddress,
        hash: BlockHash,
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
        hash: TransactionHash,
    ) -> Result<Option<ContractAction>, sqlx::Error>;

    /// Get a [ContractAction] for the given address and transaction identifier.
    async fn get_contract_action_by_address_and_transaction_identifier(
        &self,
        address: &ContractAddress,
        identifier: &Identifier,
    ) -> Result<Option<ContractAction>, sqlx::Error>;

    /// Get the contract actions for the transaction with the given id.
    async fn get_contract_actions_by_transaction_id(
        &self,
        id: u64,
    ) -> Result<Vec<ContractAction>, sqlx::Error>;

    /// Get a stream of [ContractAction]s for the given address starting at the given block height
    /// and contract_action ID.
    fn get_contract_actions_by_address(
        &self,
        address: &ContractAddress,
        height: u32,
        contract_action_id: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<ContractAction, sqlx::Error>> + Send;

    /// Get a tuple of end indices:
    /// - the highest end index into the zswap state of all currently known transactions,
    /// - the highest end index into the zswap state of all currently known relevant
    ///   transactions,i.e. such that belong to any wallet,
    /// - the highest end index into the zswap state of all currently known relevant transactions
    ///   for a particular wallet identified by the given [SessionId].
    async fn get_highest_indices(
        &self,
        session_id: SessionId,
    ) -> Result<(Option<u64>, Option<u64>, Option<u64>), sqlx::Error>;

    /// Get a stream of all [Transaction]s relevant for a wallet, starting at the given index.
    fn get_relevant_transactions(
        &self,
        session_id: SessionId,
        index: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<Transaction, sqlx::Error>> + Send;

    /// Connect a wallet, i.e. add it to the active ones.
    async fn connect_wallet(&self, viewing_key: &ViewingKey) -> Result<(), sqlx::Error>;

    /// Disconnect a wallet, i.e. remove it from the active ones.
    async fn disconnect_wallet(&self, session_id: SessionId) -> Result<(), sqlx::Error>;

    /// Set the wallet active at the current timestamp to avoid timing out.
    async fn set_wallet_active(&self, session_id: SessionId) -> Result<(), sqlx::Error>;

    /// Get unshielded UTXOs based on filter criteria.
    /// This consolidated method replaces multiple specific UTXO query methods.
    async fn get_unshielded_utxos(
        &self,
        address: Option<&UnshieldedAddress>,
        filter: UnshieldedUtxoFilter<'_>,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error>;

    /// Get all transactions that create or spend unshielded UTXOs for the given address.
    async fn get_transactions_involving_unshielded(
        &self,
        address: &UnshieldedAddress,
    ) -> Result<Vec<Transaction>, sqlx::Error>;
}

/// Just needed as a type argument for `infra::api::export_schema` which should not depend on any
/// features like "cloud" and hence types like `infra::postgres::PostgresStorage` cannot be used.
/// Once traits with async functions are object safe, this can go away and be replaced with
/// `Box<dyn Storage>` at the type level.
#[derive(Debug, Clone, Default)]
pub struct NoopStorage;

#[allow(unused_variables)]
impl Storage for NoopStorage {
    #[cfg_attr(coverage, coverage(off))]
    async fn get_latest_block(&self) -> Result<Option<Block>, sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_block_by_hash(&self, hash: BlockHash) -> Result<Option<Block>, sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_block_by_height(&self, height: u32) -> Result<Option<Block>, sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    fn get_blocks(
        &self,
        height: u32,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<Block, sqlx::Error>> {
        stream::empty()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_transaction_by_id(&self, id: u64) -> Result<Transaction, sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_transactions_by_block_id(&self, id: u64) -> Result<Vec<Transaction>, sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_transactions_by_hash(
        &self,
        hash: TransactionHash,
    ) -> Result<Vec<Transaction>, sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_transactions_by_identifier(
        &self,
        identifier: &Identifier,
    ) -> Result<Vec<Transaction>, sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_contract_deploy_by_address(
        &self,
        address: &ContractAddress,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_contract_action_by_address(
        &self,
        address: &ContractAddress,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_contract_action_by_address_and_block_hash(
        &self,
        address: &ContractAddress,
        hash: BlockHash,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_contract_action_by_address_and_block_height(
        &self,
        address: &ContractAddress,
        height: u32,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_contract_action_by_address_and_transaction_hash(
        &self,
        address: &ContractAddress,
        hash: TransactionHash,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_contract_action_by_address_and_transaction_identifier(
        &self,
        address: &ContractAddress,
        identifier: &Identifier,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_contract_actions_by_transaction_id(
        &self,
        id: u64,
    ) -> Result<Vec<ContractAction>, sqlx::Error> {
        unimplemented!()
    }

    fn get_contract_actions_by_address(
        &self,
        address: &ContractAddress,
        block_height: u32,
        contract_id: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<ContractAction, sqlx::Error>> + Send {
        stream::empty()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_highest_indices(
        &self,
        session_id: SessionId,
    ) -> Result<(Option<u64>, Option<u64>, Option<u64>), sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    fn get_relevant_transactions(
        &self,
        session_id: SessionId,
        index: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<Transaction, sqlx::Error>> + Send {
        stream::empty()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn connect_wallet(&self, viewing_key: &ViewingKey) -> Result<(), sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn disconnect_wallet(&self, session_id: SessionId) -> Result<(), sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn set_wallet_active(&self, session_id: SessionId) -> Result<(), sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_unshielded_utxos(
        &self,
        address: Option<&UnshieldedAddress>,
        filter: UnshieldedUtxoFilter<'_>,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_transactions_involving_unshielded(
        &self,
        address: &UnshieldedAddress,
    ) -> Result<Vec<Transaction>, sqlx::Error> {
        unimplemented!()
    }
}
