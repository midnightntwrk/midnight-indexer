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
    domain::{LedgerStateCache, Transaction, storage::Storage},
    infra::api::{
        ApiError, ApiResult, ContextExt, HexEncoded, InnerApiError, ResultExt,
        v1::{
            decode_session_id,
            wallet::{ViewingUpdate, WalletProgressUpdate, WalletSyncEvent, ZswapChainStateUpdate},
        },
    },
};
use async_graphql::{Context, Subscription, async_stream::try_stream};
use drop_stream::DropStreamExt;
use fastrace::trace;
use futures::{
    Stream, StreamExt,
    future::ok,
    stream::{self, TryStreamExt},
};
use indexer_common::domain::{
    LedgerStateStorage, NetworkId, SessionId, Subscriber, TransactionResult, WalletIndexed,
};
use log::{debug, warn};
use std::{
    future::ready, marker::PhantomData, num::NonZeroU32, pin::pin, sync::Arc, time::Duration,
};
use stream_cancel::{StreamExt as _, Trigger, Tripwire};
use tokio::time::interval;
use tokio_stream::wrappers::IntervalStream;

// TODO: Make configurable!
const BATCH_SIZE: NonZeroU32 = NonZeroU32::new(100).unwrap();

// TODO: Make configurable!
const PROGRESS_UPDATES_INTERVAL: Duration = Duration::from_secs(3);

// TODO: Make configurable!
const ACTIVATE_WALLET_INTERVAL: Duration = Duration::from_secs(60);

pub struct WalletSubscription<S, B, Z> {
    _s: PhantomData<S>,
    _b: PhantomData<B>,
    _z: PhantomData<Z>,
}

impl<S, B, Z> Default for WalletSubscription<S, B, Z> {
    fn default() -> Self {
        Self {
            _s: PhantomData,
            _b: PhantomData,
            _z: PhantomData,
        }
    }
}

#[Subscription]
impl<S, B, Z> WalletSubscription<S, B, Z>
where
    S: Storage,
    B: Subscriber,
    Z: LedgerStateStorage,
{
    /// Subscribe to wallet synchronization events for the given session ID starting at the given
    /// index or at zero if omitted. The events are either viewing updates or progress updates.
    #[trace(properties = { "session_id": "{session_id:?}", "index": "{index:?}" })]
    pub async fn wallet<'a>(
        &self,
        cx: &'a Context<'a>,
        session_id: HexEncoded,
        index: Option<u64>,
        send_progress_updates: Option<bool>,
    ) -> Result<impl Stream<Item = ApiResult<WalletSyncEvent<S>>> + use<'a, S, B, Z>, ApiError>
    {
        cx.get_metrics().wallets_connected.increment(1);
        debug!(session_id:%; "wallet subscription started");

        let session_id =
            decode_session_id(session_id).map_err_into_client_error(|| "invalid session ID")?;
        let index = index.unwrap_or_default();
        let send_progress_updates = send_progress_updates.unwrap_or(true);

        // Build a stream of WalletSyncEvents by merging ViewingUpdates and ProgressUpdates. The
        // ViewingUpdates stream should be infinite by definition (see the trait). However, if it
        // nevertheless completes, we use a Tripwire to ensure the ProgressUpdates stream also
        // completes, preventing the merged stream from hanging indefinitely waiting for both
        // streams to complete.
        let (trigger, tripwire) = Tripwire::new();

        let viewing_updates = viewing_updates::<S, B, Z>(cx, session_id, index, trigger)
            .map_ok(WalletSyncEvent::ViewingUpdate);

        let progress_updates = if send_progress_updates {
            progress_updates::<S>(cx, session_id)
                .take_until_if(tripwire)
                .map_ok(WalletSyncEvent::ProgressUpdate)
                .boxed()
        } else {
            stream::empty().boxed()
        };

        let events = tokio_stream::StreamExt::merge(viewing_updates, progress_updates);

        // As long as the subscription is alive, the wallet is periodically set active, even if
        // there are no new transactions.
        let storage = cx.get_storage::<S>();
        let set_wallet_active = IntervalStream::new(interval(ACTIVATE_WALLET_INTERVAL))
            .then(move |_| async move { storage.set_wallet_active(session_id).await })
            .map_err(|error| {
                ApiError::Server(InnerApiError(
                    "set wallet active".to_string(),
                    Some(Arc::new(error)),
                ))
            });
        let events = stream::select(events.map_ok(Some), set_wallet_active.map_ok(|_| None))
            .try_filter_map(ok)
            .on_drop(move || {
                cx.get_metrics().wallets_connected.decrement(1);
                debug!(session_id:%; "wallet subscription ended");
            });

        Ok(events)
    }
}

#[trace(properties = { "session_id": "{session_id:?}", "index": "{index}" })]
fn viewing_updates<'a, S, B, Z>(
    cx: &'a Context<'a>,
    session_id: SessionId,
    mut index: u64,
    trigger: Trigger,
) -> impl Stream<Item = ApiResult<ViewingUpdate<S>>> + use<'a, S, B, Z>
where
    S: Storage,
    B: Subscriber,
    Z: LedgerStateStorage,
{
    let network_id = cx.get_network_id();
    let storage = cx.get_storage::<S>();
    let subscriber = cx.get_subscriber::<B>();
    let ledger_state_storage = cx.get_ledger_state_storage::<Z>();
    let zswap_state_cache = cx.get_ledger_state_cache();

    let wallet_indexed_events = subscriber
        .subscribe::<WalletIndexed>()
        .try_filter(move |wallet_indexed| ready(wallet_indexed.session_id == session_id));

    try_stream! {
        // Stream exiting transactions.
        debug!(session_id:%, index; "streaming existing transactions");

        let transactions = storage.get_relevant_transactions(session_id, index, BATCH_SIZE);
        let mut transactions = pin!(transactions);
        while let Some(transaction) = transactions
            .try_next()
            .await
            .map_err_into_server_error(|| "get next transaction")?
        {
            let viewing_update = viewing_update(
                index,
                transaction,
                ledger_state_storage,
                zswap_state_cache,
                network_id,
            )
            .await?;

            index = viewing_update.index;

            yield viewing_update;
        }

        // Stream live transactions.
        let mut wallet_indexed_events = pin!(wallet_indexed_events);
        while wallet_indexed_events
            .try_next()
            .await
            .map_err_into_server_error(|| "get next WalletIndexed event")?
            .is_some()
        {
            debug!(index; "streaming next transactions");

            let transactions =
                storage.get_relevant_transactions(session_id, index, BATCH_SIZE);
            let mut transactions = pin!(transactions);

            while let Some(transaction) = transactions
                .try_next()
                .await
                .map_err_into_server_error(|| "get next transaction")?
            {
                let viewing_update = viewing_update(
                    index,
                    transaction,
                    ledger_state_storage,
                    zswap_state_cache,
                    network_id,
                )
                .await?;

                index = viewing_update.index;

                yield viewing_update;
            }
        }

        warn!("stream of WalletIndexed events completed unexpectedly");
        trigger.cancel();
    }
}

#[trace(properties = { "from": "{from:?}" })]
async fn viewing_update<S, Z>(
    from: u64,
    transaction: Transaction,
    ledger_state_storage: &Z,
    zswap_state_cache: &LedgerStateCache,
    network_id: NetworkId,
) -> ApiResult<ViewingUpdate<S>>
where
    S: Storage,
    Z: LedgerStateStorage,
{
    // For failures, don't increment the index, because no changes were applied to the zswap state.
    // Put another way: the next transaction will have the same start_index like this end index.
    // This avoids "update with end before start" errors when calling `collapsed_update`.
    let index = if transaction.transaction_result == TransactionResult::Failure {
        transaction.end_index
    } else {
        transaction.end_index + 1
    };

    let update = if from == transaction.start_index {
        let relevant_transaction = ZswapChainStateUpdate::RelevantTransaction(transaction.into());
        vec![relevant_transaction]
    } else {
        // We calculate the collapsed update BEFORE the start index of the transaction, hence `- 1`!
        let collapsed_update = zswap_state_cache
            .collapsed_update(
                from,
                transaction.start_index - 1,
                ledger_state_storage,
                network_id,
                transaction.protocol_version,
            )
            .await
            .map_err_into_server_error(|| "create collapsed update")?;

        vec![
            ZswapChainStateUpdate::MerkleTreeCollapsedUpdate(collapsed_update.into()),
            ZswapChainStateUpdate::RelevantTransaction(transaction.into()),
        ]
    };

    let viewing_update = ViewingUpdate { index, update };
    debug!(viewing_update:?; "built viewing update");

    Ok(viewing_update)
}

fn progress_updates<'a, S>(
    cx: &'a Context<'a>,
    session_id: SessionId,
) -> impl Stream<Item = ApiResult<WalletProgressUpdate>> + use<'a, S>
where
    S: Storage,
{
    let intervals = IntervalStream::new(interval(PROGRESS_UPDATES_INTERVAL));
    intervals.then(move |_| progress_update(session_id, cx.get_storage::<S>()))
}

async fn progress_update<S>(session_id: SessionId, storage: &S) -> ApiResult<WalletProgressUpdate>
where
    S: Storage,
{
    let (highest_index, highest_relevant_index, highest_relevant_wallet_index) = storage
        .get_highest_end_indices(session_id)
        .await
        .map_err_into_server_error(|| "get highest indices")?;

    let highest_index = highest_index.unwrap_or_default();
    let highest_relevant_index = highest_relevant_index.unwrap_or_default();
    let highest_relevant_wallet_index = highest_relevant_wallet_index.unwrap_or_default();

    Ok(WalletProgressUpdate {
        highest_index,
        highest_relevant_index,
        highest_relevant_wallet_index,
    })
}
