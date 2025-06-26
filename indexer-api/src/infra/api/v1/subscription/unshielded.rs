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
        ContextExt, ResultExt, UnshieldedAddressResultExt,
        v1::{UnshieldedAddress, UnshieldedProgress, UnshieldedUtxo, UnshieldedUtxoEvent},
    },
};
use async_graphql::{Context, Subscription, async_stream::try_stream};
use fastrace::trace;
use futures::{Stream, StreamExt, TryStreamExt};
use indexer_common::domain::{ByteVec, Subscriber, UnshieldedUtxoIndexed};
use log::{debug, warn};
use std::{collections::HashSet, future::ready, marker::PhantomData, pin::pin, time::Duration};
use tokio::time::interval;
use tokio_stream::wrappers::IntervalStream;

// TODO: Make configurable!
const PROGRESS_UPDATES_INTERVAL: Duration = Duration::from_secs(30);

/// Same skeleton pattern as block / contract / wallet subscriptions.
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
    ///
    /// Emits events whenever unshielded UTXOs are created or spent for the given address.
    /// Each event includes the transaction details and lists of created/spent UTXOs.
    ///
    /// # Arguments
    /// * `address` - The unshielded address to monitor (must be in Bech32m format).
    /// * `transaction_id` - Optional transaction ID to start replay from (defaults to genesis if
    ///   omitted).
    ///
    /// # Returns
    /// A stream of `UnshieldedUtxoEvent`s containing:
    /// - `progress`: Progress information for wallet synchronization (always present).
    /// - `transaction`: The transaction that created/spent UTXOs (None for progress-only events).
    /// - `createdUtxos`: UTXOs created in this transaction for the address (None for progress-only
    ///   events).
    /// - `spentUtxos`: UTXOs spent in this transaction for the address (None for progress-only
    ///   events).
    #[trace(properties = { "address": "{address:?}", "transaction_id": "{transaction_id:?}" })]
    async fn unshielded_utxos<'a>(
        &self,
        cx: &'a Context<'a>,
        address: UnshieldedAddress,
        transaction_id: Option<u64>,
    ) -> async_graphql::Result<
        impl Stream<Item = async_graphql::Result<UnshieldedUtxoEvent<S>>> + use<'a, S, B>,
    > {
        let network_id = cx.get_network_id();
        let address = address
            .try_into_domain(network_id)
            .address_validation("convert address into domain address")?;

        // Use 0 as default to include all transactions from genesis.
        // Since transaction IDs start from 1 (BIGSERIAL/AUTOINCREMENT), using >= 0
        // ensures we include the genesis transaction (ID 1) and all subsequent ones.
        let from_transaction_id = transaction_id.unwrap_or(0);
        let utxo_events = utxo_updates::<S, B>(cx, address.clone(), from_transaction_id).await?;

        let progress_updates = progress_updates::<S>(cx, address.clone()).await?;

        let events = tokio_stream::StreamExt::merge(utxo_events, progress_updates);

        Ok(events)
    }
}

#[trace(properties = { "address": "{address:?}", "from_transaction_id": "{from_transaction_id}" })]
async fn utxo_updates<'a, S, B>(
    cx: &'a Context<'a>,
    address: ByteVec,
    from_transaction_id: u64,
) -> async_graphql::Result<
    impl Stream<Item = async_graphql::Result<UnshieldedUtxoEvent<S>>> + use<'a, S, B>,
>
where
    S: Storage,
    B: Subscriber,
{
    let network_id = cx.get_network_id();
    let storage = cx.get_storage::<S>();
    let subscriber = cx.get_subscriber::<B>();

    let utxo_indexed_events = {
        let address = address.clone();

        subscriber
            .subscribe::<UnshieldedUtxoIndexed>()
            .try_filter(move |event| ready(event.address == address))
    };

    let utxo_updates = try_stream! {
        debug!(
            address:?,
            from_transaction_id;
            "starting unshielded subscription with historical replay"
        );

        // Phase 1: Replay all historical transactions for this address.
        let historical_transactions = storage
            .get_transactions_involving_unshielded(&address, from_transaction_id)
            .await
            .internal("fetch historical transactions for address")?;

        let mut processed_transaction_ids = HashSet::new();

        for transaction in historical_transactions {
            processed_transaction_ids.insert(transaction.id);

            debug!(
                address:?,
                transaction_id = transaction.id;
                "processing historical transaction"
            );

            let created = storage
                .get_unshielded_utxos_created_in_transaction_for_address(&address, transaction.id)
                .await
                .internal("fetch created UTXOs for historical transaction")?;

            let spent = storage
                .get_unshielded_utxos_spent_in_transaction_for_address(&address, transaction.id)
                .await
                .internal("fetch spent UTXOs for historical transaction")?;

            // Only emit events for transactions that actually have UTXOs for this address.
            if !created.is_empty() || !spent.is_empty() {
                let (_, highest_transaction_id) = storage
                    .get_highest_indices_for_address(&address)
                    .await
                    .internal("fetch highest indices for address")?;
                let highest_transaction_id = highest_transaction_id.unwrap_or(0);

                let progress = UnshieldedProgress {
                    highest_transaction_id,
                    current_transaction_id: transaction.id,
                };

                let created_utxos = Some(created.into_iter()
                    .map(|utxo| UnshieldedUtxo::<S>::from((utxo, network_id)))
                    .collect());
                let spent_utxos = Some(spent.into_iter()
                    .map(|utxo| UnshieldedUtxo::<S>::from((utxo, network_id)))
                    .collect());

                yield UnshieldedUtxoEvent {
                    progress,
                    transaction: Some(transaction.into()),
                    created_utxos,
                    spent_utxos,
                };
            }
        }

        // Phase 2: Stream live events, skipping any already processed.
        let mut utxo_indexed_events = pin!(utxo_indexed_events);
        while let Some(UnshieldedUtxoIndexed { transaction_id, .. }) = utxo_indexed_events
            .try_next()
            .await
            .internal("get next UnshieldedUtxoIndexed event")?
        {
            // Skip transactions we already processed in the historical phase.
            if processed_transaction_ids.contains(&transaction_id) {
                continue;
            }

            debug!(
                address:?,
                transaction_id;
                "handling live UnshieldedUtxoIndexed event"
            );

            let transaction = storage
                .get_transaction_by_id(transaction_id)
                .await
                .internal("fetch transaction for live subscription event")?;

            let transaction = match transaction {
                Some(transaction) => transaction,

                None => {
                    warn!(transaction_id; "transaction not found, skipping live event");
                    continue;
                }
            };

            let created = storage
                .get_unshielded_utxos_created_in_transaction_for_address(&address, transaction_id)
                .await
                .internal("fetch created UTXOs for live event")?;

            let spent = storage
                .get_unshielded_utxos_spent_in_transaction_for_address(&address, transaction_id)
                .await
                .internal("fetch spent UTXOs for live event")?;

            // Only emit events for transactions that actually have UTXOs for this address.
            if !created.is_empty() || !spent.is_empty() {
                let (_, highest_transaction_id) = storage
                    .get_highest_indices_for_address(&address)
                    .await
                    .internal("fetch highest indices for address")?;
                let highest_transaction_id = highest_transaction_id.unwrap_or(0);

                let progress = UnshieldedProgress {
                    highest_transaction_id,
                    current_transaction_id: transaction.id,
                };

                let created_utxos = Some(created.into_iter()
                    .map(|utxo| UnshieldedUtxo::<S>::from((utxo, network_id)))
                    .collect());
                let spent_utxos = Some(spent.into_iter()
                    .map(|utxo| UnshieldedUtxo::<S>::from((utxo, network_id)))
                    .collect());

                yield UnshieldedUtxoEvent {
                    progress,
                    transaction: Some(transaction.into()),
                    created_utxos,
                    spent_utxos,
                };
            }
        }

        warn!("stream of UnshieldedUtxoIndexed events completed unexpectedly");
    };

    Ok(utxo_updates)
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
    // Calculate progress information using transaction IDs.
    let (_, highest_transaction_id) = storage
        .get_highest_indices_for_address(&address)
        .await
        .internal("fetch highest indices for address")?;

    let highest_transaction_id = highest_transaction_id.unwrap_or(0);
    let current_transaction_id = highest_transaction_id; // For progress-only events, current = highest.

    let progress = UnshieldedProgress {
        highest_transaction_id,
        current_transaction_id,
    };

    Ok(UnshieldedUtxoEvent {
        progress,
        transaction: None,
        created_utxos: None,
        spent_utxos: None,
    })
}
