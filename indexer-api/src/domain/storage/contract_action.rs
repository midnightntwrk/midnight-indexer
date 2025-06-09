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

use crate::domain::{ContractAction, storage::NoopStorage};
use futures::{Stream, stream};
use indexer_common::domain::{BlockHash, ContractAddress, Identifier, TransactionHash};
use std::{fmt::Debug, num::NonZeroU32};

#[trait_variant::make(Send)]
pub trait ContractActionStorage
where
    Self: Debug + Clone + Send + Sync + 'static,
{
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

    #[cfg_attr(coverage, coverage(off))]
    fn get_contract_actions_by_address(
        &self,
        address: &ContractAddress,
        block_height: u32,
        contract_action_id: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<ContractAction, sqlx::Error>> + Send {
        stream::empty()
    }
}
