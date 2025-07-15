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
pub mod dust;
pub mod transaction;
pub mod unshielded;
pub mod wallet;

use crate::domain::storage::{
    block::BlockStorage, contract_action::ContractActionStorage, dust::DustStorage,
    transaction::TransactionStorage, unshielded::UnshieldedUtxoStorage, wallet::WalletStorage,
};
use std::fmt::Debug;

/// Storage abstraction.
#[trait_variant::make(Send)]
pub trait Storage
where
    Self: BlockStorage
        + ContractActionStorage
        + DustStorage
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

impl DustStorage for NoopStorage {
    async fn get_current_dust_state(&self) -> Result<crate::domain::DustSystemState, sqlx::Error> {
        Ok(crate::domain::DustSystemState {
            commitment_tree_root:
                "0000000000000000000000000000000000000000000000000000000000000000".to_owned(),
            generation_tree_root:
                "0000000000000000000000000000000000000000000000000000000000000000".to_owned(),
            block_height: 0,
            timestamp: 0,
            total_registrations: 0,
        })
    }

    async fn get_dust_generation_status_batch(
        &self,
        _cardano_stake_keys: &[String],
    ) -> Result<Vec<crate::domain::DustGenerationStatus>, sqlx::Error> {
        Ok(vec![])
    }

    async fn get_dust_merkle_root_at_timestamp(
        &self,
        _tree_type: indexer_common::domain::DustMerkleTreeType,
        _timestamp: i64,
    ) -> Result<Option<Vec<u8>>, sqlx::Error> {
        Ok(None)
    }

    fn get_dust_generations(
        &self,
        _dust_address: &str,
        _from_generation_index: i64,
        _from_merkle_index: i64,
        _only_active: bool,
        _batch_size: std::num::NonZeroU32,
    ) -> impl futures::Stream<Item = Result<crate::domain::DustGenerationEvent, sqlx::Error>> {
        futures::stream::empty()
    }

    fn get_dust_nullifier_transactions(
        &self,
        _prefixes: &[String],
        _min_prefix_length: usize,
        _from_block: i64,
        _batch_size: std::num::NonZeroU32,
    ) -> impl futures::Stream<Item = Result<crate::domain::DustNullifierTransactionEvent, sqlx::Error>>
    {
        futures::stream::empty()
    }

    fn get_dust_commitments(
        &self,
        _commitment_prefixes: &[String],
        _min_prefix_length: usize,
        _start_index: i64,
        _batch_size: std::num::NonZeroU32,
    ) -> impl futures::Stream<Item = Result<crate::domain::DustCommitmentEvent, sqlx::Error>> {
        futures::stream::empty()
    }

    fn get_registration_updates(
        &self,
        _addresses: &[(indexer_common::domain::AddressType, String)],
        _from_timestamp: i64,
        _batch_size: std::num::NonZeroU32,
    ) -> impl futures::Stream<Item = Result<crate::domain::RegistrationUpdateEvent, sqlx::Error>>
    {
        futures::stream::empty()
    }
}

impl Storage for NoopStorage {}
