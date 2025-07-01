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
    domain::{Transaction, storage::Storage},
    infra::api::{
        ApiError, ApiResult, ContextExt, ResultExt,
        v1::unshielded::{
            UnshieldedAddress, UnshieldedProgressUpdate, UnshieldedSyncEvent, UnshieldedUtxo,
            UnshieldedUtxoUpdate,
        },
    },
};
use async_graphql::{Context, Subscription, async_stream::try_stream};
use fastrace::trace;
use futures::{Stream, StreamExt, TryStreamExt};
use indexer_common::domain::{NetworkId, RawUnshieldedAddress, Subscriber, UnshieldedUtxoIndexed};
use log::{debug, warn};
use std::{future::ready, marker::PhantomData, num::NonZeroU32, pin::pin, time::Duration};
use stream_cancel::{StreamExt as _, Trigger, Tripwire};
use tokio::time::interval;
use tokio_stream::wrappers::IntervalStream;

// TODO: Make configurable!
const BATCH_SIZE: NonZeroU32 = NonZeroU32::new(100).unwrap();

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
    /// Subscribes to unshielded synchronization events for a specific address and the given
    /// transaction ID or 0 if omitted. The events are either unshielded UTXO updates or progress
    /// updates.
    #[trace(properties = { "address": "{address:?}", "transaction_id": "{transaction_id:?}" })]
    async fn unshielded_utxos<'a>(
        &self,
        cx: &'a Context<'a>,
        address: UnshieldedAddress,
        transaction_id: Option<u64>,
    ) -> Result<impl Stream<Item = ApiResult<UnshieldedSyncEvent<S>>> + use<'a, S, B>, ApiError>
    {
        let address = address
            .try_into_domain(cx.get_network_id())
            .map_err_into_client_error(|| "invalid address")?;

        // Build a stream of WalletSyncEvents by merging ViewingUpdates and ProgressUpdates. The
        // ViewingUpdates stream should be infinite by definition (see the trait). However, if it
        // nevertheless completes, we use a Tripwire to ensure the ProgressUpdates stream also
        // completes, preventing the merged stream from hanging indefinitely waiting for both
        // streams to complete.
        let (trigger, tripwire) = Tripwire::new();

        let unshielded_utxo_updates =
            unshielded_utxo_updates::<S, B>(cx, address, transaction_id.unwrap_or(0), trigger)
                .map_ok(|update| UnshieldedSyncEvent::UnshieldedUtxoUpdate(update.into()));

        let progress_updates = progress_updates::<S>(cx, address)
            .take_until_if(tripwire)
            .map_ok(UnshieldedSyncEvent::ProgressUpdate);

        let events = tokio_stream::StreamExt::merge(unshielded_utxo_updates, progress_updates);

        Ok(events)
    }
}

#[trace(properties = { "address": "{address:?}", "transaction_id": "{transaction_id}" })]
fn unshielded_utxo_updates<'a, S, B>(
    cx: &'a Context<'a>,
    address: RawUnshieldedAddress,
    mut transaction_id: u64,
    trigger: Trigger,
) -> impl Stream<Item = ApiResult<UnshieldedUtxoUpdate<S>>> + use<'a, S, B>
where
    S: Storage,
    B: Subscriber,
{
    let network_id = cx.get_network_id();
    let storage = cx.get_storage::<S>();
    let subscriber = cx.get_subscriber::<B>();

    let utxo_indexed_events = subscriber
        .subscribe::<UnshieldedUtxoIndexed>()
        .try_filter(move |event| ready(event.address == address));

    try_stream! {
        // Stream UTXO events for existing transactions.
        debug!(address:?, transaction_id; "streaming existing UTXO events");

        let transactions =
            storage.get_transactions_involving_unshielded(address, transaction_id, BATCH_SIZE);

        let mut transactions = pin!(transactions);
        while let Some(transaction) = transactions
            .try_next()
            .await
            .map_err_into_server_error(|| format!("get next transaction for address {address}"))?
        {
            if let Some(utxo_update) = unshielded_utxo_update(
                &mut transaction_id,
                storage,
                address,
                transaction,
                network_id,
            )
            .await?
            {
                yield utxo_update;
            }
        }

        // Stream UTXO events for live transactions.
        let mut utxo_indexed_events = pin!(utxo_indexed_events);
        while utxo_indexed_events
            .try_next()
            .await
            .map_err_into_server_error(|| "get next UnshieldedUtxoIndexed event")?
            .is_some()
        {
            let transactions =
                storage.get_transactions_involving_unshielded(address, transaction_id, BATCH_SIZE);

            let mut transactions = pin!(transactions);
            while let Some(transaction) =
                transactions
                    .try_next()
                    .await
                    .map_err_into_server_error(|| {
                        format!("get next transaction for address {address}")
                    })?
            {
                if let Some(utxo_update) = unshielded_utxo_update(
                    &mut transaction_id,
                    storage,
                    address,
                    transaction,
                    network_id,
                )
                .await?
                {
                    yield utxo_update;
                }
            }
        }

        warn!("stream of UnshieldedUtxoIndexed events completed unexpectedly");
        trigger.cancel();
    }
}

async fn unshielded_utxo_update<S>(
    transaction_id: &mut u64,
    storage: &S,
    address: RawUnshieldedAddress,
    transaction: Transaction,
    network_id: NetworkId,
) -> ApiResult<Option<UnshieldedUtxoUpdate<S>>>
where
    S: Storage,
{
    *transaction_id = transaction.id;

    let created = storage
        .get_unshielded_utxos_created_in_transaction_for_address(address, transaction.id)
        .await
        .map_err_into_server_error(|| {
            format!(
                "get created UTXOs for existing transaction with ID {}",
                transaction.id
            )
        })?;

    let spent = storage
        .get_unshielded_utxos_spent_in_transaction_for_address(address, transaction.id)
        .await
        .map_err_into_server_error(|| {
            format!(
                "get spent UTXOs for existing transaction with ID {}",
                transaction.id
            )
        })?;

    // Only emit events for transactions that actually have UTXOs for this address.
    let unshielded_utxo_update = (!created.is_empty() || !spent.is_empty()).then(|| {
        let created_utxos = created
            .into_iter()
            .map(|utxo| UnshieldedUtxo::<S>::from((utxo, network_id)))
            .collect();

        let spent_utxos = spent
            .into_iter()
            .map(|utxo| UnshieldedUtxo::<S>::from((utxo, network_id)))
            .collect();

        UnshieldedUtxoUpdate {
            transaction: transaction.into(),
            created_utxos,
            spent_utxos,
        }
    });

    Ok(unshielded_utxo_update)
}

fn progress_updates<'a, S>(
    cx: &'a Context<'a>,
    address: RawUnshieldedAddress,
) -> impl Stream<Item = ApiResult<UnshieldedProgressUpdate>> + use<'a, S>
where
    S: Storage,
{
    let intervals = IntervalStream::new(interval(PROGRESS_UPDATES_INTERVAL));
    intervals.then(move |_| progress_update(address, cx.get_storage::<S>()))
}

async fn progress_update<S>(
    address: RawUnshieldedAddress,
    storage: &S,
) -> ApiResult<UnshieldedProgressUpdate>
where
    S: Storage,
{
    // Calculate progress information using transaction IDs.
    let highest_transaction_id = storage
        .get_highest_transaction_id_for_unshielded_address(address)
        .await
        .map_err_into_server_error(|| "get highest transaction ID for address")?;

    let highest_transaction_id = highest_transaction_id.unwrap_or(0);

    Ok(UnshieldedProgressUpdate {
        highest_transaction_id,
    })
}
