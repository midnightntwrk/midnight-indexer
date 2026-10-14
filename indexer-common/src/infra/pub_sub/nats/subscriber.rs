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
use async_nats::{
    Client, ConnectOptions, Event, Request, SubscribeError,
    client::{RequestErrorKind, SubscribeErrorKind},
};
use futures::{Stream, StreamExt, TryFutureExt, TryStreamExt, stream};
use log::{debug, info, warn};
use secrecy::ExposeSecret;
use std::time::Duration;
use thiserror::Error;

// Bounds the round trip that confirms a subscription, so an unreachable server delays each
// subscription by at most this long.
pub(super) const CONFIRM_TIMEOUT: Duration = Duration::from_secs(1);
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
        let mut subscription = self
            .client
            .subscribe(T::TOPIC)
            .await
            .map_err(SubscriberError::Subscribe);

        // The server delivers only messages published after it has registered the subscription, and
        // this method promises every message published once it returns. The server processes a
        // connection's commands in order and answers a request to a subject without subscribers
        // with "no responders", so that answer confirms the subscription above. `Client::flush`
        // cannot confirm it: the Rust client's flush resolves once the bytes reach the socket,
        // without waiting for the server's PONG, unlike other NATS clients. See "Flush" in
        // https://docs.nats.io/learn/resilient-clients/drain-and-shutdown.
        match self
            .client
            .send_request(
                self.client.new_inbox(),
                Request::new().timeout(Some(CONFIRM_TIMEOUT)),
            )
            .await
        {
            // The expected answer: the server processed the SUB and found no subscriber to the inbox.
            Err(error) if error.kind() == RequestErrorKind::NoResponders => {
                debug!("confirmed NATS subscription for topic {}", T::TOPIC);
            }
            // Any reply also completes a round trip.
            Ok(_) => {
                debug!(
                    "confirmed NATS subscription for topic {} by a reply",
                    T::TOPIC
                );
            }
            // An unconfirmed subscription may miss messages, so it fails like an unestablished one.
            Err(error) => {
                subscription = Err(SubscriberError::Subscribe(SubscribeError::with_source(
                    SubscribeErrorKind::Other,
                    error,
                )));
            }
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
