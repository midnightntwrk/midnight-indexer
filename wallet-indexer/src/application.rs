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

use crate::domain::storage::Storage;
use anyhow::Context;
use async_stream::try_stream;
use dashmap::DashMap;
use fastrace::trace;
use futures::{Stream, StreamExt, TryStreamExt, future::ok};
use indexer_common::domain::{BlockIndexed, Publisher, Subscriber, WalletIndexed};
use itertools::Itertools;
use log::{debug, warn};
use serde::Deserialize;
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{select, signal::unix::Signal, sync::Semaphore, task, time::sleep};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(with = "humantime_serde")]
    pub active_wallets_query_delay: Duration,

    #[serde(with = "humantime_serde")]
    pub active_wallets_ttl: Duration,

    pub transaction_batch_size: NonZeroUsize,

    #[serde(default = "concurrency_limit_default")]
    pub concurrency_limit: NonZeroUsize,
}

pub async fn run(
    config: Config,
    storage: impl Storage,
    publisher: impl Publisher,
    subscriber: impl Subscriber,
    mut sigterm: Signal,
) -> anyhow::Result<()> {
    let Config {
        active_wallets_query_delay,
        active_wallets_ttl,
        transaction_batch_size,
        concurrency_limit,
    } = config;

    // Shared counter for the maximum transaction ID observed in BlockIndexed events. This allows
    // the Wallet Indexer to not unnecessarily query the database when it is already up-to-date. The
    // initial value is set to the maximum in case initial events are missed during startup.
    let max_transaction_id = Arc::new(AtomicU64::new(u64::MAX));

    let block_indexed_task = task::spawn({
        let subscriber = subscriber.clone();
        let max_transaction_id = max_transaction_id.clone();

        async move {
            let block_indexed_stream = subscriber.subscribe::<BlockIndexed>();

            block_indexed_stream
                .try_for_each(|block_indexed| {
                    if let Some(id) = block_indexed.max_transaction_id {
                        let max_id = max_transaction_id.load(Ordering::Acquire);

                        // Above we initially set max_transaction_id to u64::MAX so
                        // index_wallets_task will always index on startup. This initial value needs
                        // to be replaced unconditionally with the first received value.
                        if max_id == u64::MAX || max_id < id {
                            max_transaction_id.store(id, Ordering::Release);
                        }
                    }

                    ok(())
                })
                .await
                .context("cannot get next BlockIndexed event")?;

            warn!("block_indexed_task completed");

            Ok::<(), anyhow::Error>(())
        }
    });

    let index_wallets_task = {
        task::spawn(async move {
            // As wallet IDs are cycled (see comment of `active_wallet_ids`), we prevent concurrent
            // processing of the same wallet by using a semaphore of one (see below) per wallet ID.
            let worker_by_wallet_id = Arc::new(DashMap::new());

            active_wallet_ids(active_wallets_query_delay, active_wallets_ttl, &storage)
                .map(|result| result.context("get next active wallet ID"))
                .try_for_each_concurrent(Some(concurrency_limit.get()), |wallet_id| {
                    let worker_by_wallet_id = worker_by_wallet_id.clone();
                    let max_transaction_id = max_transaction_id.clone();
                    let mut publisher = publisher.clone();
                    let mut storage = storage.clone();

                    async move {
                        let permit = worker_by_wallet_id
                            .entry(wallet_id)
                            .or_insert_with(|| Arc::new(Semaphore::new(1)))
                            .clone()
                            .try_acquire_owned();

                        if permit.is_ok() {
                            index_wallet(
                                wallet_id,
                                transaction_batch_size,
                                max_transaction_id,
                                &mut publisher,
                                &mut storage,
                            )
                            .await?;
                        }

                        Ok(())
                    }
                })
                .await?;

            warn!("index_wallets_task completed");

            Ok::<(), anyhow::Error>(())
        })
    };

    select! {
        result = block_indexed_task => result
            .context("block_indexed_task")
            .and_then(|r| r.context("block_indexed_task failed")),

        result = index_wallets_task => result
            .context("index_wallets_task panicked")
            .and_then(|r| r.context("index_wallets_task failed")),

        _ = sigterm.recv() => {
            warn!("SIGTERM received");
            Ok(())
        }
    }
}

/// Repeatedly query the active wallet IDs with the given delay between repetitions and continuously
/// stream the ones of the current repetition in a cycle. This only hits the database once per
/// repetition but keeps the stream "hot" (fast producer), yet means that newly connected wallets
/// are only indexed once the current repetition ends. Therefore a balance between database load and
/// wallet latency needs to be found; reasonable values for the delay seem to be between 100ms and
/// 1000ms.
fn active_wallet_ids(
    active_wallets_query_delay: Duration,
    active_wallets_ttl: Duration,
    storage: &impl Storage,
) -> impl Stream<Item = Result<Uuid, sqlx::Error>> + '_ {
    try_stream! {
        loop {
            // Query the current active wallet IDs.
            let wallet_ids = storage.active_wallet_ids(active_wallets_ttl).await?;

            if wallet_ids.is_empty() {
                sleep(active_wallets_query_delay).await;
                continue;
            }

            let deadline = Instant::now() + active_wallets_query_delay;

            // First we stream all current wallet IDs exactly once, regardless of the deadline.
            for &wallet_id in &wallet_ids {
                yield wallet_id
            }

            // Then we cycle the current wallet IDs until the deadline is reached.
            for wallet_id in wallet_ids.into_iter().cycle() {
                if Instant::now() > deadline {
                    break;
                }
                yield wallet_id
            }
        }
    }
}

#[trace(properties = { "wallet_id": "{wallet_id}" })]
async fn index_wallet(
    wallet_id: Uuid,
    transaction_batch_size: NonZeroUsize,
    max_transaction_id: Arc<AtomicU64>,
    publisher: &mut impl Publisher,
    storage: &mut impl Storage,
) -> anyhow::Result<()> {
    let tx = storage
        .acquire_lock(wallet_id)
        .await
        .with_context(|| format!("acquire lock for wallet ID {wallet_id}"))?;

    let Some(mut tx) = tx else {
        return Ok(());
    };

    let wallet = storage
        .get_wallet_by_id(wallet_id, &mut tx)
        .await
        .with_context(|| format!("get wallet for wallet ID {wallet_id}"))?;

    // Only continue if possibly needed.
    if wallet.last_indexed_transaction_id < max_transaction_id.load(Ordering::Acquire) {
        let from = wallet.last_indexed_transaction_id + 1;
        let transactions = storage
            .get_transactions(from, transaction_batch_size, &mut tx)
            .await
            .context("get transactions")?;

        let last_indexed_transaction_id = if let Some(transaction) = transactions.last() {
            transaction.id
        } else {
            return Ok(());
        };

        let relevant_transactions = transactions
            .into_iter()
            .map(|transaction| {
                transaction
                    .relevant(&wallet)
                    .with_context(|| {
                        format!("check transaction relevance for wallet ID {wallet_id}")
                    })
                    .map(|relevant| (relevant, transaction))
            })
            .filter_map_ok(|(relevant, transaction)| relevant.then_some(transaction))
            .collect::<Result<Vec<_>, _>>()?;

        storage
            .save_relevant_transactions(
                &wallet.viewing_key,
                &relevant_transactions,
                last_indexed_transaction_id,
                &mut tx,
            )
            .await
            .with_context(|| format!("save relevant transactions for wallet ID {wallet_id}"))?;

        tx.commit().await.context("commit database transaction")?;

        if !relevant_transactions.is_empty() {
            let session_id = wallet.viewing_key.to_session_id();

            publisher
                .publish(&WalletIndexed { session_id })
                .await
                .with_context(|| {
                    format!("publish WalletIndexed event for wallet ID {wallet_id}")
                })?;
        }

        debug!(
            wallet_id:%,
            last_indexed_transaction_id,
            relevant_transactions_len = relevant_transactions.len();
            "wallet indexed"
        );
    }

    Ok(())
}

fn concurrency_limit_default() -> NonZeroUsize {
    std::thread::available_parallelism().unwrap_or(NonZeroUsize::MIN)
}
