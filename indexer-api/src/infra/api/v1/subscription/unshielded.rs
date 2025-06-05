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
    infra::api::{
        ContextExt, ResultExt,
        v1::{
            UnshieldedAddress, UnshieldedUtxo, UnshieldedUtxoEvent, UnshieldedUtxoEventType,
            addr_to_common,
        },
    },
};
use async_graphql::{Context, Subscription, async_stream::try_stream};
use fastrace::trace;
use futures::{Stream, TryStreamExt};
use indexer_common::{
    domain::{Subscriber, UnshieldedUtxoIndexed},
    error::StdErrorExt,
};
use log::{debug, error, warn};
use std::{marker::PhantomData, pin::pin, time::Duration};
use tokio::{
    select,
    time::{MissedTickBehavior, interval},
};

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

        let common_address = addr_to_common(&requested, network_id)?;

        let stream = try_stream! {
            // Create a drop guard that logs when the subscription ends
            let _guard = scopeguard::guard((), |_| {
                debug!(
                    "unshielded UTXO subscription dropped for address: {:?}",
                    &requested.0
                );
            });

            let mut utxo_stream = pin!(utxo_stream);

            let mut keep_alive = interval(Duration::from_secs(30));
            keep_alive.set_missed_tick_behavior(MissedTickBehavior::Skip);

            let mut last_transaction = storage
                .get_transactions_involving_unshielded(&common_address)
                .await
                .internal("get latest transaction for address")?
                .into_iter()
                .next();

            loop {
                select! {
                    event_result = utxo_stream.try_next() => {
                        match event_result {
                            Ok(Some(UnshieldedUtxoIndexed { address, transaction_id })) => {
                                if address != requested.0 {
                                    continue;
                                }

                                debug!(address, transaction_id; "handling UnshieldedUtxoIndexed event");

                                let tx = storage
                                    .get_transaction_by_id(transaction_id)
                                    .await
                                    .internal("fetch tx for subscription event").unwrap();

                                last_transaction = Some(tx.clone());

                                let created = storage
                                    .get_unshielded_utxos(
                                        Some(&common_address),
                                        UnshieldedUtxoFilter::CreatedInTxForAddress(transaction_id),
                                    )
                                    .await
                                    .internal("fetch created UTXOs").unwrap();

                                let spent = storage
                                    .get_unshielded_utxos(
                                        Some(&common_address),
                                        UnshieldedUtxoFilter::SpentInTxForAddress(transaction_id),
                                    )
                                    .await
                                    .internal("fetch spent UTXOs").unwrap();

                                yield UnshieldedUtxoEvent {
                                    event_type: UnshieldedUtxoEventType::UPDATE,
                                    transaction: tx.into(),
                                    created_utxos: created.into_iter()
                                        .map(|utxo| UnshieldedUtxo::<S>::from((utxo, network_id)))
                                        .collect(),
                                    spent_utxos: spent.into_iter()
                                        .map(|utxo| UnshieldedUtxo::<S>::from((utxo, network_id)))
                                        .collect(),
                                };
                            }

                            Ok(None) => {
                                warn!("stream of UnshieldedUtxoIndexed ended unexpectedly");
                                break;
                            }

                            Err(error) => {
                                error!(error = error.as_chain(); "cannot get next UnshieldedUtxoIndexed");
                                break;
                            }
                        }
                    }

                    // Emit periodic PROGRESS events
                    _ = keep_alive.tick() => {
                        debug!(address = requested.0; "emitting PROGRESS event");

                        // For PROGRESS events, we need a transaction to include
                        // If we don't have one for this address, we'll get the latest one from the chain
                        let tx = match &last_transaction {
                            Some(tx) => tx.to_owned(),

                            None => {
                                // Try to get the latest transaction from the chain
                                match storage.get_latest_block().await {
                                    Ok(Some(block)) => {
                                        match storage.get_transactions_by_block_id(block.id).await {
                                            Ok(transactions) if !transactions.is_empty() => {
                                                transactions.into_iter().next().unwrap()
                                            }

                                            _ => {
                                                // No transactions available, skip this PROGRESS event
                                                continue;
                                            }
                                        }
                                    }
                                    _ => {
                                        // Can't get latest block, skip this PROGRESS event
                                        continue;
                                    }
                                }
                            }
                        };

                        yield UnshieldedUtxoEvent {
                            event_type: UnshieldedUtxoEventType::PROGRESS,
                            transaction: tx.into(),
                            created_utxos: Vec::new(),
                            spent_utxos: Vec::new(),
                        };
                    }
                }
            }
        };

        Ok(stream)
    }
}
