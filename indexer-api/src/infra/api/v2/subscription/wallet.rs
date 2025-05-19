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

/**
    The wallet subscription is a stream of relevant transactions for the wallet.
    A relevant transaction is a transaction with its zswap start and end indices.
    However, only a fraction of all transactions are relevant for a specific wallet (session-id).
    Therefore, the stream of relevant transactions can contain gaps between the previous end
    index and the current start index (start > end). To fill this gap, Merkle tree collapsed
    updates of the zswap state are added to the stream (previous end => current start) before the
    relevant transaction gets sent.
    See the GraphQL schema for details:
    https://github.com/input-output-hk/midnight-pubsub-indexer/blob/main/api/src/main/resources/pubsub_indexer_v2.graphql

    The Scala-Indexer is used to process and send relevant transactions in batches.
    The Rust-Indexer must comply with the current GraphQL schema. However, to simplify the
    processing of wallet updates, the batches are reduced to one or two items:
        - If there is no gap between the indices, send just the relevant transaction.
        - If there is a gap, send first a MT collapsed update and then the transaction.

    The approach to stream wallet updates should be similar to the algorithm for streaming
    blocks:
        - Stream from the database all relevant transactions (and collapsed updated), starting
            at an (optional) offset provided by the client.
        - Once no relevant transactions are left in the database, wait (in a loop) for
            the WalletIndexed event. Then try to stream all new relevant transactions if there
            are any for this wallet (session-id). Then repeat.
**/
use crate::{
    domain::{HexEncoded, Storage, Transaction, ZswapStateCache},
    infra::api::{
        v2::{ViewingUpdate, WalletSyncEvent, ZswapChainStateUpdate},
        ContextExt,
    },
};
use async_graphql::{async_stream::try_stream, Context, Subscription};
use futures::{future::ready, stream::TryStreamExt, Stream, StreamExt};
use indexer_common::{
    domain::{NetworkId, SessionId, Subscriber, WalletIndexed},
    error::StdErrorExt,
};
use std::{marker::PhantomData, num::NonZeroU32, pin::pin, sync::Arc};
use tracing::{debug, error, warn};

// TODO: Make configurable!
const BATCH_SIZE: NonZeroU32 = unsafe { NonZeroU32::new_unchecked(100) };

pub struct WalletSubscription<S, B> {
    _s: PhantomData<S>,
    _b: PhantomData<B>,
}

impl<S, B> Default for WalletSubscription<S, B> {
    fn default() -> Self {
        Self {
            _s: PhantomData,
            _b: PhantomData,
        }
    }
}

#[Subscription]
impl<S: Storage, B: Subscriber> WalletSubscription<S, B> {
    /// Subscribe to wallet updates.
    pub async fn wallet<'a>(
        &self,
        cx: &'a Context<'a>,
        session_id: HexEncoded,
        index: Option<u64>,
        _send_progress_updates: Option<bool>,
    ) -> async_graphql::Result<impl Stream<Item = async_graphql::Result<WalletSyncEvent<S>>> + 'a>
    {
        let storage = cx.get_storage::<S>()?;
        let subscriber = cx.get_subscriber::<B>()?;
        let network_id = cx.get_network_id()?;
        let zswap_state_cache = cx.get_zswap_state_cache()?;
        let session_id = Arc::new(session_id.hex_decode::<SessionId>()?);

        let wallet_indexed_events = {
            let stream = subscriber
                .subscribe::<WalletIndexed>()
                .await
                .inspect_err(|error| {
                    error!(
                        error = error.as_chain(),
                        "cannot subscribe to WalletIndexed events"
                    )
                })?;

            let session_id = session_id.clone();
            stream.filter(move |wallet_indexed| {
                ready(if let Ok(wallet_indexed) = wallet_indexed {
                    wallet_indexed.session_id == *session_id
                } else {
                    true
                })
            })
        };

        let from_index = index.unwrap_or(0);

        let wallet_stream = try_stream! {
            let mut wallet_indexed_events = pin!(wallet_indexed_events);
            let mut next_from_index = from_index;

            // First get all stored relevant `Transaction`s from the requested `from_index`.
            let transactions = storage.get_relevant_transactions(&session_id, from_index, BATCH_SIZE);
            debug!(
                next_from_index,
                "got relevant transactions for wallet {:?}", session_id
            );

            // Then yield all relevant `Transactions`s.
            let mut transactions = pin!(transactions);
            while let Some(transaction) = transactions
                .try_next()
                .await
                .inspect_err(|error| error!(error = error.as_chain(), "cannot get next transaction"))?
            {
                let event = get_wallet_sync_event(
                    next_from_index,
                    transaction,
                    network_id,
                    storage,
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
                            error = error.as_chain(),
                            "cannot get next WalletIndexed event"
                        )
                    })?
            {
                debug!(
                    next_from_index,
                    "handling WalletIndexed event for wallet {:?}", session_id
                );

                let transactions =
                    storage.get_relevant_transactions(&session_id, next_from_index, BATCH_SIZE);
                let mut transactions = pin!(transactions);
                while let Some(transaction) = transactions.try_next().await.inspect_err(|error| {
                    error!(error = error.as_chain(), "cannot get next transaction")
                })? {
                    let event = get_wallet_sync_event(
                        next_from_index,
                        transaction,
                        network_id,
                        storage,
                        zswap_state_cache,
                    )
                    .await?;
                    if let WalletSyncEvent::ViewingUpdate(ViewingUpdate { index, .. }) = event {
                        next_from_index = index;
                    }

                    yield event;
                }
            }

            warn!("stream of WalletIndexed events completed unexpectedly");
        };

        Ok(wallet_stream)
    }
}

async fn get_wallet_sync_event<S: Storage>(
    from: u64,
    transaction: Transaction,
    network_id: NetworkId,
    storage: &S,
    zswap_state_cache: &ZswapStateCache,
) -> async_graphql::Result<WalletSyncEvent<S>> {
    let next_index = transaction.end_index + 1;
    let update = if from == transaction.start_index {
        vec![ZswapChainStateUpdate::RelevantTransaction(
            transaction.into(),
        )]
    } else {
        let collapsed_update = zswap_state_cache
            .trim_merkle_tree(
                from,
                transaction.start_index - 1,
                network_id,
                transaction.protocol_version,
                storage,
            )
            .await?;
        vec![
            ZswapChainStateUpdate::MerkleTreeCollapsedUpdate(collapsed_update.into()),
            ZswapChainStateUpdate::RelevantTransaction(transaction.into()),
        ]
    };

    Ok(WalletSyncEvent::ViewingUpdate(ViewingUpdate {
        index: next_index,
        update,
    }))
}
