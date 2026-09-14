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
        // `broadcast::channel` rounds its capacity up to the next power of two, so keep every
        // value a power of two for the number here to be the ring size.
        BlockIndexed | WalletIndexed | UnshieldedUtxoIndexed | BridgeEventIndexed => 64,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        domain::{
            BlockIndexed, BridgeEventIndexed, Message, Publisher, Subscriber, Topic,
            UnshieldedUtxoIndexed, WalletIndexed, bridge::BridgeEvent,
        },
        infra::pub_sub::in_mem::{InMemPubSub, capacity},
    };
    use assert_matches::assert_matches;
    use futures::StreamExt;
    use serde_json::Value;
    use std::{error::Error as StdError, time::Duration};
    use tokio::{sync::broadcast, time::sleep};
    use uuid::Uuid;

    /// Every message type reaches a subscriber for it. The match is exhaustive, so a new topic
    /// must be round-tripped here too.
    #[tokio::test]
    async fn test_publish_subscribe() -> Result<(), Box<dyn StdError>> {
        let pub_sub = InMemPubSub::default();

        for &topic in Topic::VARIANTS {
            match topic {
                Topic::BlockIndexed => {
                    let event = BlockIndexed {
                        height: 123,
                        max_transaction_id: None,
                        caught_up: false,
                    };
                    assert_publish_subscribe(&pub_sub, event).await?
                }

                Topic::WalletIndexed => {
                    let event = WalletIndexed {
                        wallet_id: Uuid::nil(),
                    };
                    assert_publish_subscribe(&pub_sub, event).await?
                }

                Topic::UnshieldedUtxoIndexed => {
                    let event = UnshieldedUtxoIndexed {
                        address: [3u8; 32].into(),
                    };
                    assert_publish_subscribe(&pub_sub, event).await?
                }

                Topic::BridgeEventIndexed => {
                    let event = BridgeEventIndexed {
                        block_height: 42,
                        event: BridgeEvent::ReserveTransfer {
                            mc_tx_hash: [1u8; 32].into(),
                            amount: 1_000_000,
                            midnight_tx_hash: [2u8; 32].into(),
                        },
                    };
                    assert_publish_subscribe(&pub_sub, event).await?
                }
            }
        }

        Ok(())
    }

    /// Subscribes, publishes the message, then asserts the subscriber receives it unchanged.
    async fn assert_publish_subscribe<T: Message + Send + Sync>(
        pub_sub: &InMemPubSub,
        message: T,
    ) -> Result<(), Box<dyn StdError>> {
        let subscriber = pub_sub.subscriber();
        let mut messages = subscriber.subscribe::<T>();

        pub_sub.publisher().publish(&message).await?;

        let received = messages.next().await;
        assert_matches!(received, Some(Ok(received)) if received == message);

        Ok(())
    }

    /// `broadcast::channel` rounds its capacity up to the next power of two: asking for 42
    /// allocates the same 64 slots as asking for 64. `Sender::len` counts slots still unread by
    /// some receiver, so with one idle receiver and more sends than slots it is the ring size.
    #[test]
    fn test_broadcast_channel_rounds_capacity_up() -> Result<(), Box<dyn StdError>> {
        for requested in [42, 64] {
            let (sender, _receiver) = broadcast::channel(requested);
            for _ in 0..128 {
                sender.send(())?;
            }

            assert_eq!(sender.len(), 64);
        }

        Ok(())
    }

    /// Regression test: when no external subscriber is attached, the drain
    /// task is the sole receiver keeping a topic's channel alive. If it broke
    /// on `RecvError::Lagged`, the receiver would be dropped and subsequent
    /// sends would fail with `SendError` because the broadcast channel has no
    /// active receivers.
    ///
    /// To force the drain task to lag, we send one message past the channel's
    /// capacity in a tight loop; `broadcast::channel` rounds its capacity up to
    /// the next power of two, so the ring holds more slots than `capacity`
    /// asks for. `send` contains no await points, so on a current-thread
    /// runtime the drain task cannot be scheduled until we explicitly yield,
    /// guaranteeing overflow. Every topic is covered by driving the loop from
    /// `Topic::VARIANTS`.
    #[tokio::test(flavor = "current_thread")]
    async fn test_drain_survives_lag() -> Result<(), Box<dyn StdError>> {
        let pub_sub = InMemPubSub::default();

        for &topic in Topic::VARIANTS {
            // The drain discards whatever it receives, so the payload carries nothing and
            // `InMemPublisher` would only add a serialization step this test does not exercise.
            let sender = pub_sub.sender(topic);

            for _ in 0..=capacity(topic).next_power_of_two() {
                sender.send(Value::Null)?;
            }

            // Let the drain task observe the lag.
            sleep(Duration::from_millis(50)).await;

            // If the drain task broke on lag, this send would fail with `SendError` because no
            // receivers remain.
            sender.send(Value::Null)?;
        }

        Ok(())
    }
}
