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
        v2::{into_from_height, Block, BlockOffsetInput},
        ContextExt,
    },
};
use async_graphql::{async_stream::try_stream, Context, Subscription};
use futures::{Stream, TryStreamExt};
use indexer_common::{
    domain::{BlockIndexed, Subscriber},
    error::StdErrorExt,
};
use std::{marker::PhantomData, num::NonZeroU32, pin::pin};
use tracing::{debug, error, warn};

// TODO: Make configurable!
const BATCH_SIZE: NonZeroU32 = unsafe { NonZeroU32::new_unchecked(100) };

pub struct BlockSubscription<S, B> {
    _s: PhantomData<S>,
    _b: PhantomData<B>,
}

impl<S, B> Default for BlockSubscription<S, B> {
    fn default() -> Self {
        Self {
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
    /// Subscribe to block events.
    async fn blocks<'a>(
        &self,
        cx: &'a Context<'a>,
        offset: Option<BlockOffsetInput>,
    ) -> async_graphql::Result<impl Stream<Item = async_graphql::Result<Block<S>>> + 'a> {
        let storage = cx.get_storage::<S>()?;
        let subscriber = cx.get_subscriber::<B>()?;

        let block_indexed_stream =
            subscriber
                .subscribe::<BlockIndexed>()
                .await
                .inspect_err(|error| {
                    error!(
                        error = error.as_chain(),
                        "cannot subscribe to BlockIndexed events"
                    )
                })?;

        let from_height = into_from_height(offset, storage).await?;

        let blocks_stream = try_stream! {
            let mut block_indexed_stream = pin!(block_indexed_stream);
            let mut next_from_height = from_height;

            // First get all stored `Block`s from the requested `from_height`.
            let blocks = storage.get_blocks(from_height, BATCH_SIZE);
            debug!(from_height, "got blocks");

            // Then yield all stored `Block`s.
            let mut blocks = pin!(blocks);
            while let Some(block) = blocks
                .try_next()
                .await
                .inspect_err(|error| error!(error = error.as_chain(), "cannot get next block"))?
            {
                assert_eq!(block.height, next_from_height);
                next_from_height += 1;

                yield block.into();
            }

            // Then get now stored `Block`s after receiving a `BlockIndexed` event.
            while let Some(BlockIndexed { height }) =
                block_indexed_stream.try_next().await.inspect_err(|error| {
                    error!(
                        error = error.as_chain(),
                        "cannot get next BlockIndexed event"
                    )
                })?
            {
                debug!(height, "handling BlockIndexed event");

                // The next height cannot be less than the so far last height!
                assert!(height >= next_from_height);

                let blocks = storage.get_blocks(next_from_height, BATCH_SIZE);
                let mut blocks = pin!(blocks);
                while let Some(block) = blocks.try_next().await.inspect_err(|error| {
                    error!(error = error.as_chain(), "cannot get next block")
                })? {
                    assert_eq!(block.height, next_from_height);
                    next_from_height += 1;

                    yield block.into();
                }
            }

            warn!("stream of BlockIndexed events completed unexpectedly");
        };

        Ok(blocks_stream)
    }
}
