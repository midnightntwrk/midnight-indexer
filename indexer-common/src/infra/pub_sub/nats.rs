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

use crate::domain::Topic;
use async_nats::{Subject, subject::ToSubject};
use secrecy::SecretString;
use serde::Deserialize;

/// Configuration for NATS pub-sub.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub url: String,
    pub username: String,
    pub password: SecretString,
    pub max_reconnects: usize,
}

// Topic variant names are the subject suffix, so renaming one changes the wire format.
impl ToSubject for Topic {
    fn to_subject(&self) -> Subject {
        format!("pub-sub.{self}").into()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        domain::{Publisher, Subscriber, Topic, WalletIndexed},
        error::BoxError,
        infra::pub_sub::nats::{
            Config,
            publisher::NatsPublisher,
            subscriber::{CONFIRM_TIMEOUT, NatsSubscriber, SubscriberError},
        },
    };
    use anyhow::Context;
    use async_nats::subject::ToSubject;
    use futures::{StreamExt, TryStreamExt};
    use std::{
        pin::pin,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };
    use testcontainers::{
        ContainerAsync, GenericImage, ImageExt, core::WaitFor, runners::AsyncRunner,
    };
    use tokio::{
        io::{self, AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        task,
        time::{sleep, timeout},
    };
    use uuid::Uuid;

    /// Pins the subject wire format, which the `Display` impl of `Topic` determines. The match is
    /// exhaustive over every variant, so a new topic does not compile until its subject is pinned.
    #[test]
    fn test_topic_to_subject() {
        use Topic::*;
        for &topic in Topic::VARIANTS {
            let expected = match topic {
                BlockIndexed => "pub-sub.BlockIndexed",
                WalletIndexed => "pub-sub.WalletIndexed",
                UnshieldedUtxoIndexed => "pub-sub.UnshieldedUtxoIndexed",
                BridgeEventIndexed => "pub-sub.BridgeEventIndexed",
            };

            assert_eq!(topic.to_subject().as_str(), expected);
        }
    }

    #[tokio::test]
    async fn test_publish_subscribe() -> Result<(), BoxError> {
        let (_nats_container, config) = start_nats().await?;

        let subscriber = NatsSubscriber::new(config.clone())
            .await
            .context("create NatsSubscriber")?;
        let wallet_indexed_messages = subscriber.subscribe::<WalletIndexed>().await;

        let wallet_indexed = WalletIndexed {
            wallet_id: Uuid::nil(),
        };
        let message_received = Arc::new(AtomicBool::new(false));

        task::spawn({
            let publisher = NatsPublisher::new(config)
                .await
                .context("create NatsPublisher")?;
            let message_received = message_received.clone();
            async move {
                while !message_received.load(Ordering::Relaxed) {
                    sleep(Duration::from_millis(100)).await;
                    publisher
                        .publish(&wallet_indexed)
                        .await
                        .expect("can publish");
                }
            }
        });

        let messages = wallet_indexed_messages
            .take(1)
            .try_collect::<Vec<_>>()
            .await
            .context("collect messages")?;
        message_received.store(true, Ordering::Relaxed);
        assert_eq!(messages, [wallet_indexed]);

        Ok(())
    }

    /// A message published on another connection right after `subscribe` returns is delivered,
    /// even when the subscriber's commands reach the server late, as over a slow link, and before
    /// the stream is first polled. One published before `subscribe` is not.
    #[tokio::test]
    async fn test_message_published_before_first_poll_is_delivered() -> Result<(), BoxError> {
        let (nats_container, config) = start_nats().await?;
        let nats_port = nats_container
            .get_host_port_ipv4(4222)
            .await
            .context("get NATS port")?;
        let proxy_port = start_delaying_proxy(nats_port, Duration::from_millis(200)).await?;

        let subscriber = NatsSubscriber::new(Config {
            url: format!("localhost:{proxy_port}"),
            ..config.clone()
        })
        .await
        .context("create NatsSubscriber")?;
        let publisher = NatsPublisher::new(config)
            .await
            .context("create NatsPublisher")?;

        let before_subscribe = WalletIndexed {
            wallet_id: Uuid::from_u128(1),
        };
        publisher
            .publish(&before_subscribe)
            .await
            .context("publish before subscribe")?;

        let wallet_indexed_messages = subscriber.subscribe::<WalletIndexed>().await;
        let after_subscribe = WalletIndexed {
            wallet_id: Uuid::from_u128(2),
        };
        publisher
            .publish(&after_subscribe)
            .await
            .context("publish after subscribe")?;

        // Both messages travel on the publisher's connection in order, so had the first been
        // delivered, it would be received first.
        let messages = timeout(
            Duration::from_secs(2),
            wallet_indexed_messages.take(1).try_collect::<Vec<_>>(),
        )
        .await
        .context("no message within 2s")?
        .context("collect messages")?;
        assert_eq!(messages, [after_subscribe]);

        Ok(())
    }

    /// `subscribe` returns shortly after its confirmation timeout when the server does not answer,
    /// and the stream's first item is an error.
    #[tokio::test]
    async fn test_subscribe_returns_when_server_does_not_answer() -> Result<(), BoxError> {
        let (nats_container, config) = start_nats().await?;
        let subscriber = NatsSubscriber::new(config)
            .await
            .context("create NatsSubscriber")?;

        nats_container.pause().await.context("pause NATS")?;
        let start = Instant::now();
        let wallet_indexed_messages = subscriber.subscribe::<WalletIndexed>().await;
        let elapsed = start.elapsed();
        nats_container.unpause().await.context("unpause NATS")?;

        assert!(
            elapsed < CONFIRM_TIMEOUT + Duration::from_millis(100),
            "subscribe took {elapsed:?}"
        );
        let first = timeout(
            Duration::from_millis(100),
            pin!(wallet_indexed_messages).next(),
        )
        .await;
        assert!(
            matches!(first, Ok(Some(Err(SubscriberError::Subscribe(_))))),
            "first item is {first:?}"
        );

        Ok(())
    }

    /// Starts a TCP proxy to `server_port` that holds each chunk a client sends for `delay` before
    /// forwarding it, and returns the proxy's port.
    async fn start_delaying_proxy(server_port: u16, delay: Duration) -> Result<u16, BoxError> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .context("bind proxy")?;
        let port = listener.local_addr().context("get proxy address")?.port();

        task::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                let Ok(server) = TcpStream::connect(("127.0.0.1", server_port)).await else {
                    continue;
                };
                let (mut client_reader, mut client_writer) = client.into_split();
                let (mut server_reader, mut server_writer) = server.into_split();

                task::spawn(async move {
                    let _ = io::copy(&mut server_reader, &mut client_writer).await;
                });
                task::spawn(async move {
                    let mut buffer = vec![0; 64 * 1024];
                    while let Ok(read) = client_reader.read(&mut buffer).await
                        && read > 0
                    {
                        sleep(delay).await;
                        if server_writer.write_all(&buffer[..read]).await.is_err() {
                            break;
                        }
                    }
                });
            }
        });

        Ok(port)
    }

    async fn start_nats() -> Result<(ContainerAsync<GenericImage>, Config), BoxError> {
        let nats_container = GenericImage::new("nats", "2.12.3")
            .with_wait_for(WaitFor::message_on_stderr("Server is ready"))
            .with_cmd([
                "--user",
                "indexer",
                "--pass",
                env!("APP__INFRA__PUB_SUB__PASSWORD"),
                "--http_port",
                "8222",
            ])
            .start()
            .await
            .context("start NATS container")?;

        // In spite of the above "WaitFor" NATS stubbornly rejects connections.
        let monitoring_port = nats_container
            .get_host_port_ipv4(8222)
            .await
            .context("get NATS monitoring port")?;
        let health_url = format!("http://localhost:{monitoring_port}/healthz");
        let start = Instant::now();
        while reqwest::get(&health_url)
            .await
            .and_then(|r| r.error_for_status())
            .is_err()
            && Instant::now() - start < Duration::from_millis(1_500)
        {
            sleep(Duration::from_millis(100)).await;
        }

        let nats_port = nats_container
            .get_host_port_ipv4(4222)
            .await
            .context("get NATS port")?;
        let config = Config {
            url: format!("localhost:{nats_port}"),
            username: "indexer".to_string(),
            password: env!("APP__INFRA__PUB_SUB__PASSWORD").into(),
            max_reconnects: 1,
        };

        Ok((nats_container, config))
    }
}
