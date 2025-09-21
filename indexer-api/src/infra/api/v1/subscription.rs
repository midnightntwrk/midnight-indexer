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

mod block;
mod contract_action;
mod shielded;
mod unshielded;
mod zswap_ledger_events;

use crate::{
    domain::{self, storage::Storage},
    infra::api::v1::subscription::{
        block::BlockSubscription, contract_action::ContractActionSubscription,
        shielded::ShieldedTransactionsSubscription, unshielded::UnshieldedTransactionsSubscription,
        zswap_ledger_events::ZswapLedgerEventsSubscription,
    },
};
use async_graphql::MergedSubscription;
use fastrace::{Span, future::FutureExt, prelude::SpanContext};
use futures::{Stream, stream::TryStreamExt};
use indexer_common::domain::{LedgerStateStorage, Subscriber};

#[derive(MergedSubscription)]
pub struct Subscription<S, B, Z>(
    BlockSubscription<S, B>,
    ContractActionSubscription<S, B>,
    ShieldedTransactionsSubscription<S, B, Z>,
    UnshieldedTransactionsSubscription<S, B>,
    ZswapLedgerEventsSubscription<S, B>,
)
where
    S: Storage,
    B: Subscriber,
    Z: LedgerStateStorage;

impl<S, B, Z> Default for Subscription<S, B, Z>
where
    S: Storage,
    B: Subscriber,
    Z: LedgerStateStorage,
{
    fn default() -> Self {
        Subscription(
            BlockSubscription::default(),
            ContractActionSubscription::default(),
            ShieldedTransactionsSubscription::default(),
            UnshieldedTransactionsSubscription::default(),
            ZswapLedgerEventsSubscription::default(),
        )
    }
}

async fn get_next_transaction<E>(
    transactions: &mut (impl Stream<Item = Result<domain::Transaction, E>> + Unpin),
) -> Result<Option<domain::Transaction>, E> {
    transactions
        .try_next()
        .in_span(Span::root(
            "subscription.transactions.get-next-transaction",
            SpanContext::random(),
        ))
        .await
}
