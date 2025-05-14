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

use crate::{
    domain::Storage,
    infra::api::{
        ContextExt, ResultExt,
        v1::{Block, BlockOffset, resolve_height},
    },
};
use async_graphql::{Context, Subscription, async_stream::try_stream};
use futures::{Stream, TryStreamExt};
use indexer_common::domain::{BlockIndexed, Subscriber};
use log::{debug, warn};
use metrics::{Counter, counter};
use std::{marker::PhantomData, num::NonZeroU32, pin::pin};

// TODO: Make configurable!
const BATCH_SIZE: NonZeroU32 = NonZeroU32::new(100).unwrap();

pub struct BlockSubscription<S, B> {
    blocks_calls: Counter,
    _s: PhantomData<S>,
    _b: PhantomData<B>,
}

impl<S, B> Default for BlockSubscription<S, B> {
    fn default() -> Self {
        let blocks_calls = counter!("indexer_api_calls_subscription_blocks");

        Self {
            blocks_calls,
            _s: PhantomData,
            _b: PhantomData,
        }
    }
}

#[Subscription]
impl<S, B> BlockSubscription<S, B>
where
    S: Storage,
    B: Subscriber,
{
    /// Subscribe to blocks.
    async fn blocks<'a>(
        &self,
        cx: &'a Context<'a>,
        offset: Option<BlockOffset>,
    ) -> async_graphql::Result<impl Stream<Item = async_graphql::Result<Block<S>>> + use<'a, S, B>>
    {
        self.blocks_calls.increment(1);

        let storage = cx.get_storage::<S>();
        let subscriber = cx.get_subscriber::<B>();

        let block_indexed_stream = subscriber
            .subscribe::<BlockIndexed>()
            .await
            .internal("cannot subscribe to BlockIndexed events")?;

        // It is fine to use `?` on `resolve_height`, because it implements correct error handling.
        let mut height = resolve_height(offset, storage).await?;

        let blocks_stream = try_stream! {
            let mut block_indexed_stream = pin!(block_indexed_stream);

            // First get all stored `Block`s from the requested `height`.
            let blocks = storage.get_blocks(height, BATCH_SIZE);
            debug!(height; "got blocks");

            // Then yield all stored `Block`s.
            let mut blocks = pin!(blocks);
            while let Some(block) = blocks.try_next().await.internal("get next block")? {
                assert_eq!(block.height, height);
                height += 1;

                yield block.into();
            }

            // Then get now stored `Block`s after receiving a `BlockIndexed` event.
            while block_indexed_stream
                .try_next()
                .await
                .internal("get next BlockIndexed event")?
                .is_some()
            {
                debug!("handling BlockIndexed event");

                let blocks = storage.get_blocks(height, BATCH_SIZE);
                let mut blocks = pin!(blocks);
                while let Some(block) = blocks.try_next().await.internal("get next block")? {
                    assert_eq!(block.height, height);
                    height += 1;

                    yield block.into();
                }
            }

            warn!("stream of BlockIndexed events completed unexpectedly");
        };

        Ok(blocks_stream)
    }
}
