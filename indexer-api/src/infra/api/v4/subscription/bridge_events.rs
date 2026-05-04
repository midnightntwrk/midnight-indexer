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
    domain::storage::{Storage, bridge::BridgeEventFilter},
    infra::api::{
        ApiError, ApiResult, ContextExt, ResultExt,
        v4::{
            HexEncoded,
            bridge::{BridgeBalance, BridgeEvent, BridgeEventVariant},
        },
    },
};
use async_graphql::{Context, Subscription};
use async_stream::try_stream;
use futures::{Stream, TryStreamExt};
use indexer_common::domain::{BridgeEventIndexed, Subscriber, UnshieldedAddress};
use std::{future::ready, marker::PhantomData, pin::pin};

const BACKFILL_BATCH: u64 = 100;

pub struct BridgeEventsSubscription<S, B> {
    _s: PhantomData<S>,
    _b: PhantomData<B>,
}

impl<S, B> Default for BridgeEventsSubscription<S, B> {
    fn default() -> Self {
        Self { _s: PhantomData, _b: PhantomData }
    }
}

#[Subscription]
impl<S, B> BridgeEventsSubscription<S, B>
where
    S: Storage,
    B: Subscriber,
{
    /// Subscribe to c2m-bridge pallet events.
    ///
    /// Backfills events with id > `from` then live-tails new events. Filters apply across both
    /// phases.
    async fn bridge_events<'a>(
        &self,
        cx: &'a Context<'a>,
        from: Option<u64>,
        recipient: Option<HexEncoded>,
        variant: Option<BridgeEventVariant>,
    ) -> Result<impl Stream<Item = ApiResult<BridgeEvent>> + use<'a, S, B>, ApiError> {
        let storage = cx.get_storage::<S>();
        let subscriber = cx.get_subscriber::<B>();
        let recipient = recipient
            .map(|h| h.hex_decode::<UnshieldedAddress>())
            .transpose()
            .map_err_into_client_error(|| "invalid recipient address")?;
        let variant_pallet = variant.map(Into::into);

        let stream = try_stream! {
            let mut last_id = from.unwrap_or(0);

            loop {
                let filter = BridgeEventFilter {
                    variant: variant_pallet,
                    recipient,
                    block_height_from: None,
                    block_height_to: None,
                    id_from: Some(last_id),
                };
                let events = storage
                    .get_bridge_events(&filter, 0, BACKFILL_BATCH)
                    .await
                    .map_err_into_server_error(|| "get bridge events")?;

                if events.is_empty() {
                    break;
                }
                for event in events {
                    last_id = last_id.max(event.id);
                    yield BridgeEvent::from(event);
                }
            }

            // Live tail.
            let live = subscriber
                .subscribe::<BridgeEventIndexed>()
                .try_filter(move |evt| {
                    let recipient_match = recipient
                        .map(|r| evt.event.recipient().map(|er| er.as_bytes() == r.as_ref()).unwrap_or(false))
                        .unwrap_or(true);
                    let variant_match = variant_pallet
                        .map(|v| evt.event.variant() == v)
                        .unwrap_or(true);
                    ready(recipient_match && variant_match)
                });
            let mut live = pin!(live);
            while let Some(_msg) = live.try_next().await
                .map_err_into_server_error(|| "subscribe BridgeEventIndexed")?
            {
                let filter = BridgeEventFilter {
                    variant: variant_pallet,
                    recipient,
                    block_height_from: None,
                    block_height_to: None,
                    id_from: Some(last_id),
                };
                let events = storage
                    .get_bridge_events(&filter, 0, BACKFILL_BATCH)
                    .await
                    .map_err_into_server_error(|| "get bridge events (live)")?;
                for event in events {
                    last_id = last_id.max(event.id);
                    yield BridgeEvent::from(event);
                }
            }
        };

        Ok(stream)
    }

    /// Subscribe to a recipient's bridge balance. Emits the current balance on subscribe and
    /// re-emits whenever a relevant bridge event indexes.
    async fn bridge_balance<'a>(
        &self,
        cx: &'a Context<'a>,
        address: HexEncoded,
    ) -> Result<impl Stream<Item = ApiResult<BridgeBalance>> + use<'a, S, B>, ApiError> {
        let storage = cx.get_storage::<S>();
        let subscriber = cx.get_subscriber::<B>();
        let address = address
            .hex_decode::<UnshieldedAddress>()
            .map_err_into_client_error(|| "invalid recipient address")?;

        let stream = try_stream! {
            // Emit initial balance.
            let initial = storage
                .get_bridge_balance(address)
                .await
                .map_err_into_server_error(|| "get bridge balance")?;
            yield BridgeBalance::from(initial);

            // Re-emit on each relevant pub-sub event.
            let live = subscriber
                .subscribe::<BridgeEventIndexed>()
                .try_filter(move |evt| {
                    let matches = evt
                        .event
                        .recipient()
                        .map(|r| r.as_bytes() == address.as_ref())
                        .unwrap_or(false);
                    ready(matches)
                });
            let mut live = pin!(live);
            while let Some(_msg) = live.try_next().await
                .map_err_into_server_error(|| "subscribe BridgeEventIndexed")?
            {
                let updated = storage
                    .get_bridge_balance(address)
                    .await
                    .map_err_into_server_error(|| "get bridge balance (live)")?;
                yield BridgeBalance::from(updated);
            }
        };

        Ok(stream)
    }
}
