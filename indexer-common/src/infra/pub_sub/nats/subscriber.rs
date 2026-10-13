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
    domain::{Message, Subscriber},
    infra::pub_sub::nats::Config,
};
use async_nats::{Client, ConnectOptions, Event};
use futures::{Stream, StreamExt, TryFutureExt, TryStreamExt, stream};
use log::{info, warn};
use secrecy::ExposeSecret;
use std::time::Duration;
use thiserror::Error;

const REPEAT_DELAY: Duration = Duration::from_millis(100);

// NATS based [Subscriber] implementation.
#[derive(Clone)]
pub struct NatsSubscriber {
    client: Client,
}

impl NatsSubscriber {
    pub async fn new(config: Config) -> Result<Self, Error> {
        let Config {
            url,
            username,
            password,
            max_reconnects,
        } = config;

        let options = ConnectOptions::new()
            .user_and_password(username, password.expose_secret().to_owned())
            .max_reconnects(max_reconnects)
            .event_callback(|event| async {
                match event {
                    Event::Connected => info!("NATS client connected"),
                    Event::Disconnected => warn!("NATS client disconnected"),
                    Event::LameDuckMode => warn!("NATS client in lame duck mode"),
                    Event::Draining => warn!("NATS client draining"),
                    Event::Closed => warn!("NATS client closed"),
                    Event::SlowConsumer(_) => warn!("NATS client has slow consumer"),
                    Event::ServerError(error) => warn!(error:%; "NATS server error"),
                    Event::ClientError(error) => warn!(error:%; "NATS client error"),
                }
            });
        let client = options.connect(url).await?;

        Ok(Self { client })
    }
}

impl Subscriber for NatsSubscriber {
    type Error = SubscriberError;

    async fn subscribe<T>(&self) -> impl Stream<Item = Result<T, Self::Error>> + Send
    where
        T: Message,
    {
        // NATS registers a subscription when its command reaches the server; the flush round trip
        // waits for that, so every message published afterwards is delivered.
        let subscription = self
            .client
            .subscribe(T::TOPIC)
            .await
            .map_err(SubscriberError::Subscribe);
        if let Err(error) = self.client.flush().await {
            warn!(error:%; "cannot flush NATS subscription for topic {}", T::TOPIC);
        }

        // The subscriber stream may complete, without returning any error (it is not baked into the
        // item type), e.g. because of disconnecting. Therefore we follow it with an infinite
        // stream which is slightly throttled and resubscribes repeatedly.
        let resubscriptions = tokio_stream::StreamExt::throttle(stream::repeat(()), REPEAT_DELAY)
            .then(|_| {
                self.client
                    .subscribe(T::TOPIC)
                    .map_err(SubscriberError::Subscribe)
            });

        stream::iter([subscription])
            .chain(resubscriptions)
            .map_ok(|subscriber| {
                subscriber.map(|message| {
                    let message = serde_json::from_slice(&message.payload)?;
                    Ok(message)
                })
            })
            .try_flatten()
    }
}

#[derive(Debug, Error)]
#[error("cannot create NATS based subscriber")]
pub struct Error(#[from] async_nats::ConnectError);

#[derive(Debug, Error)]
pub enum SubscriberError {
    #[error("cannot subscribe")]
    Subscribe(#[from] async_nats::SubscribeError),

    #[error("cannot JSON deserialize message")]
    Deserialize(#[from] serde_json::Error),
}
