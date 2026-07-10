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

use crate::{
    domain::{BlockIndexed, Message, Subscriber, Topic, UnshieldedUtxoIndexed, WalletIndexed},
    infra::{pool::postgres::PostgresPool, pub_sub::pg::channel},
};
use futures::{Stream, StreamExt};
use log::warn;
use sqlx::postgres::PgListener;
use std::{sync::Arc, time::Duration};
use thiserror::Error;
use tokio::{sync::broadcast, task};
use tokio_stream::wrappers::{BroadcastStream, errors::BroadcastStreamRecvError};

/// Buffer per topic. Notifications are edge-triggered nudges recovered by cursor re-queries, so a
/// slow subscriber that lags past this bound drops nudges rather than stalling everyone.
const CHANNEL_CAPACITY: usize = 1024;

/// Delay before re-establishing a listener after its stream ends or errors.
const RECONNECT_DELAY: Duration = Duration::from_millis(100);

/// Postgres `LISTEN`/`NOTIFY` based [Subscriber].
///
/// One background [PgListener] per topic — a fixed, small number of dedicated connections — fans
/// each notification out to all subscribers of that topic via a [`broadcast`] channel. The
/// connection count is therefore independent of the number of concurrent subscribers, unlike a
/// naive listener-per-subscription which would exhaust the pool at scale.
#[derive(Clone)]
pub struct PgSubscriber {
    senders: Arc<Senders>,
}

struct Senders {
    block_indexed: broadcast::Sender<String>,
    wallet_indexed: broadcast::Sender<String>,
    unshielded_utxo: broadcast::Sender<String>,
}

impl PgSubscriber {
    /// Create a subscriber, spawning one listener task per topic against the given pool.
    pub fn new(pool: PostgresPool) -> Self {
        let senders = Senders {
            block_indexed: spawn_listener::<BlockIndexed>(pool.clone()),
            wallet_indexed: spawn_listener::<WalletIndexed>(pool.clone()),
            unshielded_utxo: spawn_listener::<UnshieldedUtxoIndexed>(pool),
        };

        Self {
            senders: Arc::new(senders),
        }
    }

    fn sender_for(&self, topic: Topic) -> &broadcast::Sender<String> {
        match topic {
            Topic("BlockIndexed") => &self.senders.block_indexed,
            Topic("WalletIndexed") => &self.senders.wallet_indexed,
            Topic("UnshieldedUtxoIndexed") => &self.senders.unshielded_utxo,

            // This must not happen; if it does, we forgot to add an arm for the topic above!
            _ => panic!("unexpected topic {topic:?}"),
        }
    }
}

impl Subscriber for PgSubscriber {
    type Error = SubscriberError;

    fn subscribe<T>(&self) -> impl Stream<Item = Result<T, Self::Error>> + Send
    where
        T: Message,
    {
        let receiver = self.sender_for(T::TOPIC).subscribe();

        BroadcastStream::new(receiver).filter_map(|payload| async move {
            match payload {
                Ok(payload) => Some(serde_json::from_str::<T>(&payload).map_err(Into::into)),

                // Lagging drops nudges; safe because subscribers recover via cursor re-queries.
                Err(BroadcastStreamRecvError::Lagged(skipped)) => {
                    warn!(skipped; "pg subscriber lagged; dropped nudges recovered by re-query");
                    None
                }
            }
        })
    }
}

/// Spawn a background task that keeps a [PgListener] on the given topic's channel alive, forwarding
/// each notification's payload into a [`broadcast`] channel shared by all subscribers.
fn spawn_listener<T>(pool: PostgresPool) -> broadcast::Sender<String>
where
    T: Message,
{
    let (sender, _keepalive) = broadcast::channel(CHANNEL_CAPACITY);
    let channel_name = channel(T::TOPIC);

    task::spawn({
        let sender = sender.clone();
        async move {
            loop {
                if let Err(error) = run_listener(&pool, &channel_name, &sender).await {
                    warn!(error:%, channel = channel_name.as_str(); "pg listener failed; reconnecting");
                }
                tokio::time::sleep(RECONNECT_DELAY).await;
            }
        }
    });

    sender
}

async fn run_listener(
    pool: &PostgresPool,
    channel_name: &str,
    sender: &broadcast::Sender<String>,
) -> Result<(), sqlx::Error> {
    let mut listener = PgListener::connect_with(pool).await?;
    listener.listen(channel_name).await?;

    let mut stream = listener.into_stream();
    while let Some(notification) = stream.next().await {
        // A send error only means no subscribers are currently attached, which is fine: the
        // notification is a nudge and a late subscriber catches up via its initial cursor query.
        let _ = sender.send(notification?.payload().to_owned());
    }

    Ok(())
}

#[derive(Debug, Error)]
pub enum SubscriberError {
    #[error("cannot JSON deserialize message")]
    Deserialize(#[from] serde_json::Error),
}
