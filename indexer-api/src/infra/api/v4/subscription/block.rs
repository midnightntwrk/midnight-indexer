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
    domain::{self, storage::Storage},
    infra::api::{
        ApiError, ApiResult, ContextExt, ResultExt,
        v4::{
            block::{Block, BlockOffset},
            resolve_height,
        },
    },
};
use async_graphql::{Context, Subscription};
use async_stream::try_stream;
use fastrace::{Span, future::FutureExt, prelude::SpanContext};
use futures::{Stream, TryStreamExt};
use log::debug;
use std::{marker::PhantomData, pin::pin};

pub struct BlockSubscription<S> {
    _s: PhantomData<S>,
}

impl<S> Default for BlockSubscription<S> {
    fn default() -> Self {
        Self { _s: PhantomData }
    }
}

#[Subscription]
impl<S> BlockSubscription<S>
where
    S: Storage,
{
    /// Subscribe to blocks starting at the given offset or at the latest block if the offset is
    /// omitted.
    async fn blocks<'a>(
        &self,
        cx: &'a Context<'a>,
        offset: Option<BlockOffset>,
    ) -> Result<impl Stream<Item = ApiResult<Block<S>>> + use<'a, S>, ApiError> {
        let storage = cx.get_storage::<S>();
        let batch_size = cx.get_subscription_config().blocks.batch_size;

        // 1. Resolve starting height
        let mut height = resolve_height(offset, storage).await?;

        let blocks = try_stream! {
            // 2. Stream existing blocks from DB.
            debug!(height; "streaming blocks");
            let db_blocks = storage.get_blocks(height, batch_size);
            let mut db_blocks = pin!(db_blocks);
            while let Some(block) = get_next_block(&mut db_blocks).await
                .map_err_into_server_error(|| format!("get next block at height {height}"))?
            {
                height = block.height + 1;
                yield block.into();
            }
        };

        Ok(blocks)
    }
}

async fn get_next_block<E>(
    blocks: &mut (impl Stream<Item = Result<domain::Block, E>> + Unpin),
) -> Result<Option<domain::Block>, E> {
    blocks
        .try_next()
        .in_span(Span::root(
            "subscription.blocks.get-next-block",
            SpanContext::random(),
        ))
        .await
}
