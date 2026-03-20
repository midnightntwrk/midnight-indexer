// This file is part of midnight-indexer.
// Copyright (C) Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Internal `DataLoader` implementations for the infra storage layer.
//!
//! These loaders batch single-item lookups into bulk SQL queries transparently.
//! They are NOT part of the domain — the domain traits remain unchanged.

use crate::{domain::Block, infra::storage::Storage};
use async_graphql::dataloader::Loader;
use futures::TryStreamExt;
use indexer_common::domain::BlockHash;
use std::{collections::HashMap, sync::Arc};

// ---------------------------------------------------------------------------
// Block loaders
// ---------------------------------------------------------------------------

pub struct BlockByHashLoader(pub(crate) Storage);

impl Loader<BlockHash> for BlockByHashLoader {
    type Value = Block;
    type Error = Arc<sqlx::Error>;

    async fn load(
        &self,
        keys: &[BlockHash],
    ) -> Result<HashMap<BlockHash, Block>, Arc<sqlx::Error>> {
        self.0
            .get_blocks_by_hashes(keys)
            .map_ok(|block| (block.hash.clone(), block))
            .try_collect()
            .await
            .map_err(Arc::new)
    }
}

pub struct BlockByHeightLoader(pub(crate) Storage);

impl Loader<u32> for BlockByHeightLoader {
    type Value = Block;
    type Error = Arc<sqlx::Error>;

    async fn load(&self, keys: &[u32]) -> Result<HashMap<u32, Block>, Arc<sqlx::Error>> {
        self.0
            .get_blocks_by_heights(keys)
            .map_ok(|block| (block.height, block))
            .try_collect()
            .await
            .map_err(Arc::new)
    }
}
