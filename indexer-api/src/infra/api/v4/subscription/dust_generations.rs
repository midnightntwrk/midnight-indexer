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

use crate::{
    domain::{LedgerStateCache, storage::Storage},
    infra::api::{
        ApiResult, ContextExt, ResultExt,
        v4::{HexEncodable, HexEncoded, merkle_tree_collapsed_update::MerkleTreeCollapsedUpdate},
    },
};
use async_graphql::{Context, SimpleObject, Subscription, Union};
use async_stream::try_stream;
use futures::{Stream, TryStreamExt};
use indexer_common::domain::{BlockIndexed, Subscriber};
use log::{debug, warn};
use std::{marker::PhantomData, pin::pin};

/// An event of the dust generations subscription.
#[derive(Union)]
pub enum DustGenerationsEvent {
    DustGenerationsItem(DustGenerationsItem),
    DustGenerationsProgress(DustGenerationsProgress),
}

pub struct DustGenerationsSubscription<S, B> {
    _s: PhantomData<S>,
    _b: PhantomData<B>,
}

impl<S, B> Default for DustGenerationsSubscription<S, B> {
    fn default() -> Self {
        Self {
            _s: PhantomData,
            _b: PhantomData,
        }
    }
}

/// A dust generations item with optional collapsed Merkle tree update.
#[derive(Debug, Clone, SimpleObject)]
pub struct DustGenerationsItem {
    /// The Merkle tree index.
    pub merkle_index: u64,
    /// The hex-encoded owner (dust address).
    pub owner: HexEncoded,
    /// The NIGHT value in STAR.
    pub value: String,
    /// The hex-encoded nonce.
    pub nonce: HexEncoded,
    /// The creation timestamp.
    pub ctime: u64,
    /// The originating transaction ID.
    pub transaction_id: u64,
    /// Collapsed Merkle tree update filling the gap before this entry.
    pub collapsed_merkle_tree: Option<MerkleTreeCollapsedUpdate>,
}

/// Progress indicator for dust generations subscription (includes final collapsed update).
#[derive(Debug, Clone, SimpleObject)]
pub struct DustGenerationsProgress {
    /// The highest index processed so far.
    pub highest_index: u64,
    /// Final collapsed Merkle tree update covering remaining range.
    pub collapsed_merkle_tree: Option<MerkleTreeCollapsedUpdate>,
}

#[Subscription]
impl<S, B> DustGenerationsSubscription<S, B>
where
    S: Storage,
    B: Subscriber,
{
    /// Subscribe to dust generation entries for a dust address within an index range.
    /// Entries are interleaved with collapsed Merkle tree updates to fill gaps.
    /// The subscription finishes after reaching the end index with a final collapsed update.
    async fn dust_generations<'a>(
        &self,
        cx: &'a Context<'a>,
        dust_address: HexEncoded,
        start_index: u64,
        end_index: u64,
    ) -> impl Stream<Item = ApiResult<DustGenerationsEvent>> {
        let storage = cx.get_storage::<S>();
        let subscriber = cx.get_subscriber::<B>();
        let ledger_state_cache = cx.get_ledger_state_cache();
        let batch_size = cx.get_subscription_config().dust_generations.batch_size;

        let block_indexed_stream = subscriber.subscribe::<BlockIndexed>();

        try_stream! {
            let dust_address_bytes = const_hex::decode(dust_address.as_ref())
                .map_err_into_client_error(|| "invalid hex-encoded dust address")?;
            let mut cursor = start_index;

            debug!(start_index, end_index; "streaming existing dust generation entries");

            let entries = storage
                .get_dust_generation_entries(&dust_address_bytes, cursor, end_index, batch_size)
                .await;
            let mut entries = pin!(entries);
            while let Some(entry) = entries
                .try_next()
                .await
                .map_err_into_server_error(|| "get next dust generation entry")?
            {
                let collapsed_merkle_tree = make_collapsed_update(
                    cursor,
                    entry.merkle_index,
                    storage,
                    ledger_state_cache,
                ).await?;

                cursor = entry.merkle_index + 1;

                yield DustGenerationsEvent::DustGenerationsItem(DustGenerationsItem {
                    merkle_index: entry.merkle_index,
                    owner: entry.owner.hex_encode(),
                    value: entry.value.to_string(),
                    nonce: entry.nonce.hex_encode(),
                    ctime: entry.ctime,
                    transaction_id: entry.transaction_id,
                    collapsed_merkle_tree,
                });
            }

            if cursor > end_index {
                let final_update = make_final_collapsed_update(
                    cursor, end_index, storage, ledger_state_cache,
                ).await?;

                yield DustGenerationsEvent::DustGenerationsProgress(DustGenerationsProgress {
                    highest_index: end_index,
                    collapsed_merkle_tree: final_update,
                });

                return;
            }

            debug!(cursor; "streaming live dust generation entries");
            let mut block_indexed_stream = pin!(block_indexed_stream);
            while block_indexed_stream
                .try_next()
                .await
                .map_err_into_server_error(|| "get next BlockIndexed event")?
                .is_some()
            {
                let entries = storage
                    .get_dust_generation_entries(&dust_address_bytes, cursor, end_index, batch_size)
                    .await;
                let mut entries = pin!(entries);
                while let Some(entry) = entries
                    .try_next()
                    .await
                    .map_err_into_server_error(|| "get next dust generation entry")?
                {
                    let collapsed_merkle_tree = make_collapsed_update(
                        cursor,
                        entry.merkle_index,
                        storage,
                        ledger_state_cache,
                    ).await?;

                    cursor = entry.merkle_index + 1;

                    yield DustGenerationsEvent::DustGenerationsItem(DustGenerationsItem {
                        merkle_index: entry.merkle_index,
                        owner: entry.owner.hex_encode(),
                        value: entry.value.to_string(),
                        nonce: entry.nonce.hex_encode(),
                        ctime: entry.ctime,
                        transaction_id: entry.transaction_id,
                        collapsed_merkle_tree,
                    });
                }

                // Check if we've now reached end_index.
                if cursor > end_index {
                    let final_update = make_final_collapsed_update(
                        cursor, end_index, storage, ledger_state_cache,
                    ).await?;

                    yield DustGenerationsEvent::DustGenerationsProgress(DustGenerationsProgress {
                        highest_index: end_index,
                        collapsed_merkle_tree: final_update,
                    });

                    return;
                }
            }

            warn!("stream of BlockIndexed events completed unexpectedly");
        }
    }
}

/// Compute a collapsed Merkle tree update to fill the gap between cursor and entry's index.
async fn make_collapsed_update<S: Storage>(
    cursor: u64,
    entry_index: u64,
    storage: &S,
    ledger_state_cache: &LedgerStateCache,
) -> ApiResult<Option<MerkleTreeCollapsedUpdate>> {
    if cursor >= entry_index || entry_index == 0 {
        return Ok(None);
    }

    let block = storage
        .get_latest_block()
        .await
        .map_err_into_server_error(|| "get latest block for collapsed update")?;

    let Some(block) = block else {
        return Ok(None);
    };

    let update = ledger_state_cache
        .dust_generations_collapsed_update(cursor, entry_index - 1, storage, block.protocol_version)
        .await
        .map_err_into_server_error(|| "create dust generations collapsed update")?;

    Ok(Some(update.into()))
}

/// Compute the final collapsed Merkle tree update covering the remaining range.
async fn make_final_collapsed_update<S: Storage>(
    cursor: u64,
    end_index: u64,
    storage: &S,
    ledger_state_cache: &LedgerStateCache,
) -> ApiResult<Option<MerkleTreeCollapsedUpdate>> {
    let block = storage
        .get_latest_block()
        .await
        .map_err_into_server_error(|| "get latest block for final collapsed update")?;

    let Some(block) = block else {
        return Ok(None);
    };

    let update = ledger_state_cache
        .dust_generations_collapsed_update(cursor, end_index, storage, block.protocol_version)
        .await;

    match update {
        Ok(update) => Ok(Some(update.into())),
        Err(_) => Ok(None),
    }
}
