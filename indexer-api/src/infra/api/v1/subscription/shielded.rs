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
    domain::{self, LedgerStateCache, storage::Storage},
    infra::api::{
        ApiError, ApiResult, ContextExt, ResultExt,
        v1::{
            AsBytesExt, HexEncoded, decode_session_id, subscription::get_next_transaction,
            transaction::Transaction,
        },
    },
};
use async_graphql::{Context, SimpleObject, Subscription, Union, async_stream::try_stream};
use derive_more::Debug;
use drop_stream::DropStreamExt;
use fastrace::trace;
use futures::{
    Stream, StreamExt,
    future::ok,
    stream::{self, TryStreamExt},
};
use indexer_common::domain::{LedgerStateStorage, SessionId, Subscriber, WalletIndexed};
use log::{debug, warn};
use std::{future::ready, marker::PhantomData, num::NonZeroU32, pin::pin, time::Duration};
use stream_cancel::{StreamExt as _, Trigger, Tripwire};
use tokio::time::interval;
use tokio_stream::wrappers::IntervalStream;

// TODO: Make configurable.
const BATCH_SIZE: NonZeroU32 = NonZeroU32::new(100).unwrap();

// TODO: Make configurable.
const PROGRESS_UPDATES_INTERVAL: Duration = Duration::from_secs(3);

// TODO: Make configurable.
const ACTIVATE_WALLET_INTERVAL: Duration = Duration::from_secs(60);

/// An event of the shielded transactions subscription.
#[derive(Debug, Union)]
pub enum ShieldedTransactionsEvent<S: Storage> {
    ViewingUpdate(ViewingUpdate<S>),
    ShieldedTransactionsProgress(ShieldedTransactionsProgress),
}

/// Aggregates a relevant transaction with the next start index and an optional collapsed
/// Merkle-Tree update.
#[derive(Debug, SimpleObject)]
pub struct ViewingUpdate<S: Storage> {
    /// Next start index into the zswap state to be queried. Usually the end index of the included
    /// relevant transaction plus one unless that is a failure in which case just its end
    /// index.
    pub index: u64,

    /// Relevant transaction for the wallet and maybe a collapsed Merkle-Tree update.
    pub update: Vec<ZswapChainStateUpdate<S>>,
}

/// Aggregates information about the shielded transactions indexing progress.
#[derive(Debug, SimpleObject)]
pub struct ShieldedTransactionsProgress {
    /// The highest end index into the zswap state of all currently known transactions.
    pub highest_index: u64,

    /// The highest end index into the zswap state of all currently known relevant transactions,
    /// i.e. those that belong to any known wallet. Less or equal `highest_index`.
    pub highest_relevant_index: u64,

    /// The highest end index into the zswap state of all currently known relevant transactions for
    /// a particular wallet. Less or equal `highest_relevant_index`.
    pub highest_relevant_wallet_index: u64,
}

#[derive(Debug, Union)]
#[allow(clippy::large_enum_variant)]
pub enum ZswapChainStateUpdate<S: Storage> {
    MerkleTreeCollapsedUpdate(MerkleTreeCollapsedUpdate),
    RelevantTransaction(RelevantTransaction<S>),
}

#[derive(Debug, SimpleObject)]
pub struct MerkleTreeCollapsedUpdate {
    /// The start index into the zswap state.
    start: u64,

    /// The end index into the zswap state.
    end: u64,

    /// The hex-encoded merkle-tree collapsed update.
    #[debug(skip)]
    update: HexEncoded,

    /// The protocol version.
    protocol_version: u32,
}

impl From<domain::MerkleTreeCollapsedUpdate> for MerkleTreeCollapsedUpdate {
    fn from(value: domain::MerkleTreeCollapsedUpdate) -> Self {
        let domain::MerkleTreeCollapsedUpdate {
            start_index,
            end_index,
            update,
            protocol_version,
        } = value;

        Self {
            start: start_index,
            end: end_index,
            update: update.hex_encode(),
            protocol_version: protocol_version.0,
        }
    }
}

#[derive(Debug, SimpleObject)]
pub struct RelevantTransaction<S: Storage> {
    /// Relevant transaction for the wallet.
    transaction: Transaction<S>,

    /// The start index.
    start: u64,

    /// The end index.
    end: u64,
}

impl<S> From<domain::Transaction> for RelevantTransaction<S>
where
    S: Storage,
{
    fn from(transaction: domain::Transaction) -> Self {
        Self {
            start: transaction.start_index,
            end: transaction.end_index.saturating_sub(1), // Domain end index is exclusive!
            transaction: transaction.into(),
        }
    }
}

pub struct ShieldedTransactionsSubscription<S, B, Z> {
    _s: PhantomData<S>,
    _b: PhantomData<B>,
    _z: PhantomData<Z>,
}

impl<S, B, Z> Default for ShieldedTransactionsSubscription<S, B, Z> {
    fn default() -> Self {
        Self {
            _s: PhantomData,
            _b: PhantomData,
            _z: PhantomData,
        }
    }
}

#[Subscription]
impl<S, B, Z> ShieldedTransactionsSubscription<S, B, Z>
where
    S: Storage,
    B: Subscriber,
    Z: LedgerStateStorage,
{
    /// Subscribe shielded transaction events for the given session ID starting at the given index
    /// or at zero if omitted.
    pub async fn shielded_transactions<'a>(
        &self,
        cx: &'a Context<'a>,
        session_id: HexEncoded,
        index: Option<u64>,
        send_progress_updates: Option<bool>,
    ) -> Result<
        impl Stream<Item = ApiResult<ShieldedTransactionsEvent<S>>> + use<'a, S, B, Z>,
        ApiError,
    > {
        cx.get_metrics().wallets_connected.increment(1);

        let session_id =
            decode_session_id(session_id).map_err_into_client_error(|| "invalid session ID")?;
        let index = index.unwrap_or_default();
        let send_progress_updates = send_progress_updates.unwrap_or(true);

        // Build a stream of shielded transaction events by merging ViewingUpdates and
        // ProgressUpdates. The ViewingUpdates stream should be infinite by definition (see
        // the trait). However, if it nevertheless completes, we use a Tripwire to ensure
        // the ProgressUpdates stream also completes, preventing the merged stream from
        // hanging indefinitely waiting for both streams to complete.
        let (trigger, tripwire) = Tripwire::new();

        let viewing_updates = make_viewing_updates::<S, B, Z>(cx, session_id, index, trigger)
            .map_ok(ShieldedTransactionsEvent::ViewingUpdate);

        let progress_updates = if send_progress_updates {
            make_progress_updates::<S>(cx, session_id)
                .take_until_if(tripwire)
                .map_ok(ShieldedTransactionsEvent::ShieldedTransactionsProgress)
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
            .map(|item| item.map_err_into_server_error(|| "set wallet active"));
        let events = stream::select(events.map_ok(Some), set_wallet_active.map_ok(|_| None))
            .try_filter_map(ok)
            .on_drop(move || {
                cx.get_metrics().wallets_connected.decrement(1);
                debug!(session_id:%; "shielded transaction subscription ended");
            });

        Ok(events)
    }
}

fn make_viewing_updates<'a, S, B, Z>(
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
        while let Some(transaction) = get_next_transaction(&mut transactions)
            .await
            .map_err_into_server_error(|| "get next transaction")?
        {
            let viewing_update = make_viewing_update(
                index,
                transaction,
                ledger_state_storage,
                zswap_state_cache,
            )
            .await?;

            index = viewing_update.index;

            yield viewing_update;
        }

        // Stream live transactions.
        debug!(session_id:%, index; "streaming live transactions");
        let mut wallet_indexed_events = pin!(wallet_indexed_events);
        while wallet_indexed_events
            .try_next()
            .await
            .map_err_into_server_error(|| "get next WalletIndexed event")?
            .is_some()
        {
            debug!(index; "streaming next live transactions");

            let transactions =
                storage.get_relevant_transactions(session_id, index, BATCH_SIZE);
            let mut transactions = pin!(transactions);
            while let Some(transaction) =  get_next_transaction(&mut transactions)
                .await
                .map_err_into_server_error(|| "get next transaction")?
            {
                let viewing_update = make_viewing_update(
                    index,
                    transaction,
                    ledger_state_storage,
                    zswap_state_cache,
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
async fn make_viewing_update<S, Z>(
    from: u64,
    transaction: domain::Transaction,
    ledger_state_storage: &Z,
    zswap_state_cache: &LedgerStateCache,
) -> ApiResult<ViewingUpdate<S>>
where
    S: Storage,
    Z: LedgerStateStorage,
{
    debug!(from, transaction:?; "making viewing update");

    let index = transaction.end_index;

    let update = if from == transaction.start_index || transaction.start_index == 0 {
        let relevant_transaction = ZswapChainStateUpdate::RelevantTransaction(transaction.into());
        vec![relevant_transaction]
    } else {
        // We calculate the collapsed update BEFORE the start index of the transaction, hence `- 1`!
        let collapsed_update = zswap_state_cache
            .collapsed_update(
                from,
                transaction.start_index - 1,
                ledger_state_storage,
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
    debug!(viewing_update:?; "made viewing update");

    Ok(viewing_update)
}

fn make_progress_updates<'a, S>(
    cx: &'a Context<'a>,
    session_id: SessionId,
) -> impl Stream<Item = ApiResult<ShieldedTransactionsProgress>> + use<'a, S>
where
    S: Storage,
{
    let intervals = IntervalStream::new(interval(PROGRESS_UPDATES_INTERVAL));
    intervals.then(move |_| make_progress_update(session_id, cx.get_storage::<S>()))
}

async fn make_progress_update<S>(
    session_id: SessionId,
    storage: &S,
) -> ApiResult<ShieldedTransactionsProgress>
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

    Ok(ShieldedTransactionsProgress {
        highest_index,
        highest_relevant_index,
        highest_relevant_wallet_index,
    })
}
