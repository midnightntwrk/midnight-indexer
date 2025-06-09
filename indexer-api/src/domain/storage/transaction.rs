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

use crate::domain::{Transaction, storage::NoopStorage};
use futures::{Stream, stream};
use indexer_common::domain::{Identifier, SessionId, TransactionHash, UnshieldedAddress};
use std::{fmt::Debug, num::NonZeroU32};

/// Storage abstraction.
#[trait_variant::make(Send)]
pub trait TransactionStorage
where
    Self: Debug + Clone + Send + Sync + 'static,
{
    /// Get a transaction for the given ID.
    async fn get_transaction_by_id(&self, id: u64) -> Result<Option<Transaction>, sqlx::Error>;

    /// Get the transactions for the block with the given ID.
    async fn get_transactions_by_block_id(&self, id: u64) -> Result<Vec<Transaction>, sqlx::Error>;

    /// Get transactions for the given hash, ordered descendingly by ID. Transaction hashes are
    /// unique for successful transactions, yet not for failed ones.
    async fn get_transactions_by_hash(
        &self,
        hash: TransactionHash,
    ) -> Result<Vec<Transaction>, sqlx::Error>;

    /// Get transactions for the given identifier. Identifiers are not unique.
    async fn get_transactions_by_identifier(
        &self,
        identifier: &Identifier,
    ) -> Result<Vec<Transaction>, sqlx::Error>;

    /// Get a stream of all transactions relevant for a wallet with the given session ID, starting
    /// at the given index.
    fn get_relevant_transactions(
        &self,
        session_id: SessionId,
        index: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<Transaction, sqlx::Error>> + Send;

    /// Get all transactions that create or spend unshielded UTXOs for the given address.
    async fn get_transactions_involving_unshielded(
        &self,
        address: &UnshieldedAddress,
    ) -> Result<Vec<Transaction>, sqlx::Error>;

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
