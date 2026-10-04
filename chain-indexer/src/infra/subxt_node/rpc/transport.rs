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

//! The [Transport] seam and its jsonrpsee implementation, [WsTransport].

use futures::{StreamExt, TryStreamExt, stream::BoxStream};
use http::HeaderMap;
use indexer_common::error::BoxError;
use jsonrpsee::{
    core::{
        client::{
            BatchResponse, ClientT, Error as ClientError, SubscriptionClientT, SubscriptionKind,
        },
        params::{ArrayParams, BatchRequestBuilder},
    },
    ws_client::{WsClient, WsClientBuilder},
};
use serde_json::Value;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

/// One JSON-RPC method call.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub method: &'static str,
    pub params: Vec<Value>,
}

/// The outcome of one call: its result, or the node's JSON-RPC error for it.
pub type CallResult = Result<Value, CallError>;

/// A JSON-RPC error object returned for a single call.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("JSON-RPC error {code}: {message}")]
pub struct CallError {
    pub code: i32,
    pub message: String,
}

/// A subscription's notifications.
pub type Notifications = BoxStream<'static, Result<Value, TransportError>>;

/// A subscription: its ID, as the node names it in follow-up calls, and its notifications.
pub struct Subscription {
    pub id: Value,
    pub notifications: Notifications,
}

/// Error of a [Transport].
#[derive(Debug, Error)]
pub enum TransportError {
    /// The connection to the node is lost.
    #[error("node connection lost")]
    Disconnected(#[source] BoxError),
    /// The node did not answer within the request timeout.
    #[error("no answer from the node in time")]
    Timeout(#[source] BoxError),
    /// Any other failure, e.g. a request or response the node or a proxy refuses.
    #[error(transparent)]
    Other(BoxError),
}

/// JSON-RPC transport to a node.
#[trait_variant::make(Send)]
pub trait Transport: Send + Sync + 'static {
    /// Send the calls as one JSON-RPC batch; one [CallResult] per call, in order.
    async fn batch(&self, calls: Vec<Call>) -> Result<Vec<CallResult>, TransportError>;

    /// Subscribe with the given method and parameters.
    async fn subscribe(
        &self,
        method: &'static str,
        params: Vec<Value>,
        unsubscribe: &'static str,
    ) -> Result<Subscription, TransportError>;

    /// Replace a lost connection with a new one.
    async fn reconnect(&self) -> Result<(), TransportError>;
}

/// [Transport] over one jsonrpsee WebSocket connection, carrying batches and subscriptions alike.
pub struct WsTransport {
    url: String,
    headers: HeaderMap,
    client: RwLock<Arc<WsClient>>,
}

impl WsTransport {
    /// Largest request or response, matching subxt's reconnecting client.
    const MAX_MESSAGE_SIZE: u32 = 50 * 1024 * 1024;

    /// Connect to the node at the given URL.
    pub async fn new(url: impl Into<String>, headers: HeaderMap) -> Result<Self, TransportError> {
        let url = url.into();
        let client = Self::connect(&url, &headers).await?;

        Ok(Self {
            url,
            headers,
            client: RwLock::new(Arc::new(client)),
        })
    }

    async fn connect(url: &str, headers: &HeaderMap) -> Result<WsClient, TransportError> {
        WsClientBuilder::default()
            .set_headers(headers.to_owned())
            .max_request_size(Self::MAX_MESSAGE_SIZE)
            .max_response_size(Self::MAX_MESSAGE_SIZE)
            .build(url)
            .await
            .map_err(|error| TransportError::Disconnected(error.into()))
    }

    async fn client(&self) -> Arc<WsClient> {
        self.client.read().await.clone()
    }
}

impl Transport for WsTransport {
    async fn batch(&self, calls: Vec<Call>) -> Result<Vec<CallResult>, TransportError> {
        let mut batch = BatchRequestBuilder::new();
        for Call { method, params } in calls {
            batch
                .insert(method, array_params(params))
                .map_err(|error| TransportError::Other(error.into()))?;
        }

        let response: BatchResponse<Value> = self
            .client()
            .await
            .batch_request(batch)
            .await
            .map_err(transport_error)?;

        let results = response
            .into_iter()
            .map(|entry| {
                entry.map_err(|error| CallError {
                    code: error.code(),
                    message: error.message().to_owned(),
                })
            })
            .collect();

        Ok(results)
    }

    async fn subscribe(
        &self,
        method: &'static str,
        params: Vec<Value>,
        unsubscribe: &'static str,
    ) -> Result<Subscription, TransportError> {
        let subscription = self
            .client()
            .await
            .subscribe::<Value, _>(method, array_params(params), unsubscribe)
            .await
            .map_err(transport_error)?;

        let id = match subscription.kind() {
            SubscriptionKind::Subscription(id) => {
                serde_json::to_value(id).map_err(|error| TransportError::Other(error.into()))?
            }
            kind => {
                let error = format!("subscription without an ID: {kind:?}");
                return Err(TransportError::Other(error.into()));
            }
        };
        let notifications = subscription
            .map_err(|error| TransportError::Other(error.into()))
            .boxed();

        Ok(Subscription { id, notifications })
    }

    async fn reconnect(&self) -> Result<(), TransportError> {
        let client = Self::connect(&self.url, &self.headers).await?;
        *self.client.write().await = Arc::new(client);
        Ok(())
    }
}

fn array_params(params: Vec<Value>) -> ArrayParams {
    params
        .into_iter()
        .fold(ArrayParams::new(), |mut array, param| {
            array.insert(param).expect("a JSON value can be serialized");
            array
        })
}

fn transport_error(error: ClientError) -> TransportError {
    use ClientError::*;
    match error {
        RestartNeeded(_) | Transport(_) => TransportError::Disconnected(error.into()),
        RequestTimeout => TransportError::Timeout(error.into()),
        error => TransportError::Other(error.into()),
    }
}
