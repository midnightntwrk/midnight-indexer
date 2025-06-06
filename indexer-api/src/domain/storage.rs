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

pub mod block;
pub mod contract_action;
pub mod transaction;
pub mod unshielded_utxo;
pub mod wallet;

use crate::domain::{
    Block, ContractAction, Transaction, UnshieldedUtxo,
    storage::{
        block::BlockStorage,
        contract_action::ContractActionStorage,
        transaction::TransactionStorage,
        unshielded_utxo::{UnshieldedUtxoFilter, UnshieldedUtxoStorage},
        wallet::WalletStorage,
    },
};
use futures::{Stream, stream};
use indexer_common::domain::{
    BlockHash, ContractAddress, Identifier, SessionId, TransactionHash, UnshieldedAddress,
    ViewingKey,
};
use std::{fmt::Debug, num::NonZeroU32};

/// Storage abstraction.
#[trait_variant::make(Send)]
pub trait Storage
where
    Self: BlockStorage
        + ContractActionStorage
        + TransactionStorage
        + UnshieldedUtxoStorage
        + WalletStorage
        + Debug
        + Clone
        + Send
        + Sync
        + 'static,
{
}

/// Just needed as a type argument for `infra::api::export_schema` which should not depend on any
/// features like "cloud" and hence types like `infra::postgres::PostgresStorage` cannot be used.
/// Once traits with async functions are object safe, this can go away and be replaced with
/// `Box<dyn Storage>` at the type level.
#[derive(Debug, Clone, Default)]
pub struct NoopStorage;

impl Storage for NoopStorage {}

#[allow(unused_variables)]
impl BlockStorage for NoopStorage {
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
}

#[allow(unused_variables)]
impl ContractActionStorage for NoopStorage {
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
}

#[allow(unused_variables)]
impl TransactionStorage for NoopStorage {
    #[cfg_attr(coverage, coverage(off))]
    async fn get_transaction_by_id(&self, id: u64) -> Result<Option<Transaction>, sqlx::Error> {
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
    fn get_relevant_transactions(
        &self,
        session_id: SessionId,
        index: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<Transaction, sqlx::Error>> + Send {
        stream::empty()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_transactions_involving_unshielded(
        &self,
        address: &UnshieldedAddress,
    ) -> Result<Vec<Transaction>, sqlx::Error> {
        unimplemented!()
    }

    #[cfg_attr(coverage, coverage(off))]
    async fn get_highest_indices(
        &self,
        session_id: SessionId,
    ) -> Result<(Option<u64>, Option<u64>, Option<u64>), sqlx::Error> {
        unimplemented!()
    }
}

#[allow(unused_variables)]
impl WalletStorage for NoopStorage {
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
}

#[allow(unused_variables)]
impl UnshieldedUtxoStorage for NoopStorage {
    #[cfg_attr(coverage, coverage(off))]
    async fn get_unshielded_utxos(
        &self,
        address: Option<&UnshieldedAddress>,
        filter: UnshieldedUtxoFilter<'_>,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        unimplemented!()
    }
}
