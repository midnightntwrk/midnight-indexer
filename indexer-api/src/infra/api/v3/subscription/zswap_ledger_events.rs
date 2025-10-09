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
    domain::{LedgerEvent, storage::Storage},
    infra::api::{ApiResult, ContextExt, ResultExt, v3::ledger_events::ZswapLedgerEvent},
};
use async_graphql::{Context, Subscription};
use async_stream::try_stream;
use fastrace::{Span, future::FutureExt, prelude::SpanContext};
use futures::{Stream, TryStreamExt};
use indexer_common::domain::{BlockIndexed, LedgerEventGrouping, Subscriber};
use log::{debug, warn};
use std::{marker::PhantomData, num::NonZeroU32, pin::pin};

// TODO: Make configurable!
const BATCH_SIZE: NonZeroU32 = NonZeroU32::new(100).unwrap();

pub struct ZswapLedgerEventsSubscription<S, B> {
    _s: PhantomData<S>,
    _b: PhantomData<B>,
}

impl<S, B> Default for ZswapLedgerEventsSubscription<S, B> {
    fn default() -> Self {
        Self {
            _s: PhantomData,
            _b: PhantomData,
        }
    }
}

#[Subscription]
impl<S, B> ZswapLedgerEventsSubscription<S, B>
where
    S: Storage,
    B: Subscriber,
{
    /// Subscribe to zswap ledger events starting at the given ID or at the very start if omitted.
    async fn zswap_ledger_events<'a>(
        &self,
        cx: &'a Context<'a>,
        id: Option<u64>,
    ) -> impl Stream<Item = ApiResult<ZswapLedgerEvent>> {
        let mut id = id.unwrap_or(1);
        let storage = cx.get_storage::<S>();
        let subscriber = cx.get_subscriber::<B>();

        let block_indexed_stream = subscriber.subscribe::<BlockIndexed>();

        try_stream! {
            debug!(id; "streaming existing events");

            let ledger_events = storage.get_ledger_events(LedgerEventGrouping::Zswap, id, BATCH_SIZE).await;
            let mut ledger_events = pin!(ledger_events);
            while let Some(ledger_event) = get_next_ledger_event(&mut ledger_events)
                .await
                .map_err_into_server_error(|| format!("get next ledger event at id {id}"))?
            {
                id = ledger_event.id + 1;
                yield ledger_event.into();
            }

            debug!(id; "streaming live events");
            let mut block_indexed_stream = pin!(block_indexed_stream);
            while block_indexed_stream
                .try_next()
                .await
                .map_err_into_server_error(|| "get next BlockIndexed event")?
                .is_some()
            {
                debug!(id; "streaming next events");

                let ledger_events = storage.get_ledger_events(LedgerEventGrouping::Zswap, id, BATCH_SIZE).await;
                let mut ledger_events = pin!(ledger_events);
                while let Some(ledger_event) = get_next_ledger_event(&mut ledger_events)
                    .await
                    .map_err_into_server_error(|| format!("get next ledger event at id {id}"))?
                {
                    id = ledger_event.id + 1;
                    yield ledger_event.into();
                }
            }

            warn!("stream of BlockIndexed events completed unexpectedly");
        }
    }
}

async fn get_next_ledger_event<E>(
    ledger_events: &mut (impl Stream<Item = Result<LedgerEvent, E>> + Unpin),
) -> Result<Option<LedgerEvent>, E> {
    ledger_events
        .try_next()
        .in_span(Span::root(
            "subscription.zswap-ledger-events.get-next-ledger-event",
            SpanContext::random(),
        ))
        .await
}
