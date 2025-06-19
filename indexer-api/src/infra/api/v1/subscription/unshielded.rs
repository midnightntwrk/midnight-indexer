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
    domain::storage::Storage,
    infra::api::{
        ContextExt, ResultExt,
        v1::{UnshieldedAddress, UnshieldedProgress, UnshieldedUtxo, UnshieldedUtxoEvent},
    },
};
use async_graphql::{Context, Subscription, async_stream::try_stream};
use fastrace::trace;
use futures::{Stream, StreamExt, stream::TryStreamExt};
use indexer_common::domain::{ByteVec, NetworkId, Subscriber, UnshieldedUtxoIndexed};
use log::{debug, warn};
use std::{collections::HashSet, future::ready, marker::PhantomData, pin::pin, time::Duration};
use tokio::time::interval;
use tokio_stream::wrappers::IntervalStream;

// TODO: Make configurable!
const PROGRESS_UPDATES_INTERVAL: Duration = Duration::from_secs(30);

/// Same skeleton pattern as block / contract / wallet subscriptions
pub struct UnshieldedSubscription<S, B> {
    _s: PhantomData<S>,
    _b: PhantomData<B>,
}

impl<S, B> Default for UnshieldedSubscription<S, B> {
    fn default() -> Self {
        Self {
            _s: PhantomData,
            _b: PhantomData,
        }
    }
}

#[Subscription]
impl<S, B> UnshieldedSubscription<S, B>
where
    S: Storage,
    B: Subscriber,
{
    /// Subscribes to unshielded UTXO events for a specific address.
    /// Replays all historical transactions for the address from genesis before switching to live
    /// events. This ensures clients receive complete transaction history immediately upon
    /// subscription.
    ///
    /// # Arguments
    /// * `address` - The unshielded address to monitor (must be in Bech32m format)
    ///
    /// # Returns
    /// A stream of `UnshieldedUtxoEvent`s containing:
    /// - `progress`: Progress information for wallet synchronization (always present)
    /// - `transaction`: The transaction that created/spent UTXOs (None for progress-only events)
    /// - `createdUtxos`: UTXOs created in this transaction for the address (None for progress-only
    ///   events)
    /// - `spentUtxos`: UTXOs spent in this transaction for the address (None for progress-only
    ///   events)
    #[trace(properties = { "address": "{address:?}" })]
    async fn unshielded_utxos<'a>(
        &self,
        cx: &'a Context<'a>,
        address: UnshieldedAddress,
    ) -> async_graphql::Result<impl Stream<Item = async_graphql::Result<UnshieldedUtxoEvent<S>>> + 'a>
    {
        let encoded_address = address.0.clone();
        let network_id = cx.get_network_id();
        let address = address
            .try_into_domain(network_id)
            .internal("convert address into domain address")?;

        let encoded_address_for_update = encoded_address.clone();
        let update_events = unshielded_updates_with_historical_replay::<S, B>(
            cx,
            address.clone(),
            encoded_address.clone(),
            network_id,
        )
        .await?
        .map_ok(move |event| {
            debug!(address = encoded_address_for_update; "emitting UPDATE event");
            event
        });

        let encoded_address_for_progress = encoded_address.clone();
        let progress_updates = progress_updates::<S>(cx, address)
            .await?
            .map_ok(move |event| {
                debug!(address = encoded_address_for_progress; "emitting PROGRESS event");
                event
            });

        let events = tokio_stream::StreamExt::merge(update_events, progress_updates);

        Ok(events)
    }
}

#[trace(properties = { "address": "{api_address}" })]
async fn unshielded_updates_with_historical_replay<'a, S, B>(
    cx: &'a Context<'a>,
    domain_address: indexer_common::domain::UnshieldedAddress,
    api_address: String,
    network_id: NetworkId,
) -> async_graphql::Result<
    impl Stream<Item = async_graphql::Result<UnshieldedUtxoEvent<S>>> + use<'a, S, B>,
>
where
    S: Storage,
    B: Subscriber,
{
    let storage = cx.get_storage::<S>();
    let subscriber = cx.get_subscriber::<B>();

    let address_for_filter = domain_address.clone();
    let utxo_indexed_events = subscriber
        .subscribe::<UnshieldedUtxoIndexed>()
        .try_filter(move |event| ready(event.address == address_for_filter));

    let updates = try_stream! {
        debug!(api_address; "starting historical replay from genesis");

        // Phase 1: Replay all historical transactions for this address
        let historical_transactions = storage
            .get_transactions_involving_unshielded(&domain_address)
            .await
            .internal("fetch historical transactions for address")?;

        debug!(
            api_address,
            transaction_count = historical_transactions.len();
            "replaying historical transactions"
        );

        // Track which transactions we've already processed to avoid duplicates
        // when switching to live events.
        let mut processed_transaction_ids = HashSet::new();

        for transaction in historical_transactions {
            let transaction_id = transaction.id;
            processed_transaction_ids.insert(transaction_id);

            debug!(
                api_address,
                transaction_id,
                transaction_hash:% = transaction.hash;
                "processing historical transaction"
            );

            let created = storage
                .get_unshielded_utxos_created_in_transaction_for_address(&domain_address, transaction_id)
                .await
                .internal("fetch created UTXOs for historical transaction")?;

            let spent = storage
                .get_unshielded_utxos_spent_in_transaction_for_address(&domain_address, transaction_id)
                .await
                .internal("fetch spent UTXOs for historical transaction")?;

            // Calculate progress: current transaction's end_index relative to highest known.
            let (highest_index, _) = storage
                .get_highest_indices_for_address(&domain_address)
                .await
                .internal("fetch highest indices for address")?;
            let highest_index = highest_index.unwrap_or(0);
            let current_index = transaction.end_index;

            let progress = UnshieldedProgress {
                highest_index,
                current_index,
            };

            // Only emit events for transactions that actually have UTXOs for this address.
            if !created.is_empty() || !spent.is_empty() {
                yield UnshieldedUtxoEvent {
                    progress,
                    transaction: Some(transaction.into()),
                    created_utxos: Some(created.into_iter()
                        .map(|utxo| UnshieldedUtxo::<S>::from((utxo, network_id)))
                        .collect()),
                    spent_utxos: Some(spent.into_iter()
                        .map(|utxo| UnshieldedUtxo::<S>::from((utxo, network_id)))
                        .collect()),
                };
            }
        }

        debug!(
            api_address,
            processed_count = processed_transaction_ids.len();
            "completed historical replay, switching to live events"
        );

        // Phase 2: Switch to live event streaming.
        let mut utxo_indexed_events = pin!(utxo_indexed_events);
        while let Some(UnshieldedUtxoIndexed { address: _, transaction_id }) = utxo_indexed_events
            .try_next()
            .await
            .internal("get next UnshieldedUtxoIndexed event")?
        {
            // Skip transactions we've already processed during historical replay.
            if processed_transaction_ids.contains(&transaction_id) {
                debug!(
                    api_address,
                    transaction_id;
                    "skipping already processed transaction from live events"
                );
                continue;
            }

            debug!(
                api_address,
                transaction_id;
                "handling live UnshieldedUtxoIndexed event"
            );

            let tx = storage
                .get_transaction_by_id(transaction_id)
                .await
                .internal("fetch tx for live subscription event")?;

            let tx = match tx {
                Some(tx) => tx,
                None => {
                    warn!(transaction_id; "transaction not found in live event, skipping");
                    continue;
                }
            };

            let created = storage
                .get_unshielded_utxos_created_in_transaction_for_address(&domain_address, transaction_id)
                .await
                .internal("fetch created UTXOs for live event")?;

            let spent = storage
                .get_unshielded_utxos_spent_in_transaction_for_address(&domain_address, transaction_id)
                .await
                .internal("fetch spent UTXOs for live event")?;

            let (highest_index, _) = storage
                .get_highest_indices_for_address(&domain_address)
                .await
                .internal("fetch highest indices for address in live event")?;
            let highest_index = highest_index.unwrap_or(0);
            let current_index = tx.end_index;

            let progress = UnshieldedProgress {
                highest_index,
                current_index,
            };

            // Track this transaction as processed.
            processed_transaction_ids.insert(transaction_id);

            yield UnshieldedUtxoEvent {
                progress,
                transaction: Some(tx.into()),
                created_utxos: Some(created.into_iter()
                    .map(|utxo| UnshieldedUtxo::<S>::from((utxo, network_id)))
                    .collect()),
                spent_utxos: Some(spent.into_iter()
                    .map(|utxo| UnshieldedUtxo::<S>::from((utxo, network_id)))
                    .collect()),
            };
        }

        warn!("stream of UnshieldedUtxoIndexed events completed unexpectedly");
    };

    Ok(updates)
}

async fn progress_updates<'a, S>(
    cx: &'a Context<'a>,
    address: ByteVec,
) -> async_graphql::Result<
    impl Stream<Item = async_graphql::Result<UnshieldedUtxoEvent<S>>> + use<'a, S>,
>
where
    S: Storage,
{
    let storage = cx.get_storage::<S>();

    let intervals = IntervalStream::new(interval(PROGRESS_UPDATES_INTERVAL));
    let updates = intervals.then(move |_| progress_update(address.clone(), storage));

    Ok(updates)
}

async fn progress_update<S>(
    address: ByteVec,
    storage: &S,
) -> async_graphql::Result<UnshieldedUtxoEvent<S>>
where
    S: Storage,
{
    // Calculate progress information
    let (highest_index, current_index) = storage
        .get_highest_indices_for_address(&address)
        .await
        .internal("fetch highest indices for address")?;

    let highest_index = highest_index.unwrap_or(0);
    let current_index = current_index.unwrap_or(0);

    let progress = UnshieldedProgress {
        highest_index,
        current_index,
    };

    Ok(UnshieldedUtxoEvent {
        progress,
        transaction: None,
        created_utxos: None,
        spent_utxos: None,
    })
}
