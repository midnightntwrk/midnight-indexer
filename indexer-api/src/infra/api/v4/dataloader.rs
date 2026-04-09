// This file is part of midnight-indexer.
// Copyright (C) Midnight Foundation
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

use crate::domain::{self, storage::Storage};
use async_graphql::dataloader::Loader;
use derive_more::Deref;
use indexer_common::domain::BlockHash;
use std::{collections::HashMap, sync::Arc};

#[derive(Deref)]
pub struct BlockByHashLoader<S>(S);

impl<S: Storage> BlockByHashLoader<S> {
    pub fn new(storage: S) -> Self {
        Self(storage)
    }
}

impl<S: Storage> Loader<BlockHash> for BlockByHashLoader<S> {
    type Value = domain::Block;
    type Error = Arc<sqlx::Error>;

    async fn load(
        &self,
        keys: &[BlockHash],
    ) -> Result<HashMap<BlockHash, domain::Block>, Arc<sqlx::Error>> {
        let blocks = self
            .get_blocks_by_hashes(keys)
            .await
            .map_err(Arc::new)?
            .into_iter()
            .map(|b| (b.hash, b))
            .collect();
        Ok(blocks)
    }
}
