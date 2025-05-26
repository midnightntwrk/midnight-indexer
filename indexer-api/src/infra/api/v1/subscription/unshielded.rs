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
    domain::{Storage, UnshieldedUtxoFilter},
    infra::api::v1::{
        ContextExt, UnshieldedAddress, UnshieldedUtxo, UnshieldedUtxoEvent,
        UnshieldedUtxoEventType, addr_to_common,
    },
};
use anyhow::Context as AnyhowContext;
use async_graphql::{Context, Subscription, async_stream::try_stream};
use fastrace::trace;
use futures::{Stream, TryStreamExt};
use indexer_common::{
    domain::{Subscriber, UnshieldedUtxoIndexed},
    error::StdErrorExt,
};
use log::{debug, error, warn};
use std::{marker::PhantomData, pin::pin};

/// Same skeleton pattern as block / contract / wallet subscriptions
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
    /// * `address` - The unshielded address to monitor (must be in Bech32m format)
    ///
    /// # Returns
    /// A stream of `UnshieldedUtxoEvent`s containing:
    /// - `eventType`: UPDATE (for actual changes) or PROGRESS (for keep-alive messages)
    /// - `transaction`: The transaction that created/spent UTXOs
    /// - `createdUtxos`: UTXOs created in this transaction for the address
    /// - `spentUtxos`: UTXOs spent in this transaction for the address
    #[trace(properties = { "address": "{address:?}" })]
    async fn unshielded_utxos<'a>(
        &self,
        cx: &'a Context<'a>,
        address: UnshieldedAddress,
    ) -> async_graphql::Result<impl Stream<Item = async_graphql::Result<UnshieldedUtxoEvent<S>>> + 'a>
    {
        let subscriber = cx.get_subscriber::<B>();
        let storage = cx.get_storage::<S>();
        let network_id = cx.get_network_id();

        let utxo_stream = subscriber.subscribe::<UnshieldedUtxoIndexed>();

        let requested = address;

        let stream = try_stream! {
            let mut utxo_stream = pin!(utxo_stream);

            while let Some(UnshieldedUtxoIndexed {address_bech32m, transaction_id} ) = utxo_stream.try_next().await.inspect_err(|e| error!(
                error:? = e.as_chain(); "cannot get next UnshieldedUtxoIndexed"
            ))? {
                if address_bech32m != requested.0 {
                    continue;
                }

                let common_address = addr_to_common(&requested, network_id)?;

                debug!("handling UnshieldedUtxoIndexed event, address: {:?}, tx_id: {:?}", &address_bech32m, &transaction_id);

                let tx = storage
                    .get_transaction_by_id(transaction_id)
                    .await
                    .context("fetch tx for subscription event")?;

                let created = storage
                    .get_unshielded_utxos(
                        Some(&common_address),
                        UnshieldedUtxoFilter::CreatedInTxForAddress(transaction_id),
                    )
                    .await
                    .context("fetch created UTXOs")?;

                let spent = storage
                    .get_unshielded_utxos(
                        Some(&common_address),
                        UnshieldedUtxoFilter::SpentInTxForAddress(transaction_id),
                    )
                    .await
                    .context("fetch spent UTXOs")?;

                let (event_type, created_utxos, spent_utxos) = if created.is_empty() && spent.is_empty() {
                    (
                        UnshieldedUtxoEventType::PROGRESS,
                        Vec::new(),
                        Vec::new(),
                    )
                } else {
                    (
                        UnshieldedUtxoEventType::UPDATE,
                        created.into_iter()
                               .map(|utxo| UnshieldedUtxo::<S>::from((utxo, network_id)))
                               .collect(),
                        spent.into_iter()
                              .map(|utxo| UnshieldedUtxo::<S>::from((utxo, network_id)))
                              .collect(),
                    )
                };

                yield UnshieldedUtxoEvent {
                    event_type,
                    transaction: tx.into(),
                    created_utxos,
                    spent_utxos,
                };
            }

            warn!("stream of UnshieldedUtxoIndexed ended unexpectedly");
        };

        Ok(stream)
    }
}
