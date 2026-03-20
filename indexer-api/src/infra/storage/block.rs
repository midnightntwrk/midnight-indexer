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

use crate::{
    domain::{Block, storage::block::BlockStorage},
    infra::storage::Storage,
};
use async_stream::try_stream;
use fastrace::trace;
use futures::{Stream, StreamExt, TryStreamExt};
use indexer_common::{domain::BlockHash, stream::flatten_chunks};
use indoc::indoc;
use log::debug;
use std::num::NonZeroU32;

impl BlockStorage for Storage {
    #[trace]
    async fn get_latest_block(&self) -> Result<Option<Block>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                id,
                hash,
                height,
                protocol_version,
                parent_hash,
                author,
                timestamp,
                ledger_parameters
            FROM blocks
            ORDER BY height DESC
            LIMIT 1
        "};

        sqlx::query_as(query).fetch_optional(&*self.pool).await
    }

    #[trace(properties = { "hash": "{hash}" })]
    async fn get_block_by_hash(&self, hash: BlockHash) -> Result<Option<Block>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                id,
                hash,
                height,
                protocol_version,
                parent_hash,
                author,
                timestamp,
                ledger_parameters
            FROM blocks
            WHERE hash = $1
            LIMIT 1
        "};

        sqlx::query_as(query)
            .bind(hash.as_ref())
            .fetch_optional(&*self.pool)
            .await
    }

    #[trace(properties = { "height": "{height}" })]
    async fn get_block_by_height(&self, height: u32) -> Result<Option<Block>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                id,
                hash,
                height,
                protocol_version,
                parent_hash,
                author,
                timestamp,
                ledger_parameters
            FROM blocks
            WHERE height = $1
            LIMIT 1
        "};

        sqlx::query_as(query)
            .bind(height as i64)
            .fetch_optional(&*self.pool)
            .await
    }

    fn get_blocks(
        &self,
        mut height: u32,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<Block, sqlx::Error>> {
        let chunks = try_stream! {
            loop {
                let blocks = self.get_blocks(height, batch_size).await?;

                match blocks.last() {
                    Some(block) => height = block.height + 1,
                    None => break,
                }

                yield blocks;
            }
        };

        flatten_chunks(chunks)
    }
}

const BLOCK_SELECT: &str = "SELECT id, hash, height, protocol_version, parent_hash, author, timestamp, ledger_parameters FROM blocks";

impl Storage {
    #[trace(properties = { "hashes": "{hashes:?}" })]
    pub fn get_blocks_by_hashes(
        &self,
        hashes: Vec<BlockHash>,
    ) -> impl Stream<Item = Result<Block, sqlx::Error>> + Send + 'static {
        let pool = self.pool.clone();
        try_stream! {
            let mut query_builder = sqlx::QueryBuilder::new(BLOCK_SELECT);
            query_builder.push(" WHERE hash ");

            #[cfg(feature = "cloud")]
            {
                query_builder.push(" = ANY(");
                query_builder.push_bind(hashes.iter().map(|h| h.as_ref()).collect::<Vec<_>>());
                query_builder.push(")");
            }

            #[cfg(all(feature = "standalone", not(feature = "cloud")))]
            {
                query_builder.push(" IN (");
                let mut separated = query_builder.separated(", ");
                for hash in hashes {
                    separated.push_bind(hash.as_ref().to_vec());
                }
                query_builder.push(")");
            }

            let mut stream = query_builder.build_query_as::<Block>().fetch(&*pool);
            while let Some(block) = stream.try_next().await? {
                yield block;
            }
        }
        .boxed()
    }

    #[trace(properties = { "heights": "{heights:?}" })]
    pub fn get_blocks_by_heights(
        &self,
        heights: Vec<u32>,
    ) -> impl Stream<Item = Result<Block, sqlx::Error>> + Send + 'static {
        let pool = self.pool.clone();
        try_stream! {
            let mut query_builder = sqlx::QueryBuilder::new(BLOCK_SELECT);
            query_builder.push(" WHERE height ");

            #[cfg(feature = "cloud")]
            {
                query_builder.push(" = ANY(");
                query_builder.push_bind(heights.iter().map(|&h| h as i64).collect::<Vec<_>>());
                query_builder.push(")");
            }

            #[cfg(all(feature = "standalone", not(feature = "cloud")))]
            {
                query_builder.push(" IN (");
                let mut separated = query_builder.separated(", ");
                for height in heights {
                    separated.push_bind(height as i64);
                }
                query_builder.push(")");
            }

            let mut stream = query_builder.build_query_as::<Block>().fetch(&*pool);
            while let Some(block) = stream.try_next().await? {
                yield block;
            }
        }
        .boxed()
    }

    #[trace(properties = { "height": "{height}", "batch_size": "{batch_size}" })]
    async fn get_blocks(
        &self,
        height: u32,
        batch_size: NonZeroU32,
    ) -> Result<Vec<Block>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                id,
                hash,
                height,
                protocol_version,
                parent_hash,
                author,
                timestamp,
                ledger_parameters
            FROM blocks
            WHERE height >= $1
            ORDER BY height
            LIMIT $2
        "};

        sqlx::query_as(query)
            .bind(height as i64)
            .bind(batch_size.get() as i64)
            .fetch_all(&*self.pool)
            .await
    }
}
