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
    domain::{LedgerStateCache, dust::DustGenerationDtimeUpdateEntry, storage::Storage},
    infra::api::{
        ApiResult, ContextExt, ResultExt,
        v4::{
            HexEncodable, HexEncoded, dust::DustAddress,
            merkle_tree_collapsed_update::MerkleTreeCollapsedUpdate,
        },
    },
};
use async_graphql::{Context, SimpleObject, Subscription, Union};
use async_stream::try_stream;
use futures::{Stream, TryStreamExt};
use indexer_common::domain::{BlockIndexed, ByteVec, Subscriber};
use log::{debug, warn};
use std::{marker::PhantomData, pin::pin};

/// An event of the dust generations subscription.
// The `Dust*` prefix on every variant matches the existing GraphQL type
// names; renaming would change the public union members.
#[allow(clippy::enum_variant_names)]
#[derive(Union)]
pub enum DustGenerationsEvent {
    DustGenerationsItem(DustGenerationsItem),
    DustGenerationsProgress(DustGenerationsProgress),
    DustGenerationDtimeUpdateItem(DustGenerationDtimeUpdateItem),
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
    /// Index of this output in the dust commitment Merkle tree.
    pub commitment_mt_index: u64,
    /// Index of this output in the dust generation Merkle tree.
    pub generation_mt_index: u64,
    /// The hex-encoded owner (dust address).
    pub owner: HexEncoded,
    /// The NIGHT value backing this output, in STAR.
    pub value: String,
    /// The DUST value at creation, in SPECK.
    pub initial_value: String,
    /// Hex-encoded hash of the NIGHT UTXO that backs this dust output.
    pub backing_night: HexEncoded,
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

/// A dust generation dtime update emitted when the backing Night UTXO is
/// spent and the entry's decay time is set.
#[derive(Debug, Clone, SimpleObject)]
pub struct DustGenerationDtimeUpdateItem {
    /// Generation-tree index of the entry whose dtime changed.
    pub generation_mt_index: u64,
    /// The hex-encoded owner (dust address).
    pub owner: HexEncoded,
    /// Hex-encoded hash of the NIGHT UTXO that backs this dust output.
    pub night_utxo_hash: HexEncoded,
    /// The decay time as observed in this ledger event.
    pub new_dtime: u64,
    /// The originating transaction ID.
    pub transaction_id: u64,
    /// Collapsed Merkle tree update covering the gap between the wallet's
    /// current cursor and this entry. `null` when the wallet has already
    /// passed this entry's index, which is the typical case for dtime
    /// updates on already-seen entries.
    pub collapsed_merkle_tree: Option<MerkleTreeCollapsedUpdate>,
    /// Path from the updated leaf to the root of the dust generation tree,
    /// matching the ledger's `TreeInsertionPath<DustGenerationInfo>`.
    /// Wallets apply this via `generating_tree.update_from_evidence(...)`.
    pub merkle_path: Vec<DustMerklePathEntry>,
}

/// One entry in a `DustGenerationDtimeUpdateItem.merklePath`. Mirrors the
/// ledger's `TreeInsertionPathEntry`: the hash is the node along the path
/// from leaf to root (not the sibling), and may be `null` if the tree was
/// not fully rehashed at the point the update was constructed.
#[derive(Debug, Clone, SimpleObject)]
pub struct DustMerklePathEntry {
    /// Hex-encoded hash of the node along the path. `null` when the
    /// upstream `TreeInsertionPathEntry.hash` was `None`.
    pub hash: Option<HexEncoded>,
    /// Whether the path went left at this branch.
    pub goes_left: bool,
}

#[Subscription]
impl<S, B> DustGenerationsSubscription<S, B>
where
    S: Storage,
    B: Subscriber,
{
    /// Subscribe to dust generation entries for a dust address within an index
    /// range, interleaved with collapsed Merkle tree updates and
    /// `DustGenerationDtimeUpdateItem` events for entries the subscriber owns.
    /// Finishes at end_index with a final collapsed update.
    ///
    /// On reconnect, historical dtime updates after the wallet's last fully-
    /// synced block (derived from the entry below `startIndex`) are replayed
    /// before entry backfill. Fresh subscriptions skip historical dtime
    /// backfill; the wallet learns of pre-existing spends primarily via block
    /// sync and `dustNullifierTransactions`.
    async fn dust_generations<'a>(
        &self,
        cx: &'a Context<'a>,
        dust_address: DustAddress,
        start_index: u64,
        end_index: u64,
    ) -> impl Stream<Item = ApiResult<DustGenerationsEvent>> {
        let storage = cx.get_storage::<S>();
        let subscriber = cx.get_subscriber::<B>();
        let ledger_state_cache = cx.get_ledger_state_cache();
        let batch_size = cx.get_subscription_config().dust_generations.batch_size;
        let network_id = cx.get_network_id();

        let block_indexed_stream = subscriber.subscribe::<BlockIndexed>();

        try_stream! {
            let dust_address_bytes = dust_address
                .try_into_domain(network_id)
                .map_err_into_client_error(|| "invalid bech32m dust address")?;
            let mut cursor = start_index;

            // Derive the dtime cutoff from the wallet's most recent owned
            // entry below `start_index`. `None` for fresh subscriptions; we
            // skip historical dtime backfill in that case.
            let dtime_cutoff_block_id = storage
                .get_dust_generation_dtime_cutoff_block_id(&dust_address_bytes, start_index)
                .await
                .map_err_into_server_error(|| "get dtime cutoff block id")?;

            // Single dtime cursor across initial backfill and live tail. It
            // advances past every emitted DustGenerationDtimeUpdateItem so
            // subsequent block-driven polls don't re-emit.
            let mut dtime_after_event_id = 0u64;

            if let Some(cutoff) = dtime_cutoff_block_id {
                debug!(cutoff; "replaying dtime updates after cutoff");
                let updates = storage
                    .get_dust_generation_dtime_updates(
                        &dust_address_bytes,
                        cutoff,
                        dtime_after_event_id,
                        batch_size,
                    )
                    .await;
                let mut updates = pin!(updates);
                while let Some(update) = updates
                    .try_next()
                    .await
                    .map_err_into_server_error(|| "get next dtime update")?
                {
                    dtime_after_event_id = update.ledger_event_id;
                    let collapsed_merkle_tree = make_collapsed_update(
                        cursor,
                        update.generation_mt_index,
                        storage,
                        ledger_state_cache,
                    ).await?;
                    yield DustGenerationsEvent::DustGenerationDtimeUpdateItem(
                        dtime_update_item(update, collapsed_merkle_tree),
                    );
                }
            }

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
                    entry.generation_mt_index,
                    storage,
                    ledger_state_cache,
                ).await?;

                cursor = entry.generation_mt_index + 1;

                yield DustGenerationsEvent::DustGenerationsItem(DustGenerationsItem {
                    commitment_mt_index: entry.commitment_mt_index,
                    generation_mt_index: entry.generation_mt_index,
                    owner: entry.owner.hex_encode(),
                    value: entry.value.to_string(),
                    initial_value: entry.initial_value.to_string(),
                    backing_night: entry.backing_night.hex_encode(),
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
                        entry.generation_mt_index,
                        storage,
                        ledger_state_cache,
                    ).await?;

                    cursor = entry.generation_mt_index + 1;

                    yield DustGenerationsEvent::DustGenerationsItem(DustGenerationsItem {
                        commitment_mt_index: entry.commitment_mt_index,
                        generation_mt_index: entry.generation_mt_index,
                        owner: entry.owner.hex_encode(),
                        value: entry.value.to_string(),
                        initial_value: entry.initial_value.to_string(),
                        backing_night: entry.backing_night.hex_encode(),
                        ctime: entry.ctime,
                        transaction_id: entry.transaction_id,
                        collapsed_merkle_tree,
                    });
                }

                // Drain any new dtime updates for entries the subscriber
                // owns. Reuses the same cutoff (initial entry-block anchor)
                // and the running event cursor so we don't re-emit.
                if let Some(cutoff) = dtime_cutoff_block_id {
                    let updates = storage
                        .get_dust_generation_dtime_updates(
                            &dust_address_bytes,
                            cutoff,
                            dtime_after_event_id,
                            batch_size,
                        )
                        .await;
                    let mut updates = pin!(updates);
                    while let Some(update) = updates
                        .try_next()
                        .await
                        .map_err_into_server_error(|| "get next dtime update")?
                    {
                        dtime_after_event_id = update.ledger_event_id;
                        let collapsed_merkle_tree = make_collapsed_update(
                            cursor,
                            update.generation_mt_index,
                            storage,
                            ledger_state_cache,
                        ).await?;
                        yield DustGenerationsEvent::DustGenerationDtimeUpdateItem(
                            dtime_update_item(update, collapsed_merkle_tree),
                        );
                    }
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

/// Convert a domain dtime update entry into the GraphQL item, attaching a
/// caller-computed `collapsed_merkle_tree`. The `merkle_path` entries are
/// converted from raw bytes to `HexEncoded` here.
fn dtime_update_item(
    update: DustGenerationDtimeUpdateEntry,
    collapsed_merkle_tree: Option<MerkleTreeCollapsedUpdate>,
) -> DustGenerationDtimeUpdateItem {
    let merkle_path = update
        .merkle_path
        .into_iter()
        .map(|entry| DustMerklePathEntry {
            hash: entry
                .sibling_hash
                .map(|bytes| ByteVec::from(bytes).hex_encode()),
            goes_left: entry.goes_left,
        })
        .collect();

    DustGenerationDtimeUpdateItem {
        generation_mt_index: update.generation_mt_index,
        owner: update.owner.hex_encode(),
        night_utxo_hash: update.night_utxo_hash.hex_encode(),
        new_dtime: update.new_dtime,
        transaction_id: update.transaction_id,
        collapsed_merkle_tree,
        merkle_path,
    }
}
