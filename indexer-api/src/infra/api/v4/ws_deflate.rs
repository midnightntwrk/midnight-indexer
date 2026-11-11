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

//! Opt-in deflate compression for GraphQL subscription WebSockets.
//!
//! Wire format for the `graphql-transport-ws+deflate` subprotocol:
//!
//! - Server to client: graphql-transport-ws JSON messages of at least `MIN_COMPRESS_LEN` (256)
//!   bytes are sent as binary frames containing the zlib-compressed (RFC 1950) UTF-8 JSON, which
//!   browsers decompress with the built-in `DecompressionStream('deflate')`. Smaller messages
//!   remain plain text frames, so clients must handle both frame types.
//! - Client to server: text frames carry plain JSON as usual; binary frames are treated as
//!   zlib-compressed JSON and are inflated subject to `MAX_INFLATED_LEN` (1 MiB).
//! - Control frames (ping/pong/close) are never compressed.
//!
//! Clients may offer `graphql-transport-ws+deflate` alone or alongside the standard protocols.
//! The deflate variant uses the `graphql-transport-ws` message format. Clients that do not offer
//! the deflate variant are byte-for-byte unaffected.
//!
//! Compression uses a fresh context per message, so there is no per-connection compressor state
//! and no shared-window memory cost per connection.

use async_graphql_axum::GraphQLProtocol;
use axum::{
    extract::{FromRequestParts, ws::Message},
    http::{HeaderValue, Request, StatusCode, header::SEC_WEBSOCKET_PROTOCOL, request::Parts},
};
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use futures::{Sink, Stream};
use std::{
    io::{Read, Write},
    pin::Pin,
    task::{Context, Poll},
};

/// The subprotocol identifier for compressed `graphql-transport-ws` messages.
pub const GRAPHQL_TRANSPORT_WS_DEFLATE: &str = "graphql-transport-ws+deflate";

/// GraphQL protocol extraction for the optional deflate subprotocol.
pub(super) struct DeflateGraphQLProtocol(pub GraphQLProtocol);

impl<S: Send + Sync> FromRequestParts<S> for DeflateGraphQLProtocol {
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let deflate = parts
            .headers
            .get_all(SEC_WEBSOCKET_PROTOCOL)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(','))
            .any(|protocol| protocol.trim() == GRAPHQL_TRANSPORT_WS_DEFLATE);

        if !deflate {
            return GraphQLProtocol::from_request_parts(parts, state)
                .await
                .map(Self);
        }

        // async-graphql only knows the standard protocols, so ask it for the one whose message
        // format deflate uses. A stand-in request keeps the original intact, so WebSocketUpgrade
        // still negotiates the deflate wire format.
        let (mut graphql_parts, ()) = Request::new(()).into_parts();
        graphql_parts.headers.insert(
            SEC_WEBSOCKET_PROTOCOL,
            HeaderValue::from_static("graphql-transport-ws"),
        );

        GraphQLProtocol::from_request_parts(&mut graphql_parts, state)
            .await
            .map(Self)
    }
}

/// Outbound messages shorter than this are left uncompressed: at this size the deflate overhead
/// outweighs the gain and the wire format permits mixing text and binary frames.
const MIN_COMPRESS_LEN: usize = 256;

/// Upper bound for inflated client-to-server payloads (decompression bomb guard); aligned with
/// the default HTTP `request_body_limit`.
const MAX_INFLATED_LEN: usize = 1024 * 1024;

/// Wraps a WebSocket such that outbound text messages are zlib-compressed into binary frames and
/// inbound binary frames are inflated back into text messages.
pub struct DeflateWebSocket<S> {
    inner: S,
}

impl<S> DeflateWebSocket<S> {
    pub fn new(inner: S) -> Self {
        Self { inner }
    }
}

impl<S> Stream for DeflateWebSocket<S>
where
    S: Stream<Item = Result<Message, axum::Error>> + Unpin,
{
    type Item = Result<Message, axum::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(Message::Binary(data)))) => Poll::Ready(Some(
                inflate_capped(&data)
                    .map(|text| Message::Text(text.into()))
                    .map_err(axum::Error::new),
            )),

            other => other,
        }
    }
}

impl<S> Sink<Message> for DeflateWebSocket<S>
where
    S: Sink<Message, Error = axum::Error> + Unpin,
{
    type Error = axum::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.inner).poll_ready(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, message: Message) -> Result<(), Self::Error> {
        let message = match message {
            Message::Text(text) if text.len() >= MIN_COMPRESS_LEN => {
                let compressed = compress(text.as_bytes()).map_err(axum::Error::new)?;
                Message::Binary(compressed.into())
            }

            other => other,
        };

        Pin::new(&mut self.inner).start_send(message)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.inner).poll_close(cx)
    }
}

#[derive(Debug, thiserror::Error)]
enum InflateError {
    #[error("cannot inflate WebSocket payload")]
    Inflate(#[from] std::io::Error),

    #[error("inflated WebSocket payload exceeds {MAX_INFLATED_LEN} bytes")]
    TooLarge,

    #[error("inflated WebSocket payload is not valid UTF-8")]
    Utf8(#[from] std::string::FromUtf8Error),
}

fn compress(payload: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(
        Vec::with_capacity(payload.len() / 4),
        Compression::default(),
    );
    encoder.write_all(payload)?;
    encoder.finish()
}

fn inflate_capped(payload: &[u8]) -> Result<String, InflateError> {
    let mut inflated = Vec::new();
    ZlibDecoder::new(payload)
        .take(MAX_INFLATED_LEN as u64 + 1)
        .read_to_end(&mut inflated)?;

    if inflated.len() > MAX_INFLATED_LEN {
        return Err(InflateError::TooLarge);
    }

    Ok(String::from_utf8(inflated)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::storage::NoopStorage,
        infra::api::v4::{graphql_ws, schema_builder},
    };
    use axum::{Extension, Router, routing::get};
    use futures::{SinkExt, StreamExt, stream};
    use indexer_common::domain::NoopSubscriber;
    use std::{
        net::{Ipv4Addr, SocketAddr},
        time::Duration,
    };
    use tokio::{
        io::{AsyncRead, AsyncWrite},
        net::{TcpListener, TcpStream},
        time::timeout,
    };
    use tokio_tungstenite::{
        MaybeTlsStream, WebSocketStream, connect_async,
        tungstenite::{
            self,
            Message::{Binary, Text},
            client::IntoClientRequest,
            handshake::client::Response,
        },
    };

    // How long a test waits for the server, so that a silent server fails the test at once.
    const REPLY_TIMEOUT: Duration = Duration::from_secs(10);

    #[tokio::test]
    async fn accepts_deflate_without_a_plain_protocol_offer() {
        let (mut parts, _) = Request::builder()
            .header(SEC_WEBSOCKET_PROTOCOL, GRAPHQL_TRANSPORT_WS_DEFLATE)
            .body(())
            .unwrap()
            .into_parts();
        let original = parts.headers.clone();

        let actual = DeflateGraphQLProtocol::from_request_parts(&mut parts, &())
            .await
            .expect("a proxy may forward only the negotiated deflate protocol");
        assert_eq!(parts.headers, original);

        parts.headers.insert(
            SEC_WEBSOCKET_PROTOCOL,
            "graphql-transport-ws".parse().unwrap(),
        );
        let expected = GraphQLProtocol::from_request_parts(&mut parts, &())
            .await
            .unwrap();
        assert_eq!(actual.0, expected);
    }

    #[tokio::test]
    async fn deflate_uses_the_modern_graphql_protocol_for_combined_offers() {
        for offer in [
            "graphql-transport-ws+deflate, graphql-transport-ws",
            "graphql-transport-ws, graphql-transport-ws+deflate",
            "graphql-ws, graphql-transport-ws+deflate",
        ] {
            let (mut parts, _) = Request::builder()
                .header(SEC_WEBSOCKET_PROTOCOL, offer)
                .body(())
                .unwrap()
                .into_parts();
            let original = parts.headers.clone();
            let actual = DeflateGraphQLProtocol::from_request_parts(&mut parts, &())
                .await
                .unwrap();
            assert_eq!(parts.headers, original);

            parts.headers.insert(
                SEC_WEBSOCKET_PROTOCOL,
                "graphql-transport-ws".parse().unwrap(),
            );
            let expected = GraphQLProtocol::from_request_parts(&mut parts, &())
                .await
                .unwrap();
            assert_eq!(actual.0, expected, "offer: {offer}");
        }
    }

    #[tokio::test]
    async fn accepts_deflate_in_a_repeated_protocol_header() {
        let (mut parts, _) = Request::builder()
            .header(SEC_WEBSOCKET_PROTOCOL, "graphql-ws")
            .header(SEC_WEBSOCKET_PROTOCOL, GRAPHQL_TRANSPORT_WS_DEFLATE)
            .body(())
            .unwrap()
            .into_parts();
        let original = parts.headers.clone();
        let actual = DeflateGraphQLProtocol::from_request_parts(&mut parts, &())
            .await
            .unwrap();
        assert_eq!(parts.headers, original);

        parts.headers.insert(
            SEC_WEBSOCKET_PROTOCOL,
            "graphql-transport-ws".parse().unwrap(),
        );
        let expected = GraphQLProtocol::from_request_parts(&mut parts, &())
            .await
            .unwrap();
        assert_eq!(actual.0, expected);
    }

    #[tokio::test]
    async fn standard_and_invalid_protocol_offers_keep_their_behavior() {
        for offer in [
            None,
            Some("graphql-transport-ws"),
            Some("graphql-ws"),
            Some("unknown"),
        ] {
            let mut request = Request::builder();
            if let Some(offer) = offer {
                request = request.header(SEC_WEBSOCKET_PROTOCOL, offer);
            }
            let (mut parts, _) = request.body(()).unwrap().into_parts();
            let original = parts.headers.clone();
            let expected = GraphQLProtocol::from_request_parts(&mut parts, &()).await;
            let actual = DeflateGraphQLProtocol::from_request_parts(&mut parts, &()).await;
            assert_eq!(actual.map(|protocol| protocol.0), expected);
            assert_eq!(parts.headers, original);
        }
    }

    #[test]
    fn compress_inflate_roundtrip() {
        let payload =
            r#"{"type":"next","id":"1","payload":{"data":{"blocks":{"height":42}}}}"#.repeat(10);
        let compressed = compress(payload.as_bytes()).unwrap();
        assert!(compressed.len() < payload.len());
        assert_eq!(inflate_capped(&compressed).unwrap(), payload);
    }

    #[test]
    fn inflate_rejects_oversized_payload() {
        let bomb = compress(&vec![0u8; MAX_INFLATED_LEN + 1]).unwrap();
        assert!(matches!(inflate_capped(&bomb), Err(InflateError::TooLarge)));
    }

    #[test]
    fn inflate_rejects_garbage() {
        assert!(matches!(
            inflate_capped(b"not zlib data"),
            Err(InflateError::Inflate(_))
        ));
    }

    #[test]
    fn inflate_rejects_invalid_utf8() {
        let compressed = compress(&[0xff, 0xfe, 0xfd]).unwrap();
        assert!(matches!(
            inflate_capped(&compressed),
            Err(InflateError::Utf8(_))
        ));
    }

    #[tokio::test]
    async fn sink_compresses_large_text_only() {
        let mut messages = Vec::new();
        {
            let sink = futures::sink::unfold((), |(), message: Message| {
                messages.push(message);
                async move { Ok::<_, axum::Error>(()) }
            });
            futures::pin_mut!(sink);

            let large = "x".repeat(MIN_COMPRESS_LEN);
            let small = "y".repeat(MIN_COMPRESS_LEN - 1);
            // DeflateWebSocket requires Unpin, the pinned unfold sink is used via &mut.
            let mut socket = DeflateWebSocket::new(&mut sink);
            socket
                .send(Message::Text(large.clone().into()))
                .await
                .unwrap();
            socket
                .send(Message::Text(small.clone().into()))
                .await
                .unwrap();

            let [first, second] = &messages[..] else {
                panic!("expected two messages");
            };
            match first {
                Message::Binary(data) => assert_eq!(inflate_capped(data).unwrap(), large),
                other => panic!("expected binary frame, got {other:?}"),
            }
            assert!(matches!(second, Message::Text(text) if text.as_str() == small));
        }
    }

    #[tokio::test]
    async fn stream_inflates_binary_and_passes_text_through() {
        let compressed = compress(b"inflated payload").unwrap();
        let inner = stream::iter([
            Ok::<_, axum::Error>(Message::Binary(compressed.into())),
            Ok(Message::Text("plain".into())),
        ]);
        futures::pin_mut!(inner);

        let mut socket = DeflateWebSocket::new(&mut inner);

        let first = socket.next().await.unwrap().unwrap();
        assert!(matches!(first, Message::Text(text) if text.as_str() == "inflated payload"));

        let second = socket.next().await.unwrap().unwrap();
        assert!(matches!(second, Message::Text(text) if text.as_str() == "plain"));
    }

    /// A proxy may forward only the subprotocol the server selected, so a handshake offering
    /// nothing but the deflate variant must be upgraded and must speak compressed
    /// `graphql-transport-ws`.
    #[tokio::test]
    async fn deflate_only_offer_is_upgraded_and_compressed() {
        let address = serve_graphql_ws_route().await;
        let (mut socket, response) = connect(address, Some(GRAPHQL_TRANSPORT_WS_DEFLATE))
            .await
            .expect("deflate-only offer is upgraded");
        assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
        assert_eq!(
            response.headers().get(SEC_WEBSOCKET_PROTOCOL),
            Some(&HeaderValue::from_static(GRAPHQL_TRANSPORT_WS_DEFLATE))
        );

        // Client-to-server: a compressed binary frame is inflated and understood as
        // `graphql-transport-ws`; the short acknowledgement stays an uncompressed text frame.
        let mut encoder = ZlibEncoder::new(vec![], Compression::default());
        encoder
            .write_all(br#"{"type":"connection_init"}"#)
            .expect("payload can be compressed");
        let connection_init = encoder.finish().expect("payload can be compressed");
        socket
            .send(Binary(connection_init.into()))
            .await
            .expect("connection_init can be sent");
        let ack = next_message(&mut socket).await;
        assert_eq!(ack, Text(r#"{"type":"connection_ack"}"#.into()));

        // Server-to-client: a response above the compression threshold arrives as a compressed
        // binary frame.
        let subscribe = serde_json::json!({
            "id": "1",
            "type": "subscribe",
            "payload": { "query": "{ __schema { types { name } } }" },
        });
        socket
            .send(Text(subscribe.to_string().into()))
            .await
            .expect("subscribe can be sent");
        let next = next_message(&mut socket).await;
        let Binary(compressed) = next else {
            panic!("expected a compressed binary frame, got {next:?}");
        };
        let mut inflated = String::new();
        ZlibDecoder::new(compressed.as_ref())
            .read_to_string(&mut inflated)
            .expect("frame is zlib-compressed UTF-8");
        let next =
            serde_json::from_str::<serde_json::Value>(&inflated).expect("inflated frame is JSON");
        assert_eq!(next["type"], "next");
        assert_eq!(next["id"], "1");
        assert!(next["payload"]["data"]["__schema"]["types"].is_array());
    }

    #[tokio::test]
    async fn accepts_deflate_next_to_an_unknown_protocol() {
        for offer in [
            "unknown, graphql-transport-ws+deflate",
            "graphql-transport-ws+deflate, unknown",
        ] {
            let actual = extract(&[HeaderValue::from_static(offer)]).await;
            assert_eq!(actual, graphql_transport_ws().await, "offer: {offer}");
        }
    }

    #[tokio::test]
    async fn near_misses_of_the_deflate_protocol_are_not_rewritten() {
        for offer in [
            "graphql-transport-ws+deflate2",
            "graphql-transport-ws+",
            "GRAPHQL-TRANSPORT-WS+DEFLATE",
            "graphql-ws, graphql-transport-ws+deflate2",
        ] {
            let offers = [HeaderValue::from_static(offer)];
            let actual = extract(&offers).await;
            assert_eq!(actual, extract_standard(&offers).await, "offer: {offer}");
        }
    }

    #[tokio::test]
    async fn rejects_a_non_utf8_protocol_header() {
        let offer = HeaderValue::from_bytes(b"graphql-transport-ws+deflate, \xff")
            .expect("opaque bytes are a valid header value");
        assert_eq!(extract(&[offer]).await, Err(StatusCode::BAD_REQUEST));
    }

    /// Serves the GraphQL WebSocket route and returns its address.
    async fn serve_graphql_ws_route() -> SocketAddr {
        let schema = schema_builder::<NoopStorage, NoopSubscriber>().finish();
        let app = Router::new()
            .route(
                "/graphql/ws",
                get(graphql_ws::<NoopStorage, NoopSubscriber>),
            )
            .layer(Extension(schema));
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("listener can be bound");
        let address = listener.local_addr().expect("listener has a local address");
        tokio::spawn(async move { axum::serve(listener, app).await });
        address
    }

    /// Opens a WebSocket, offering the given `Sec-WebSocket-Protocol` if any.
    async fn connect(
        address: SocketAddr,
        offer: Option<&'static str>,
    ) -> Result<(WebSocketStream<MaybeTlsStream<TcpStream>>, Response), tungstenite::Error> {
        let mut request = format!("ws://{address}/graphql/ws")
            .into_client_request()
            .expect("request is valid");
        if let Some(offer) = offer {
            request
                .headers_mut()
                .insert(SEC_WEBSOCKET_PROTOCOL, HeaderValue::from_static(offer));
        }
        timeout(REPLY_TIMEOUT, connect_async(request))
            .await
            .expect("server answers the handshake in time")
    }

    /// The next message from the server.
    async fn next_message<S>(socket: &mut WebSocketStream<S>) -> tungstenite::Message
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        timeout(REPLY_TIMEOUT, socket.next())
            .await
            .expect("server replies in time")
            .expect("connection is open")
            .expect("reply is a valid message")
    }

    /// A request offering one `Sec-WebSocket-Protocol` header per given value.
    fn parts(offers: &[HeaderValue]) -> Parts {
        let (mut parts, ()) = Request::new(()).into_parts();
        parts.headers.extend(
            offers
                .iter()
                .cloned()
                .map(|offer| (SEC_WEBSOCKET_PROTOCOL, offer)),
        );
        parts
    }

    /// The protocol extracted for the given offers, checking that the request is left intact
    /// for WebSocketUpgrade.
    async fn extract(offers: &[HeaderValue]) -> Result<GraphQLProtocol, StatusCode> {
        let mut parts = parts(offers);
        let original = parts.headers.clone();
        let protocol = DeflateGraphQLProtocol::from_request_parts(&mut parts, &())
            .await
            .map(|protocol| protocol.0);
        assert_eq!(parts.headers, original);
        protocol
    }

    /// The protocol async-graphql itself extracts for the given offers.
    async fn extract_standard(offers: &[HeaderValue]) -> Result<GraphQLProtocol, StatusCode> {
        GraphQLProtocol::from_request_parts(&mut parts(offers), &()).await
    }

    /// The protocol whose message format deflate uses.
    async fn graphql_transport_ws() -> Result<GraphQLProtocol, StatusCode> {
        let protocol = extract_standard(&[HeaderValue::from_static("graphql-transport-ws")]).await;
        assert!(protocol.is_ok());
        protocol
    }
}
