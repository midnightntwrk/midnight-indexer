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

use anyhow::{anyhow, bail, Context};
use futures::{
    stream::{SplitSink, SplitStream},
    SinkExt, Stream, StreamExt, TryStreamExt,
};
use graphql_client::QueryBody;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::LazyLock;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
    MaybeTlsStream, WebSocketStream,
};

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsWrite = SplitSink<WsStream, Message>;
type WsRead = SplitStream<WsStream>;

// const WS_URL: &str = "ws://127.0.0.1:8088/api/v1/graphql/ws";

static CONNECTION_INIT: LazyLock<String> = LazyLock::new(|| {
    json!({
        "type": "connection_init",
    })
    .to_string()
});

pub struct GraphQlWsClient {
    write: WsWrite,
    read: WsRead,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMessage<T> {
    #[serde(rename = "next")]
    Next { id: String, payload: Payload<T> },

    #[serde(rename = "complete")]
    Complete { id: String },

    #[serde(rename = "connection_error")]
    ConnectionError { payload: serde_json::Value },
}

#[derive(Debug, Deserialize)]
pub struct Payload<T> {
    pub data: Option<T>,
    pub errors: Option<Vec<PayloadError>>,
}

#[derive(Debug, Deserialize)]
pub struct PayloadError {
    #[allow(dead_code)]
    pub message: String,
}

impl GraphQlWsClient {
    /// Connect to the given WebSocket URL and establishes the GraphQL WebSocket connection.
    pub async fn init<T: DeserializeOwned>(url: &str) -> anyhow::Result<Self> {
        let ws_stream = connect_websocket(url).await?;
        let (write, read) = ws_stream.split();
        let client = establish_connection::<T>(write, read).await?;

        Ok(client)
    }

    /// Send a subscription message for the given subscription.
    pub async fn subscribe<T>(&mut self, subscription: QueryBody<T>) -> anyhow::Result<()>
    where
        T: Serialize,
    {
        let variables =
            serde_json::to_value(subscription.variables).context("serialize variables")?;

        let subscribe_message = json!({
            "type": "subscribe",
            "id": "1",
            "payload": {
                "operationName": subscription.operation_name,
                "query": subscription.query,
                "variables": variables,
            }
        });

        self.write
            .send(Message::text(subscribe_message.to_string()))
            .await
            .context("send subscribe message")?;

        Ok(())
    }

    pub async fn messages<T>(self) -> impl Stream<Item = anyhow::Result<ServerMessage<T>>>
    where
        T: for<'de> Deserialize<'de>,
    {
        self.read.map(|result| {
            result
                .context("get next message")
                .and_then(|message| match message {
                    Message::Text(text) => serde_json::from_str::<ServerMessage<T>>(&text)
                        .context("deserialize text message to ServerMessage"),

                    _ => Err(anyhow!("unexpected non-text message")),
                })
        })
    }
}

/// Connect to the given WebSocket URL and return the WebSocket stream.
async fn connect_websocket(url: &str) -> anyhow::Result<WsStream> {
    let mut request = url
        .into_client_request()
        .context("convert url into client request")?;

    // Insert the GraphQL WebSocket subprotocol.
    let graphql_transport_ws = "graphql-transport-ws"
        .parse()
        .context("parse graphql-transport-ws as header value")?;
    request
        .headers_mut()
        .insert("Sec-WebSocket-Protocol", graphql_transport_ws);

    // Connect to the WebSocket server
    let (ws_stream, _) = connect_async(request)
        .await
        .context("connect to WebSocket")?;

    Ok(ws_stream)
}

/// Establish the GraphQL WebSocket connection by performing the handshake.
pub async fn establish_connection<T: DeserializeOwned>(
    mut write: WsWrite,
    mut read: WsRead,
) -> anyhow::Result<GraphQlWsClient> {
    // Send the connection_init message.
    write
        .send(Message::text(&*CONNECTION_INIT))
        .await
        .context("send connection_init")?;

    // Await  the connection_ack message.
    let message = read.try_next().await.context("read WebSocket message")?;
    let Some(message) = message else {
        bail!("not received any message while awaiting connection_ack");
    };
    let Message::Text(message) = message else {
        bail!("not received text message for connection_ack");
    };
    let message = serde_json::from_str::<Value>(&message).context("parse text message as JSON")?;
    let Value::String(tpe) = &message["type"] else {
        bail!("not received JSON object with string 'type' key");
    };
    if tpe != "connection_ack" {
        bail!("not received connection_ack");
    }

    Ok(GraphQlWsClient { write, read })
}
