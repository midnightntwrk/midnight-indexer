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

use crate::{
    domain::Topic,
    infra::pub_sub::in_mem::{publisher::InMemPublisher, subscriber::InMemSubscriber},
};
use log::warn;
use serde_json::Value;
use std::array;
use tokio::{
    sync::broadcast::{self, Receiver, Sender, error::RecvError},
    task,
};

/// Factory for in memory based implementations for publishers and subscribers.
#[derive(Clone)]
pub struct InMemPubSub([Sender<Value>; Topic::VARIANTS.len()]);

impl InMemPubSub {
    /// Factory for [InMemPublisher].
    pub fn publisher(&self) -> InMemPublisher {
        InMemPublisher::new(self.clone())
    }

    /// Factory for [InMemSubscriber].
    pub fn subscriber(&self) -> InMemSubscriber {
        InMemSubscriber::new(self.clone())
    }

    fn sender(&self, topic: Topic) -> &Sender<Value> {
        &self.0[topic as usize]
    }
}

impl Default for InMemPubSub {
    fn default() -> Self {
        // The array type fixes the iteration count at `Topic::VARIANTS.len()`, so `index` is
        // always in range below.
        Self(array::from_fn(|index| {
            let topic = Topic::VARIANTS[index];
            let (sender, receiver) = broadcast::channel(capacity(topic));
            // Keep one receiver alive per topic for as long as the `InMemPubSub`
            // lives. This guarantees that `broadcast::Sender::send` always has at
            // least one active receiver, so publishers do not see spurious
            // "channel closed" errors when no external subscriber happens to be
            // attached. `RecvError::Lagged` does not invalidate the receiver —
            // `recv` just skips ahead — so we must keep looping, not break.
            spawn_drain(topic, receiver);
            sender
        }))
    }
}

fn spawn_drain(topic: Topic, mut receiver: Receiver<Value>) {
    task::spawn(async move {
        loop {
            match receiver.recv().await {
                Ok(_) => continue,

                Err(RecvError::Lagged(skipped)) => {
                    warn!(topic:%, skipped; "drain receiver lagged");
                    continue;
                }

                Err(RecvError::Closed) => break,
            }
        }
    });
}

/// Messages buffered per subscriber before the slowest one starts losing them.
///
/// The match is exhaustive, so a new topic must be sized here before it compiles.
///
/// # Panics
/// `broadcast::channel` panics on a capacity of zero.
const fn capacity(topic: Topic) -> usize {
    use Topic::*;
    match topic {
        BlockIndexed => 42,
        WalletIndexed => 42,
        UnshieldedUtxoIndexed => 42,
        BridgeEventIndexed => 42,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        domain::{
            BlockIndexed, BridgeEventIndexed, Publisher, Subscriber, WalletIndexed,
            bridge::BridgeEvent,
        },
        infra::pub_sub::in_mem::InMemPubSub,
    };
    use assert_matches::assert_matches;
    use futures::StreamExt;
    use std::{error::Error as StdError, time::Duration};
    use tokio::time::sleep;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_publish_subscribe() -> Result<(), Box<dyn StdError>> {
        let pub_sub = InMemPubSub::default();
        sleep(Duration::from_millis(50)).await; //testing if IN_MEM_PUB_SUB doesn't get dropped

        let block_indexed = BlockIndexed {
            height: 123,
            max_transaction_id: None,
            caught_up: false,
        };
        let publish_block_res = pub_sub.publisher().publish(&block_indexed).await;

        assert!(publish_block_res.is_ok());

        let subscriber = pub_sub.subscriber();
        let mut messages = subscriber.subscribe::<WalletIndexed>();

        let wallet_indexed = WalletIndexed {
            wallet_id: Uuid::nil(),
        };
        pub_sub.publisher().publish(&wallet_indexed).await?;

        let message = messages.next().await;
        assert_matches!(message, Some(Ok(message)) if message == wallet_indexed);

        Ok(())
    }

    /// Regression test: publishing a bridge event through the in-memory pub-sub used to panic
    /// with "unexpected topic" because `BridgeEventIndexed` had no channel.
    #[tokio::test]
    async fn test_publish_subscribe_bridge_event() -> Result<(), Box<dyn StdError>> {
        let pub_sub = InMemPubSub::default();

        let subscriber = pub_sub.subscriber();
        let mut messages = subscriber.subscribe::<BridgeEventIndexed>();

        let bridge_event_indexed = BridgeEventIndexed {
            block_height: 42,
            event: BridgeEvent::ReserveTransfer {
                mc_tx_hash: [1u8; 32].into(),
                amount: 1_000_000,
                midnight_tx_hash: [2u8; 32].into(),
            },
        };
        pub_sub.publisher().publish(&bridge_event_indexed).await?;

        let message = messages.next().await;
        assert_matches!(message, Some(Ok(message)) if message == bridge_event_indexed);

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

        for height in 0..1000 {
            publisher
                .publish(&BlockIndexed {
                    height,
                    max_transaction_id: None,
                    caught_up: false,
                })
                .await?;
        }

        // Let the drain task observe the lag.
        sleep(Duration::from_millis(50)).await;

        // If the drain task broke on lag, this publish would fail with
        // `SendError` because no receivers remain.
        publisher
            .publish(&BlockIndexed {
                height: 9999,
                max_transaction_id: None,
                caught_up: false,
            })
            .await?;

        Ok(())
    }
}
