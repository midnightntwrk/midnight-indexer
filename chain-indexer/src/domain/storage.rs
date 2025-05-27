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

use crate::domain::{Block, BlockInfo, Transaction};
use futures::Stream;

/// Storage abstraction.
#[trait_variant::make(Send)]
pub trait Storage
where
    Self: Clone + Send + Sync + 'static,
{
    /// Get the hash and height of the highest stored [Block].
    async fn get_highest_block(&self) -> Result<Option<BlockInfo>, sqlx::Error>;

    /// Get the number of stored transactions.
    async fn get_transaction_count(&self) -> Result<u64, sqlx::Error>;

    /// Get the number of stored contract actions: deploys, calls, updates.
    async fn get_contract_action_count(&self) -> Result<(u64, u64, u64), sqlx::Error>;

    /// Save the given [Block] and return the max transaction ID.
    async fn save_block(&self, block: &Block) -> Result<Option<u64>, sqlx::Error>;

    /// Get a stream of transaction chunks for all blocks starting at the given height until the
    /// given height.
    fn get_transactions(
        &self,
        from_block_height: u32,
        to_block_height: u32,
    ) -> impl Stream<Item = Result<Vec<Transaction>, sqlx::Error>> + Send;
}
