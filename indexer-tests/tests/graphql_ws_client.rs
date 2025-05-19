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

#![cfg(feature = "cloud")]

use anyhow::{anyhow, bail, Context};
use futures::{SinkExt, StreamExt};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
    MaybeTlsStream, WebSocketStream,
};

const PROTOCOL: &str = "graphql-transport-ws";

type WsStream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
type WsWrite = futures::stream::SplitSink<WsStream, Message>;
type WsRead = futures::stream::SplitStream<WsStream>;

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum GraphQLWSMessage<T> {
    #[serde(rename = "connection_ack")]
    ConnectionAck,
    #[serde(rename = "next")]
    Next {
        id: String,
        payload: GraphQLPayload<T>,
    },
    #[serde(rename = "complete")]
    Complete { id: String },
    #[serde(rename = "connection_error")]
    ConnectionError { payload: serde_json::Value },
}

#[derive(Debug, Deserialize)]
pub struct GraphQLPayload<T> {
    pub data: Option<T>,
    pub errors: Option<Vec<GraphQLError>>,
}

#[derive(Debug, Deserialize)]
pub struct GraphQLError {
    #[allow(dead_code)]
    pub message: String,
}

pub struct GraphQLWSClient {
    write: WsWrite,
    read: WsRead,
}

impl GraphQLWSClient {
    /// Connects to the WebSocket server and establishes the GraphQL WebSocket connection.
    pub async fn connect_and_establish<T: DeserializeOwned>(port: u16) -> anyhow::Result<Self> {
        let ws_url = format!("ws://127.0.0.1:{}/api/v1/graphql/ws", port);
        let ws_stream = Self::connect_websocket(&ws_url, PROTOCOL).await?;
        let (write, read) = ws_stream.split();
        let client = Self::establish_connection::<T>(write, read).await?;
        Ok(client)
    }

    /// Connects to the WebSocket server and returns the WebSocket stream.
    pub async fn connect_websocket(ws_url: &str, protocol: &str) -> anyhow::Result<WsStream> {
        let mut request = ws_url
            .into_client_request()
            .context("Failed to create WebSocket request")?;

        // Insert the GraphQL WebSocket subprotocol
        request
            .headers_mut()
            .insert("Sec-WebSocket-Protocol", protocol.parse()?);

        // Connect to the WebSocket server
        let (ws_stream, _) = connect_async(request)
            .await
            .context("Failed to connect to WebSocket")?;

        Ok(ws_stream)
    }

    /// Establishes the GraphQL WebSocket connection by performing the handshake.
    pub async fn establish_connection<T: DeserializeOwned>(
        mut write: WsWrite,
        mut read: WsRead,
    ) -> anyhow::Result<GraphQLWSClient> {
        // Send the connection_init message
        let connection_init = json!({
            "type": "connection_init",
            "payload": {}
        });
        write
            .send(Message::Text(connection_init.to_string()))
            .await
            .context("Failed to send connection_init")?;

        // Await the connection_ack message
        loop {
            if let Some(msg) = read.next().await {
                let msg = msg.context("Failed to read WebSocket message during connection_ack")?;
                if let Message::Text(text) = msg {
                    let response: GraphQLWSMessage<T> =
                        serde_json::from_str(&text).context("Failed to parse GraphQLWSMessage")?;
                    match response {
                        GraphQLWSMessage::ConnectionAck => {
                            break;
                        }
                        GraphQLWSMessage::ConnectionError { payload } => {
                            bail!("Connection error: {:?}", payload);
                        }
                        _ => {
                            continue;
                        }
                    }
                }
            } else {
                bail!("WebSocket connection closed before receiving connection_ack");
            }
        }

        Ok(GraphQLWSClient { write, read })
    }

    /// Sends a subscription message for a given subscription.
    pub async fn send_subscription(
        &mut self,
        subscription_query: &str,
        variables: serde_json::Value,
        operation_name: &str,
        subscription_id: &str,
    ) -> anyhow::Result<()> {
        let subscribe_message = json!({
            "id": subscription_id,
            "type": "subscribe",
            "payload": {
                "query": subscription_query,
                "variables": variables,
                "operationName": operation_name
            }
        });

        self.write
            .send(Message::Text(subscribe_message.to_string()))
            .await
            .context("Failed to send subscribe message")?;

        sleep(Duration::from_millis(50)).await;

        Ok(())
    }

    /// Receives messages from the WebSocket and processes them.
    pub async fn receive_messages<F, T: DeserializeOwned>(
        &mut self,
        expected_message_count: usize,
        process_message: F,
    ) -> anyhow::Result<Vec<T>>
    where
        F: Fn(&GraphQLWSMessage<T>) -> Option<T>,
    {
        let mut received = Vec::new();
        while let Some(msg) = self.read.next().await {
            let msg = msg.map_err(|e| {
                anyhow!(
                    "Failed to read WebSocket message during subscription: {}",
                    e
                )
            })?;
            if let Message::Text(text) = msg {
                let response: GraphQLWSMessage<T> =
                    serde_json::from_str(&text).context("Failed to parse GraphQLWSMessage")?;

                match &response {
                    GraphQLWSMessage::Next { id: _id, payload } => {
                        if let Some(errors) = &payload.errors {
                            bail!("GraphQL errors: {:?}", errors);
                        }
                        if let Some(_data) = &payload.data {
                            if let Some(parsed_data) = process_message(&response) {
                                received.push(parsed_data);
                                if received.len() >= expected_message_count {
                                    break;
                                }
                            }
                        }
                    }
                    GraphQLWSMessage::Complete { id: _id } => {
                        break;
                    }
                    GraphQLWSMessage::ConnectionError { payload } => {
                        bail!("Connection error: {:?}", payload);
                    }
                    _ => {}
                }
            }
        }
        Ok(received)
    }
}
