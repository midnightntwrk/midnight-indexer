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

use anyhow::{Context, anyhow, bail};
use derive_more::Display;
use futures::{
    SinkExt, Stream, StreamExt, TryStreamExt,
    stream::{SplitSink, SplitStream, unfold},
};
use graphql_client::{GraphQLQuery, QueryBody};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::LazyLock;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

type WsWrite = SplitSink<WsStream, Message>;
type WsRead = SplitStream<WsStream>;
type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

static CONNECTION_INIT: LazyLock<String> = LazyLock::new(|| {
    json!({
        "type": "connection_init",
    })
    .to_string()
});

static PONG: LazyLock<String> = LazyLock::new(|| {
    json!({
        "type": "pong",
    })
    .to_string()
});

/// Subscribe to the given GraphQL Websocket URL (typically ending with /graphql/ws) and
/// query variables.
pub async fn subscribe<T>(
    url: &str,
    variables: T::Variables,
) -> anyhow::Result<impl Stream<Item = anyhow::Result<T::ResponseData>>>
where
    T: GraphQLQuery,
{
    let QueryBody {
        variables,
        query,
        operation_name,
    } = T::build_query(variables);
    let variables = serde_json::to_value(variables).context("serialize query variables")?;

    let data = subscribe_raw(url, operation_name, query, variables).await?;

    Ok(data.map(|data| {
        data.and_then(|data| {
            serde_json::from_value::<T::ResponseData>(data).context("deserialize response data")
        })
    }))
}

/// Subscribe with a query document given as text, yielding the untyped `data` of every
/// received message.
///
/// The typed [subscribe] above is the default. This one exists for documents that cannot be
/// generated from `e2e.graphql`, in particular one selecting a field the build under test may
/// not serve: `e2e.graphql` is shared by every typed operation, so a field that has to be
/// probed before it is requested cannot live in it.
///
/// `use<>`: the stream borrows nothing — the arguments are consumed into the subscribe message
/// before the socket is read — so it must not capture their lifetimes, or callers cannot hold it
/// past the call (e.g. move it into a spawned task).
pub async fn subscribe_raw(
    url: &str,
    operation_name: &str,
    query: &str,
    variables: Value,
) -> anyhow::Result<impl Stream<Item = anyhow::Result<Value>> + use<>> {
    let ws_stream = connect_graphql_ws(url)
        .await
        .context("connect graphql websocket connection")?;

    let (mut write, mut read) = ws_stream.split();

    init_graphql_ws(&mut write, &mut read)
        .await
        .context("initialize graphql websocket connection")?;

    let subscribe_message = json!({
        "type": "subscribe",
        "id": "1",
        "payload": {
            "operationName": operation_name,
            "query": query,
            "variables": variables,
        }
    });

    write
        .send(Message::text(subscribe_message.to_string()))
        .await
        .context("send subscribe message")?;

    // The write half is carried alongside the read half rather than dropped here: a
    // `ping` has to be answered with a `pong`, and a dropped sink cannot answer. A
    // `None` next-state ends the stream, so an error is terminal, as it was when this
    // was a `try_filter_map`.
    let messages = unfold(Some((read, write)), |state| async move {
        let (mut read, mut write) = state?;

        loop {
            let text = match read.next().await {
                Some(Ok(Message::Text(text))) => text,

                // Transport-level frames carry nothing for a subscriber. Keepalives and
                // the peer's own pongs are handled by tungstenite; ignore and read on.
                Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => continue,

                Some(Ok(Message::Close(_))) => return None,

                Some(Ok(message)) => {
                    return Some((Err(anyhow!("unexpected message: {message:?}")), None));
                }

                Some(Err(error)) => {
                    return Some((Err(anyhow!(error).context("get next message")), None));
                }

                None => return None,
            };

            let message = match serde_json::from_str::<ServerMessage>(&text) {
                Ok(message) => message,

                // Deliberately loud: an unrecognised `type` means the server speaks a
                // protocol this client does not, which a subscriber must not silently
                // read as "no data".
                Err(error) => {
                    return Some((
                        Err(anyhow!(error)
                            .context(format!("deserialize text message to ServerMessage: {text}"))),
                        None,
                    ));
                }
            };

            match message {
                ServerMessage::Next { payload } => match (payload.data, payload.errors) {
                    (Some(data), None) => return Some((Ok(data), Some((read, write)))),

                    (None, Some(errors)) => {
                        let errors = errors
                            .iter()
                            .map(|e| e.message.to_owned())
                            .collect::<Vec<_>>()
                            .join(", ");
                        return Some((Err(anyhow!(errors)), None));
                    }

                    _ => {
                        return Some((Err(anyhow!("unexpected GraphQL execution result")), None));
                    }
                },

                ServerMessage::Complete => return None,

                ServerMessage::Error { payload } => return Some((Err(anyhow!(payload)), None)),

                ServerMessage::Ping => {
                    if let Err(error) = write.send(Message::text(&*PONG)).await {
                        return Some((Err(anyhow!(error).context("send pong")), None));
                    }
                }

                ServerMessage::Pong => {}
            }
        }
    });

    Ok(messages)
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum ServerMessage {
    Next {
        payload: ExecutionResult,
    },
    Complete,
    Error {
        payload: Value,
    },

    /// Keepalives from `graphql-transport-ws`. Either side may send `ping` at any time and the
    /// receiver answers `pong`; a subscription held open for minutes has to tolerate both rather
    /// than fail on them. Every *other* `type` still fails to deserialize, and so fails loudly.
    Ping,
    Pong,
}

#[derive(Debug, Deserialize)]
pub struct ExecutionResult {
    pub data: Option<Value>,
    pub errors: Option<Vec<GraphQLError>>,
}

#[derive(Debug, Deserialize, Display)]
#[display("{message}")]
pub struct GraphQLError {
    pub message: String,
}

/// Connect to the given WebSocket URL and return the WebSocket stream.
async fn connect_graphql_ws(url: &str) -> anyhow::Result<WsStream> {
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

    // Connect to the WebSocket server.
    let (ws_stream, _) = connect_async(request)
        .await
        .context("connect to WebSocket server")?;

    Ok(ws_stream)
}

/// Establish the GraphQL WebSocket connection by performing the handshake.
pub async fn init_graphql_ws(write: &mut WsWrite, read: &mut WsRead) -> anyhow::Result<()> {
    // Send the connection_init message.
    write
        .send(Message::text(&*CONNECTION_INIT))
        .await
        .context("send connection_init")?;

    // Await  the connection_ack message.
    let Some(message) = read.try_next().await.context("read WebSocket message")? else {
        bail!("WebSocket connection closed while awaiting connection_ack");
    };

    let Message::Text(message) = message else {
        bail!("received non-text message for connection_ack");
    };

    let message = serde_json::from_str::<Value>(&message).context("parse text message as JSON")?;

    let Value::String(tpe) = &message["type"] else {
        bail!("not received JSON object with string 'type' key");
    };

    if tpe != "connection_ack" {
        bail!("not received connection_ack");
    }

    Ok(())
}
