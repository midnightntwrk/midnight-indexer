// This file is part of midnight-indexer.
// Copyright (C) Midnight Foundation
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

pub mod publisher;
pub mod subscriber;

use crate::infra::pub_sub::in_mem::{publisher::InMemPublisher, subscriber::InMemSubscriber};
use log::warn;
use serde_json::Value;
use tokio::{
    sync::broadcast::{self, Receiver, Sender, error::RecvError},
    task,
};

/// Factory for in memory based implementations for publishers and subscribers.
#[derive(Clone)]
pub struct InMemPubSub {
    block_indexed_sender: Sender<Value>,
    wallet_indexed_sender: Sender<Value>,
    unshielded_utxo_sender: Sender<Value>,
}

impl InMemPubSub {
    /// Factory for [InMemPublisher].
    pub fn publisher(&self) -> InMemPublisher {
        InMemPublisher::new(self.clone())
    }

    /// Factory for [InMemSubscriber].
    pub fn subscriber(&self) -> InMemSubscriber {
        InMemSubscriber::new(self.clone())
    }
}

impl Default for InMemPubSub {
    fn default() -> Self {
        let (block_indexed_sender, block_indexed_receiver) = broadcast::channel(42);
        let (wallet_indexed_sender, wallet_indexed_receiver) = broadcast::channel(42);
        let (unshielded_utxo_sender, unshielded_utxo_receiver) = broadcast::channel(42);

        let pub_sub = InMemPubSub {
            block_indexed_sender,
            wallet_indexed_sender,
            unshielded_utxo_sender,
        };

        // Keep one receiver alive per topic for as long as the `InMemPubSub`
        // lives. This guarantees that `broadcast::Sender::send` always has at
        // least one active receiver, so publishers do not see spurious
        // "channel closed" errors when no external subscriber happens to be
        // attached. `RecvError::Lagged` does not invalidate the receiver —
        // `recv` just skips ahead — so we must keep looping, not break.
        spawn_drain("block_indexed_receiver", block_indexed_receiver);
        spawn_drain("wallet_indexed_receiver", wallet_indexed_receiver);
        spawn_drain("unshielded_utxo_receiver", unshielded_utxo_receiver);

        pub_sub
    }
}

fn spawn_drain(name: &'static str, mut receiver: Receiver<Value>) {
    task::spawn(async move {
        loop {
            match receiver.recv().await {
                Ok(_) => continue,

                Err(RecvError::Lagged(skipped)) => {
                    warn!(receiver = name, skipped; "drain receiver lagged");
                    continue;
                }

                Err(RecvError::Closed) => break,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use crate::infra::pool::sqlite::{self, SqlitePool};
    use crate::{
        domain::{BlockIndexed, Publisher, Subscriber, WalletIndexed},
        infra::pub_sub::in_mem::InMemPubSub,
    };
    use assert_matches::assert_matches;
    use futures::StreamExt;
    use std::{error::Error as StdError, time::Duration};
    use tokio::time::sleep;
    use uuid::Uuid;

    /// An in-memory SQLite transaction. [InMemPublisher::stage] ignores it, so it need not be
    /// committed; it exists only to satisfy the [Publisher] signature.
    async fn dummy_tx(pool: &SqlitePool) -> sqlx::Transaction<'static, sqlx::Sqlite> {
        pool.begin().await.expect("begin dummy transaction")
    }

    #[tokio::test]
    async fn test_publish_subscribe() -> Result<(), Box<dyn StdError>> {
        let pub_sub = InMemPubSub::default();
        let pool = SqlitePool::new(sqlite::Config::default()).await?;
        sleep(Duration::from_millis(50)).await; //testing if IN_MEM_PUB_SUB doesn't get dropped

        let block_indexed = BlockIndexed {
            height: 123,
            max_transaction_id: None,
            caught_up: false,
        };
        let mut tx = dummy_tx(&pool).await;
        let pending = pub_sub.publisher().stage(&mut tx, &block_indexed).await?;
        assert!(pub_sub.publisher().deliver(pending).await.is_ok());

        let subscriber = pub_sub.subscriber();
        let mut messages = subscriber.subscribe::<WalletIndexed>();

        let wallet_indexed = WalletIndexed {
            wallet_id: Uuid::nil(),
        };
        let pending = pub_sub.publisher().stage(&mut tx, &wallet_indexed).await?;
        pub_sub.publisher().deliver(pending).await?;

        let message = messages.next().await;
        assert_matches!(message, Some(Ok(message)) if message == wallet_indexed);

        Ok(())
    }

    /// Regression test: when no external subscriber is attached, the drain
    /// task is the sole receiver keeping the channel alive. If it broke on
    /// `RecvError::Lagged` (the pre-fix behavior), the receiver would be
    /// dropped and subsequent `publish` calls would fail with `SendError`
    /// because the broadcast channel has no active receivers.
    ///
    /// To force the drain task to lag, we publish far more messages than the
    /// channel capacity (42) in a tight loop. `publish` contains no await
    /// points, so on a current-thread runtime the drain task cannot be
    /// scheduled until we explicitly yield, guaranteeing overflow.
    #[tokio::test(flavor = "current_thread")]
    async fn test_drain_survives_lag() -> Result<(), Box<dyn StdError>> {
        let pub_sub = InMemPubSub::default();
        let publisher = pub_sub.publisher();
        let pool = SqlitePool::new(sqlite::Config::default()).await?;
        let mut tx = dummy_tx(&pool).await;

        for height in 0..1000 {
            let pending = publisher
                .stage(
                    &mut tx,
                    &BlockIndexed {
                        height,
                        max_transaction_id: None,
                        caught_up: false,
                    },
                )
                .await?;
            publisher.deliver(pending).await?;
        }

        // Let the drain task observe the lag.
        sleep(Duration::from_millis(50)).await;

        // If the drain task broke on lag, this deliver would fail with
        // `SendError` because no receivers remain.
        let pending = publisher
            .stage(
                &mut tx,
                &BlockIndexed {
                    height: 9999,
                    max_transaction_id: None,
                    caught_up: false,
                },
            )
            .await?;
        publisher.deliver(pending).await?;

        Ok(())
    }
}
