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

//! Subscription resolver for `contractEvents` (ticket #1161).
//!
//! Starting cursor: either the `id` argument (resume from a previous event)
//! OR `filter.fromBlock` (start from the first event in that block); if both
//! are set, indexer uses the LATER cursor. Pattern follows `dustLedgerEvents`
//! / `zswapLedgerEvents` for the id-cursor path.
//!
//! `filter.toBlock`, if set, terminates the stream once the chain reaches
//! that block, matching the bounded-subscription pattern used by
//! `dust_nullifier_transactions` / `shielded_nullifier_transactions`.

use crate::{
    domain::{ContractEventRow, storage::Storage},
    infra::api::{
        ApiError, ApiResult, ContextExt, ResultExt,
        v4::{
            contract_event::{ContractEvent, ContractEventFilter as GraphQLContractEventFilter},
            directives::beta,
        },
    },
};
use async_graphql::{Context, Subscription};
use async_stream::try_stream;
use fastrace::{Span, future::FutureExt, prelude::SpanContext};
use futures::{Stream, TryStreamExt};
use indexer_common::domain::{BlockIndexed, Subscriber};
use log::{debug, warn};
use std::{marker::PhantomData, pin::pin};

pub struct ContractEventsSubscription<S, B> {
    _s: PhantomData<S>,
    _b: PhantomData<B>,
}

impl<S, B> Default for ContractEventsSubscription<S, B> {
    fn default() -> Self {
        Self {
            _s: PhantomData,
            _b: PhantomData,
        }
    }
}

#[Subscription]
impl<S, B> ContractEventsSubscription<S, B>
where
    S: Storage,
    B: Subscriber,
{
    /// Subscribe to contract events matching the given filter, returning
    /// events in monotonic `id` order.
    #[graphql(directive = beta::apply())]
    async fn contract_events<'a>(
        &self,
        cx: &'a Context<'a>,
        filter: GraphQLContractEventFilter,
        id: Option<u64>,
    ) -> impl Stream<Item = ApiResult<ContractEvent<S>>> {
        let storage = cx.get_storage::<S>();
        let subscriber = cx.get_subscriber::<B>();
        let batch_size = cx.get_subscription_config().contract_events.batch_size;
        let quotas = cx.get_subscription_quotas();
        let per_connection_counter = cx.get_per_connection_counter();
        let block_indexed_stream = subscriber.subscribe::<BlockIndexed>();

        try_stream! {
            let _quota_guard = quotas
                .try_acquire(per_connection_counter, None)
                .map_err_into_client_error(|| "subscription limit exceeded")?;

            let domain_filter = filter
                .into_domain()
                .map_err(|e| ApiError::client("invalid ContractEventFilter", e))?;

            // Starting cursor: explicit `id` arg only. `filter.fromBlock` is
            // applied via the storage layer's WHERE clause (`blocks.height >=
            // from_block`), so we don't need to translate it to an id here.
            // If both are set, the WHERE clause + id cursor combine — the
            // effective cursor is "id >= id_arg AND blocks.height >=
            // from_block", which is exactly the "later of the two" semantics
            // requested in v0.7.
            let mut id = id.unwrap_or(0);

            // Bounded-subscription terminator: if filter.toBlock is set, we
            // stop once the chain's latest block height crosses it. The check
            // happens after each backfill drain and each live-tail tick.
            let to_block = domain_filter.to_block;

            debug!(id; "streaming existing contract events");
            let rows = storage
                .get_contract_events_after_id(domain_filter.clone(), id, batch_size)
                .await;
            let mut rows = pin!(rows);
            while let Some(row) = get_next_row(&mut rows)
                .await
                .map_err_into_server_error(|| format!("get next contract event at id {id}"))?
            {
                id = row.id + 1;
                yield ContractEvent::try_from(row).map_err_into_server_error(|| {
                    format!("unexpected contract event row at id {id}")
                })?;
            }

            if reached_to_block(storage, to_block).await? {
                debug!(to_block:?; "contractEvents subscription terminated at toBlock");
                return;
            }

            debug!(id; "streaming live contract events");
            let mut block_indexed_stream = pin!(block_indexed_stream);
            while block_indexed_stream
                .try_next()
                .await
                .map_err_into_server_error(|| "get next BlockIndexed event")?
                .is_some()
            {
                let rows = storage
                    .get_contract_events_after_id(domain_filter.clone(), id, batch_size)
                    .await;
                let mut rows = pin!(rows);
                while let Some(row) = get_next_row(&mut rows)
                    .await
                    .map_err_into_server_error(|| {
                        format!("get next contract event at id {id}")
                    })?
                {
                    id = row.id + 1;
                    yield ContractEvent::try_from(row).map_err_into_server_error(|| {
                        format!("unexpected contract event row at id {id}")
                    })?;
                }

                if reached_to_block(storage, to_block).await? {
                    debug!(to_block:?; "contractEvents subscription terminated at toBlock");
                    return;
                }
            }

            warn!("contract-events stream: BlockIndexed channel ended unexpectedly");
        }
    }
}

async fn get_next_row<E>(
    rows: &mut (impl Stream<Item = Result<ContractEventRow, E>> + Unpin),
) -> Result<Option<ContractEventRow>, E> {
    rows.try_next()
        .in_span(Span::root(
            "subscription.contract-events.get-next-row",
            SpanContext::random(),
        ))
        .await
}

async fn reached_to_block<S: Storage>(storage: &S, to_block: Option<u32>) -> ApiResult<bool> {
    let Some(to_block) = to_block else {
        return Ok(false);
    };
    let latest = storage
        .get_latest_block()
        .await
        .map_err_into_server_error(|| "get latest block to evaluate toBlock terminator")?;
    Ok(latest.map(|b| b.height >= to_block).unwrap_or(false))
}
