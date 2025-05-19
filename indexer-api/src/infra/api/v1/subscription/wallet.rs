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
    domain::{HexEncoded, Storage, Transaction, ZswapStateCache},
    infra::api::{
        v1::{ProgressUpdate, ViewingUpdate, WalletSyncEvent, ZswapChainStateUpdate},
        ContextExt,
    },
};
use async_graphql::{async_stream::try_stream, Context, Subscription};
use futures::{
    future::ok,
    stream::{self, TryStreamExt},
    Stream, StreamExt,
};
use indexer_common::{
    domain::{NetworkId, SessionId, Subscriber, WalletIndexed, ZswapStateStorage},
    error::StdErrorExt,
};
use log::{debug, error};
use std::{future::ready, marker::PhantomData, num::NonZeroU32, pin::pin, time::Duration};
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
    Z: ZswapStateStorage,
{
    /// Subscribe to wallet updates.
    pub async fn wallet<'a>(
        &self,
        cx: &'a Context<'a>,
        session_id: HexEncoded,
        index: Option<u64>,
        send_progress_updates: Option<bool>,
    ) -> async_graphql::Result<
        impl Stream<Item = async_graphql::Result<WalletSyncEvent<S>>> + use<'a, S, B, Z>,
    > {
        let session_id = session_id.hex_decode::<Vec<u8>>().map_err(|error| {
            async_graphql::Error::new(format!("cannot hex-decode session ID: {error}"))
        })?;
        let session_id = SessionId::try_from(session_id.as_slice())
            .map_err(|error| async_graphql::Error::new(format!("invalid session ID: {error}")))?;

        let index = index.unwrap_or_default();
        let send_progress_updates = send_progress_updates.unwrap_or(true);

        let viewing_updates = viewing_updates::<S, B, Z>(cx, session_id, index)
            .await?
            .boxed();

        let events = if send_progress_updates {
            let progress_updates = progress_updates::<S>(cx, session_id).await?.boxed();
            vec![viewing_updates, progress_updates]
        } else {
            vec![viewing_updates]
        };
        let events = stream::iter(events).flatten_unordered(None);
        // The wallet has unexpected expectations wrt the protocol, hence we must accomodate for
        // that.
        let events = events.scan(index, |last_viewing_update_index, event| {
            let event = event.map(|event| match event {
                WalletSyncEvent::ViewingUpdate(viewing_update) => {
                    *last_viewing_update_index = viewing_update.index - 1;

                    debug!(last_viewing_update_index:%; "emitting ViewingUpdate");

                    WalletSyncEvent::ViewingUpdate(viewing_update)
                }

                WalletSyncEvent::ProgressUpdate(ProgressUpdate { mut synced, total }) => {
                    if synced > *last_viewing_update_index {
                        synced = *last_viewing_update_index;
                    } else {
                        *last_viewing_update_index = total;
                        synced = total;
                    }

                    debug!(
                        synced:%, total, last_viewing_update_index:%; "emitting ProgressUpdate"
                    );

                    WalletSyncEvent::ProgressUpdate(ProgressUpdate { synced, total })
                }
            });

            ready(Some(event))
        });

        let storage = cx.get_storage::<S>()?;
        let set_wallet_active = IntervalStream::new(interval(ACTIVATE_WALLET_INTERVAL))
            .then(move |_| async move { storage.set_wallet_active(&session_id).await })
            .map_err(Into::into);

        let events = stream::select(events.map_ok(Some), set_wallet_active.map_ok(|_| None))
            .try_filter_map(ok);

        Ok(events)
    }
}

async fn viewing_updates<'a, S, B, Z>(
    cx: &'a Context<'a>,
    session_id: SessionId,
    from_index: u64,
) -> async_graphql::Result<
    impl Stream<Item = async_graphql::Result<WalletSyncEvent<S>>> + use<'a, S, B, Z>,
>
where
    S: Storage,
    B: Subscriber,
    Z: ZswapStateStorage,
{
    let network_id = cx.get_network_id()?;
    let storage = cx.get_storage::<S>()?;
    let subscriber = cx.get_subscriber::<B>()?;
    let zswap_state_storage = cx.get_zswap_state_storage::<Z>()?;
    let zswap_state_cache = cx.get_zswap_state_cache()?;

    let wallet_indexed_events = {
        subscriber
            .subscribe::<WalletIndexed>()
            .await
            .inspect_err(|error| {
                error!(
                    error:? = error.as_chain();
                    "cannot subscribe to WalletIndexed events"
                )
            })?
            .try_filter(move |wallet_indexed| ready(wallet_indexed.session_id == session_id))
    };

    let viewing_updates = try_stream! {
        let mut wallet_indexed_events = pin!(wallet_indexed_events);

        // First get all stored relevant `Transaction`s from the requested `from_index`.
        let transactions = storage.get_relevant_transactions(&session_id, from_index, BATCH_SIZE);
        debug!(from_index, session_id:?; "got relevant transactions");

        // Then yield all relevant `Transactions`s.
        let mut next_from_index = from_index;
        let mut transactions = pin!(transactions);
        while let Some(transaction) = transactions
            .try_next()
            .await
            .inspect_err(|error| error!(error:? = error.as_chain(); "cannot get next transaction"))?
        {
            let event = viewing_update(
                next_from_index,
                transaction,
                network_id,
                zswap_state_storage,
                zswap_state_cache,
            )
            .await?;
            if let WalletSyncEvent::ViewingUpdate(ViewingUpdate { index, .. }) = event {
                next_from_index = index;
            }

            yield event;
        }

        // Then get now stored relevant `Transaction`s after receiving a `WalletIndexed` event.
        while let Some(WalletIndexed { .. }) =
            wallet_indexed_events
                .try_next()
                .await
                .inspect_err(|error| {
                    error!(
                        error:? = error.as_chain();
                        "cannot get next WalletIndexed event"
                    )
                })?
        {
            debug!(next_from_index, session_id:?; "handling WalletIndexed event");

            let transactions =
                storage.get_relevant_transactions(&session_id, next_from_index, BATCH_SIZE);
            let mut transactions = pin!(transactions);
            while let Some(transaction) = transactions.try_next().await.inspect_err(|error| {
                error!(error:? = error.as_chain(); "cannot get next transaction")
            })? {
                let viewing_update = viewing_update(
                    next_from_index,
                    transaction,
                    network_id,
                    zswap_state_storage,
                    zswap_state_cache,
                )
                .await?;
                if let WalletSyncEvent::ViewingUpdate(ViewingUpdate { index, .. }) = viewing_update
                {
                    next_from_index = index;
                }

                yield viewing_update;
            }
        }
    };

    Ok(viewing_updates)
}

async fn viewing_update<S, Z>(
    from: u64,
    transaction: Transaction,
    network_id: NetworkId,
    zswap_state_storage: &Z,
    zswap_state_cache: &ZswapStateCache,
) -> async_graphql::Result<WalletSyncEvent<S>>
where
    S: Storage,
    Z: ZswapStateStorage,
{
    let index = transaction.end_index + 1;

    let update = if from == transaction.start_index {
        let relevant_transaction = ZswapChainStateUpdate::RelevantTransaction(transaction.into());
        vec![relevant_transaction]
    } else {
        // We calculate the collapsed update BEFORE the start index of the transaction, hence `- 1`!
        let collapsed_update = zswap_state_cache
            .collapsed_update(
                from,
                transaction.start_index - 1,
                network_id,
                transaction.protocol_version,
                zswap_state_storage,
            )
            .await
            .inspect_err(|error| {
                error!(
                    error = error.as_chain(),
                    transaction:?;
                    "cannot create collapsed update"
                )
            })?;

        vec![
            ZswapChainStateUpdate::MerkleTreeCollapsedUpdate(collapsed_update.into()),
            ZswapChainStateUpdate::RelevantTransaction(transaction.into()),
        ]
    };

    let viewing_update = WalletSyncEvent::ViewingUpdate(ViewingUpdate { index, update });

    Ok(viewing_update)
}

async fn progress_updates<'a, S>(
    cx: &'a Context<'a>,
    session_id: SessionId,
) -> async_graphql::Result<impl Stream<Item = async_graphql::Result<WalletSyncEvent<S>>> + use<'a, S>>
where
    S: Storage,
{
    let storage = cx.get_storage::<S>()?;

    let intervals = IntervalStream::new(interval(PROGRESS_UPDATES_INTERVAL));
    let updates = intervals.then(move |_| progress_update::<S>(session_id, storage));

    Ok(updates)
}

async fn progress_update<S: Storage>(
    session_id: SessionId,
    storage: &S,
) -> async_graphql::Result<WalletSyncEvent<S>> {
    let synced = storage
        .get_last_relevant_end_index_for_wallet(&session_id)
        .await?
        .unwrap_or_default();

    let total = storage
        .get_last_end_index_for_wallet(&session_id)
        .await?
        .unwrap_or_default();

    let event = WalletSyncEvent::ProgressUpdate(ProgressUpdate { synced, total });

    Ok(event)
}
