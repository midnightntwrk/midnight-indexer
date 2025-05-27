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

use crate::domain::{Wallet, storage::Storage};
use anyhow::Context;
use fastrace::trace;
use futures::{Stream, StreamExt, TryStreamExt, stream};
use indexer_common::domain::{NetworkId, Publisher, WalletIndexed};
use itertools::Itertools;
use log::{debug, info};
use serde::Deserialize;
use std::{num::NonZeroUsize, time::Duration};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub network_id: NetworkId,

    #[serde(with = "humantime_serde")]
    pub active_wallets_repeat_delay: Duration,

    #[serde(with = "humantime_serde")]
    pub active_wallets_ttl: Duration,

    pub transaction_batch_size: NonZeroUsize,

    #[serde(default = "parallelism_default")]
    pub parallelism: NonZeroUsize,
}

pub async fn run(
    config: Config,
    storage: impl Storage,
    publisher: impl Publisher,
) -> anyhow::Result<()> {
    let Config {
        network_id,
        active_wallets_repeat_delay,
        active_wallets_ttl,
        transaction_batch_size,
        parallelism,
    } = config;

    active_wallets(active_wallets_repeat_delay, active_wallets_ttl, &storage)
        .map(|result| result.context("get next active wallet"))
        .try_for_each_concurrent(Some(parallelism.get()), |wallet| {
            let mut publisher = publisher.clone();
            let mut storage = storage.clone();

            async move {
                index_wallet(
                    wallet,
                    transaction_batch_size,
                    network_id,
                    &mut publisher,
                    &mut storage,
                )
                .await
            }
        })
        .await
}

fn active_wallets(
    active_wallets_repeat_delay: Duration,
    active_wallets_ttl: Duration,
    storage: &impl Storage,
) -> impl Stream<Item = Result<Wallet, sqlx::Error>> + '_ {
    tokio_stream::StreamExt::throttle(stream::repeat(()), active_wallets_repeat_delay)
        .map(|_| Ok::<_, sqlx::Error>(()))
        .and_then(move |_| storage.active_wallets(active_wallets_ttl))
        .map_ok(|wallets| stream::iter(wallets).map(Ok))
        .try_flatten()
}

#[trace]
async fn index_wallet(
    wallet: Wallet,
    transaction_batch_size: NonZeroUsize,
    network_id: NetworkId,
    publisher: &mut impl Publisher,
    storage: &mut impl Storage,
) -> anyhow::Result<()> {
    let session_id = wallet.viewing_key.to_session_id();

    debug!(session_id:%; "indexing wallet");

    let tx = storage
        .acquire_lock(session_id)
        .await
        .context("acquire lock")?;

    match tx {
        Some(mut tx) => {
            debug!(session_id:%; "acquired lock, handling session ID");

            let from = wallet.last_indexed_transaction_id + 1;
            let transactions = storage
                .get_transactions(from, transaction_batch_size, &mut tx)
                .await
                .context("get transactions")?;
            let Some(last_indexed_transaction_id) = transactions.iter().map(|t| t.id).max() else {
                debug!(from, transaction_batch_size; "no transactions");
                return Ok(());
            };
            debug!(from, last_indexed_transaction_id; "got transactions");

            let relevant_transactions = transactions
                .into_iter()
                .map(|transaction| {
                    transaction
                        .relevant(&wallet, network_id)
                        .context("check transaction relevance")
                        .map(|relevant| (relevant, transaction))
                })
                .filter_map_ok(|(relevant, transaction)| relevant.then_some(transaction))
                .collect::<Result<Vec<_>, _>>()?;
            debug!(len = relevant_transactions.len(); "relevant transactions");

            storage
                .save_relevant_transactions(
                    &wallet.viewing_key,
                    &relevant_transactions,
                    last_indexed_transaction_id,
                    &mut tx,
                )
                .await
                .context("save relevant transactions")?;

            tx.commit().await.context("commit database transaction")?;
            info!(session_id:%, from, transaction_batch_size; "wallet indexed");

            if !relevant_transactions.is_empty() {
                publisher
                    .publish(&WalletIndexed { session_id })
                    .await
                    .context("cannot publish WalletIndexed event")?;
            }
        }

        None => {
            debug!(session_id:%; "could not acquire lock, not handling wallet");
        }
    }

    Ok(())
}

fn parallelism_default() -> NonZeroUsize {
    std::thread::available_parallelism().unwrap_or(NonZeroUsize::MIN)
}
