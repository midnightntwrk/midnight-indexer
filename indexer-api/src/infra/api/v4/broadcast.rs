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

use crate::domain::storage::Storage;
use indexer_common::domain::{BlockIndexed, Subscriber};
use log::{debug, error, info};
use std::sync::Arc;
use tokio::sync::broadcast;
use futures::TryStreamExt;
use std::pin::pin;

/// A service that broadcasts new blocks to all subscribers.
/// This reduces database load by fetching the block once and fanning it out.
pub struct BlockBroadcast<S: Storage> {
    pub tx: broadcast::Sender<crate::domain::Block>,
    _s: std::marker::PhantomData<S>,
}

impl<S: Storage> BlockBroadcast<S> {
    pub fn new<B: Subscriber>(storage: S, subscriber: B) -> Arc<Self> {
        let (tx, _) = broadcast::channel(128); // Buffer size for new blocks
        let broadcast_tx = tx.clone();
        
        tokio::spawn(async move {
            info!("starting block broadcast service");
            let mut block_indexed_stream = pin!(subscriber.subscribe::<BlockIndexed>());
            let mut last_broadcasted_height = None;
            
            while let Ok(Some(_)) = block_indexed_stream.try_next().await {
                debug!("received BlockIndexed event, checking for new blocks");
                
                // If we don't know the starting height, get the latest one first
                let start_height = match last_broadcasted_height {
                    Some(h) => h + 1,
                    None => match storage.get_latest_block().await {
                        Ok(Some(b)) => {
                            last_broadcasted_height = Some(b.height);
                            // Broadcast the very first one we find too
                            let _ = broadcast_tx.send(b.clone());
                            b.height + 1
                        }
                        _ => 0,
                    }
                };

                // Fetch any blocks that came in since last time (up to 10 at once)
                let blocks = storage.get_blocks(start_height, std::num::NonZeroU32::new(10).unwrap());
                let mut blocks = pin!(blocks);
                while let Ok(Some(block)) = blocks.try_next().await {
                    debug!("broadcasting block at height {}", block.height);
                    last_broadcasted_height = Some(block.height);
                    if let Err(e) = broadcast_tx.send(block) {
                        debug!("no active subscribers for block broadcast: {}", e);
                    }
                }
            }
            error!("BlockIndexed stream closed unexpectedly in BlockBroadcast");
        });

        Arc::new(Self {
            tx,
            _s: std::marker::PhantomData,
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<crate::domain::Block> {
        self.tx.subscribe()
    }
}
