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

//! JSON-RPC access to a node: the [Transport] seam, its jsonrpsee implementation [WsTransport],
//! the [Batch] builder named after the RPC methods, and [NodeRpc], which splits, bounds, retries
//! and counts every call.

use futures::{StreamExt, TryStreamExt, future::try_join_all, stream::BoxStream};
use http::HeaderMap;
use indexer_common::{domain::BlockHash, error::BoxError};
use jsonrpsee::{
    core::{
        client::{
            BatchResponse, ClientT, Error as ClientError, SubscriptionClientT, SubscriptionKind,
        },
        params::{ArrayParams, BatchRequestBuilder},
    },
    ws_client::{WsClient, WsClientBuilder},
};
use log::{debug, warn};
use parking_lot::Mutex;
use serde_json::Value;
use std::{collections::BTreeMap, num::NonZeroUsize, sync::Arc, time::Duration};
use thiserror::Error;
use tokio::{
    sync::{RwLock, Semaphore},
    time::sleep,
};

#[cfg(test)]
pub(crate) mod testing;

/// The most subscriptions open at once: half of a node's default per-connection limit
/// (`--rpc-max-subscriptions-per-connection`, 1024).
pub const MAX_SUBSCRIPTIONS: usize = 512;

/// Methods of the new JSON-RPC spec this module calls.
pub mod method {
    pub const ARCHIVE_BODY: &str = "archive_v1_body";
    pub const ARCHIVE_CALL: &str = "archive_v1_call";
    pub const ARCHIVE_FINALIZED_HEIGHT: &str = "archive_v1_finalizedHeight";
    pub const ARCHIVE_GENESIS_HASH: &str = "archive_v1_genesisHash";
    pub const ARCHIVE_HASH_BY_HEIGHT: &str = "archive_v1_hashByHeight";
    pub const ARCHIVE_HEADER: &str = "archive_v1_header";
    pub const ARCHIVE_STORAGE: &str = "archive_v1_storage";
    pub const ARCHIVE_STOP_STORAGE: &str = "archive_v1_stopStorage";
    pub const CHAIN_HEAD_FOLLOW: &str = "chainHead_v1_follow";
    pub const CHAIN_HEAD_UNFOLLOW: &str = "chainHead_v1_unfollow";
    pub const CHAIN_HEAD_UNPIN: &str = "chainHead_v1_unpin";
    pub const CHAIN_SPEC_PROPERTIES: &str = "chainSpec_v1_properties";
    pub const RPC_METHODS: &str = "rpc_methods";
}

/// Methods a node must serve; all of them require `--state-pruning archive` except `chainHead_v1_*`
/// and `chainSpec_v1_*`.
pub const REQUIRED_METHODS: [&str; 10] = [
    method::ARCHIVE_HASH_BY_HEIGHT,
    method::ARCHIVE_HEADER,
    method::ARCHIVE_BODY,
    method::ARCHIVE_CALL,
    method::ARCHIVE_STORAGE,
    method::ARCHIVE_FINALIZED_HEIGHT,
    method::ARCHIVE_GENESIS_HASH,
    method::CHAIN_HEAD_FOLLOW,
    method::CHAIN_HEAD_UNPIN,
    method::CHAIN_SPEC_PROPERTIES,
];

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
        error => TransportError::Other(error.into()),
    }
}

/// A set of calls to send together, built with one consuming method per RPC method.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Batch(Vec<Call>);

impl Batch {
    /// `archive_v1_hashByHeight`: the hashes of the blocks at the given height.
    pub fn hash_by_height(self, height: u64) -> Self {
        self.push(method::ARCHIVE_HASH_BY_HEIGHT, vec![height.into()])
    }

    /// `archive_v1_header`: the SCALE-encoded header of the given block.
    pub fn header(self, hash: BlockHash) -> Self {
        self.push(method::ARCHIVE_HEADER, vec![hex(hash.0)])
    }

    /// `archive_v1_body`: the SCALE-encoded extrinsics of the given block.
    pub fn body(self, hash: BlockHash) -> Self {
        self.push(method::ARCHIVE_BODY, vec![hex(hash.0)])
    }

    /// `archive_v1_call`: the SCALE-encoded result of a runtime API function at the given block.
    pub fn call(self, hash: BlockHash, function: &str, parameters: &[u8]) -> Self {
        self.push(
            method::ARCHIVE_CALL,
            vec![hex(hash.0), function.into(), hex(parameters)],
        )
    }

    /// `archive_v1_genesisHash`: the hash of the genesis block.
    pub fn genesis_hash(self) -> Self {
        self.push(method::ARCHIVE_GENESIS_HASH, vec![])
    }

    /// `chainHead_v1_unpin`: release the given blocks pinned by a `chainHead_v1_follow`
    /// subscription.
    pub fn unpin(self, subscription: Value, hashes: &[BlockHash]) -> Self {
        let hashes = hashes.iter().map(|hash| hex(hash.0)).collect::<Vec<_>>();
        self.push(method::CHAIN_HEAD_UNPIN, vec![subscription, hashes.into()])
    }

    /// `chainSpec_v1_properties`: the chain spec's properties.
    pub fn chain_spec_properties(self) -> Self {
        self.push(method::CHAIN_SPEC_PROPERTIES, vec![])
    }

    /// `rpc_methods`: the methods the node serves.
    pub fn rpc_methods(self) -> Self {
        self.push(method::RPC_METHODS, vec![])
    }

    /// The number of calls.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there are no calls.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn push(mut self, method: &'static str, params: Vec<Value>) -> Self {
        self.0.push(Call { method, params });
        self
    }
}

/// `0x`-prefixed hex, as the JSON-RPC spec encodes bytes.
pub fn hex(bytes: impl AsRef<[u8]>) -> Value {
    const_hex::encode_prefixed(bytes).into()
}

/// How often and how patiently to reconnect after the connection is lost.
#[derive(Debug, Clone, Copy)]
pub struct ReconnectPolicy {
    pub max_delay: Duration,
    pub max_attempts: usize,
}

impl ReconnectPolicy {
    /// Exponential backoff from 10 ms, doubling up to `max_delay`.
    fn delay(&self, attempt: usize) -> Duration {
        let millis = 10u64.saturating_mul(2u64.saturating_pow(attempt as u32));
        Duration::from_millis(millis).min(self.max_delay)
    }

    /// Retry `attempt` after it failed with `error`, waiting before each try, at most
    /// `max_attempts` times; [Error::Unreachable] with the last error if none succeeds.
    pub(crate) async fn retry<T, F: Future<Output = Result<T, TransportError>>>(
        &self,
        error: TransportError,
        mut attempt: impl FnMut() -> F,
    ) -> Result<T, Error> {
        let mut last_error = error;
        for n in 0..self.max_attempts {
            sleep(self.delay(n)).await;
            match attempt().await {
                Ok(value) => {
                    debug!(attempt = n; "connected to node");
                    return Ok(value);
                }
                Err(error) => last_error = error,
            }
        }

        Err(Error::Unreachable {
            attempts: self.max_attempts,
            source: last_error,
        })
    }
}

/// Requests and bytes of one method, or of one storage item.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Count {
    pub requests: u64,
    pub request_bytes: u64,
    pub response_bytes: u64,
}

/// Batches sent, and the largest batch response.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BatchCount {
    pub batches: u64,
    pub largest_response_bytes: u64,
}

/// Request and byte counts, keyed by method, by method and runtime function, or by storage item.
/// Bytes are JSON wire bytes, except for storage items, whose bytes are the decoded values.
#[derive(Debug, Default)]
pub struct Counters {
    counts: Mutex<BTreeMap<String, Count>>,
    batches: Mutex<BatchCount>,
}

impl Counters {
    /// Add to the count of the given key.
    pub fn record(&self, key: &str, requests: u64, request_bytes: u64, response_bytes: u64) {
        let mut counts = self.counts.lock();
        let count = counts.entry(key.to_owned()).or_default();
        count.requests += requests;
        count.request_bytes += request_bytes;
        count.response_bytes += response_bytes;
    }

    /// The counts so far.
    pub fn counts(&self) -> BTreeMap<String, Count> {
        self.counts.lock().clone()
    }

    /// The batch counts so far.
    pub fn batches(&self) -> BatchCount {
        *self.batches.lock()
    }

    fn record_batch(&self, response_bytes: u64) {
        let mut batches = self.batches.lock();
        batches.batches += 1;
        batches.largest_response_bytes = batches.largest_response_bytes.max(response_bytes);
    }
}

/// Error of [NodeRpc].
#[derive(Debug, Error)]
pub enum Error {
    #[error(
        "batch of {batch_size} calls was rejected by the node or a proxy; lower rpc_batch_size \
         below its batch, request or response size limits"
    )]
    BatchRejected {
        batch_size: usize,
        #[source]
        source: TransportError,
    },
    #[error("cannot reach the node after {attempts} reconnect attempts")]
    Unreachable {
        attempts: usize,
        #[source]
        source: TransportError,
    },
    #[error(
        "node lacks the required RPC methods {missing:?}; it must run with --state-pruning archive"
    )]
    MissingMethods { missing: Vec<&'static str> },
    #[error("cannot subscribe with {method}")]
    Subscribe {
        method: &'static str,
        #[source]
        source: TransportError,
    },
    #[error("{method} failed")]
    Call {
        method: &'static str,
        #[source]
        source: CallError,
    },
    #[error("cannot decode the {method} response")]
    Decode {
        method: &'static str,
        #[source]
        source: serde_json::Error,
    },
}

/// JSON-RPC access to a node over a [Transport]. Batches are split at the batch size and sent
/// concurrently, at most `batches_in_flight` at a time; at most [MAX_SUBSCRIPTIONS] subscriptions
/// are open at a time; a lost connection is replaced per the [ReconnectPolicy] and the batch sent
/// again; every request and response is counted.
pub struct NodeRpc<T> {
    transport: Arc<T>,
    batch_size: NonZeroUsize,
    batches_in_flight: NonZeroUsize,
    in_flight: Arc<Semaphore>,
    subscriptions: Arc<Semaphore>,
    reconnect_policy: ReconnectPolicy,
    counters: Arc<Counters>,
}

impl<T> Clone for NodeRpc<T> {
    fn clone(&self) -> Self {
        Self {
            transport: self.transport.clone(),
            batch_size: self.batch_size,
            batches_in_flight: self.batches_in_flight,
            in_flight: self.in_flight.clone(),
            subscriptions: self.subscriptions.clone(),
            reconnect_policy: self.reconnect_policy,
            counters: self.counters.clone(),
        }
    }
}

impl<T: Transport> NodeRpc<T> {
    pub fn new(
        transport: T,
        batch_size: NonZeroUsize,
        batches_in_flight: NonZeroUsize,
        reconnect_policy: ReconnectPolicy,
    ) -> Self {
        Self {
            transport: Arc::new(transport),
            batch_size,
            batches_in_flight,
            in_flight: Arc::new(Semaphore::new(batches_in_flight.get())),
            subscriptions: Arc::new(Semaphore::new(MAX_SUBSCRIPTIONS)),
            reconnect_policy,
            counters: Default::default(),
        }
    }

    /// The request and byte counts.
    pub fn counters(&self) -> &Counters {
        &self.counters
    }

    /// The most calls in flight at once: the batch size times the batches in flight.
    pub(crate) fn max_calls_in_flight(&self) -> usize {
        self.batch_size.get() * self.batches_in_flight.get()
    }

    /// Send the calls in batches of at most the batch size; one [CallResult] per call, in order.
    pub async fn batch(&self, batch: Batch) -> Result<Vec<CallResult>, Error> {
        let chunks = batch
            .0
            .chunks(self.batch_size.get())
            .map(|calls| self.send(calls.to_vec()));
        let results = try_join_all(chunks).await?;

        Ok(results.into_iter().flatten().collect())
    }

    /// Send a single call and deserialize its result.
    pub async fn call<R: serde::de::DeserializeOwned>(&self, call: Call) -> Result<R, Error> {
        let method = call.method;
        let result = self
            .send(vec![call])
            .await?
            .pop()
            .expect("one result per call")
            .map_err(|source| Error::Call { method, source })?;

        serde_json::from_value(result).map_err(|source| Error::Decode { method, source })
    }

    /// Subscribe, once fewer than [MAX_SUBSCRIPTIONS] are open, reconnecting first if the
    /// connection is lost. The subscription is open until its notifications are dropped.
    pub async fn subscribe(
        &self,
        method: &'static str,
        params: Vec<Value>,
        unsubscribe: &'static str,
    ) -> Result<Subscription, Error> {
        let permit = self
            .subscriptions
            .clone()
            .acquire_owned()
            .await
            .expect("subscriptions semaphore is not closed");
        let mut reconnected = false;

        loop {
            match self
                .transport
                .subscribe(method, params.clone(), unsubscribe)
                .await
            {
                Ok(Subscription { id, notifications }) => {
                    let request_bytes = params.iter().map(json_size).sum::<usize>() as u64;
                    self.counters.record(method, 1, request_bytes, 0);

                    let counters = self.counters.clone();
                    let notifications = notifications
                        .inspect_ok(move |notification| {
                            let _permit = &permit;
                            counters.record(method, 0, 0, json_size(notification) as u64)
                        })
                        .boxed();

                    return Ok(Subscription { id, notifications });
                }
                Err(error @ TransportError::Disconnected(_)) if !reconnected => {
                    self.reconnect(error).await?;
                    reconnected = true;
                }
                Err(error) => {
                    return Err(Error::Subscribe {
                        method,
                        source: error,
                    });
                }
            }
        }
    }

    /// Fail with [Error::MissingMethods] unless the node serves every [REQUIRED_METHODS] method.
    pub(crate) async fn check_methods(&self) -> Result<(), Error> {
        #[derive(serde::Deserialize)]
        struct Methods {
            methods: Vec<String>,
        }

        let call = Batch::default().rpc_methods().0.pop().expect("one call");
        let Methods { methods } = self.call(call).await?;

        let missing = REQUIRED_METHODS
            .into_iter()
            .filter(|required| !methods.iter().any(|method| method == required))
            .collect::<Vec<_>>();

        if missing.is_empty() {
            Ok(())
        } else {
            Err(Error::MissingMethods { missing })
        }
    }

    async fn send(&self, calls: Vec<Call>) -> Result<Vec<CallResult>, Error> {
        let _permit = self
            .in_flight
            .acquire()
            .await
            .expect("in-flight semaphore is never closed");
        let batch_size = calls.len();
        let mut reconnected = false;

        loop {
            match self.transport.batch(calls.clone()).await {
                Ok(results) => {
                    self.count(&calls, &results);
                    return Ok(results);
                }
                // A batch that loses a freshly made connection again is refused, most likely for its
                // size, e.g. by a proxy closing the connection.
                Err(error @ TransportError::Disconnected(_)) if !reconnected => {
                    self.reconnect(error).await?;
                    reconnected = true;
                }
                Err(error) => {
                    return Err(Error::BatchRejected {
                        batch_size,
                        source: error,
                    });
                }
            }
        }
    }

    async fn reconnect(&self, error: TransportError) -> Result<(), Error> {
        warn!(error:%; "node connection lost, reconnecting");
        self.reconnect_policy
            .retry(error, || self.transport.reconnect())
            .await
    }

    fn count(&self, calls: &[Call], results: &[CallResult]) {
        let response_bytes = calls
            .iter()
            .zip(results)
            .map(|(call, result)| {
                let request_bytes = call.params.iter().map(json_size).sum::<usize>() as u64;
                let response_bytes = result.as_ref().map(json_size).unwrap_or_default() as u64;
                self.counters
                    .record(&count_key(call), 1, request_bytes, response_bytes);
                response_bytes
            })
            .sum();

        self.counters.record_batch(response_bytes);
    }
}

/// The key a call is counted under: its method, and for runtime calls also the function.
fn count_key(call: &Call) -> String {
    match (call.method, call.params.get(1).and_then(Value::as_str)) {
        (method::ARCHIVE_CALL, Some(function)) => format!("{} {function}", call.method),
        (method, _) => method.to_owned(),
    }
}

/// The size of a JSON value as serialized, without serializing it.
fn json_size(value: &Value) -> usize {
    use Value::*;
    match value {
        Null => 4,
        Bool(true) => 4,
        Bool(false) => 5,
        Number(number) => number.to_string().len(),
        String(string) => string.len() + 2,
        Array(values) => {
            values.iter().map(json_size).sum::<usize>() + values.len().saturating_sub(1) + 2
        }
        Object(entries) => {
            entries
                .iter()
                .map(|(key, value)| key.len() + 3 + json_size(value))
                .sum::<usize>()
                + entries.len().saturating_sub(1)
                + 2
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::infra::subxt_node::rpc::{
        Batch, Call, CallResult, Count, Error, MAX_SUBSCRIPTIONS, NodeRpc, REQUIRED_METHODS,
        ReconnectPolicy, TransportError, json_size, method, testing::FakeNode,
    };
    use futures::{StreamExt, TryStreamExt, stream};
    use indexer_common::domain::ByteArray;
    use serde_json::{Value, json};
    use std::{
        num::NonZeroUsize,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };
    use tokio::time::timeout;

    const POLICY: ReconnectPolicy = ReconnectPolicy {
        max_delay: Duration::from_millis(10),
        max_attempts: 3,
    };

    fn node_rpc(
        node: Arc<FakeNode>,
        batch_size: usize,
        in_flight: usize,
    ) -> NodeRpc<Arc<FakeNode>> {
        NodeRpc::new(
            node,
            NonZeroUsize::new(batch_size).unwrap(),
            NonZeroUsize::new(in_flight).unwrap(),
            POLICY,
        )
    }

    /// Echo the call, so results can be matched to calls.
    fn echo(call: &Call) -> CallResult {
        Ok(json!({ "method": call.method, "params": call.params }))
    }

    fn heights(n: u64) -> Batch {
        (0..n).fold(Batch::default(), Batch::hash_by_height)
    }

    #[tokio::test]
    async fn test_packing() {
        let node = Arc::new(FakeNode::new(echo));
        let rpc = node_rpc(node.clone(), 4, 2);

        let results = rpc.batch(heights(10)).await.expect("batch succeeds");

        assert_eq!(node.batch_sizes(), vec![4, 4, 2]);
        let heights = results
            .into_iter()
            .map(|result| result.expect("call succeeds")["params"][0].clone())
            .collect::<Vec<_>>();
        assert_eq!(heights, (0..10).map(Value::from).collect::<Vec<_>>());
    }

    #[tokio::test]
    async fn test_batches_in_flight_are_bounded() {
        let node = Arc::new(FakeNode::new(echo).with_delay(Duration::from_millis(20)));
        let rpc = node_rpc(node.clone(), 2, 3);

        rpc.batch(heights(20)).await.expect("batch succeeds");

        assert_eq!(node.batch_sizes().len(), 10);
        assert_eq!(node.max_in_flight(), 3);
    }

    #[tokio::test]
    async fn test_rejected_batch() {
        let node = Arc::new(FakeNode::new(echo).with_max_batch_size(3));
        let rpc = node_rpc(node, 4, 1);

        let error = rpc.batch(heights(4)).await.expect_err("batch is rejected");

        assert!(matches!(error, Error::BatchRejected { batch_size: 4, .. }));
        assert!(error.to_string().contains("rpc_batch_size"));
    }

    #[tokio::test]
    async fn test_payload_accounting() {
        let node = Arc::new(FakeNode::new(|call| match call.method {
            method::ARCHIVE_HASH_BY_HEIGHT => Ok(json!(["0x0101"])),
            method::ARCHIVE_HEADER => Ok(json!("0x020202")),
            _ => Ok(Value::Null),
        }));
        let rpc = node_rpc(node, 2, 1);

        let batch =
            heights(3)
                .header(ByteArray([1; 32]))
                .call(ByteArray([1; 32]), "Test_function", &[]);
        rpc.batch(batch).await.expect("batch succeeds");

        let counts = rpc.counters().counts();
        assert_eq!(
            counts[method::ARCHIVE_HASH_BY_HEIGHT],
            Count {
                requests: 3,
                request_bytes: 3,
                response_bytes: 3 * json_size(&json!(["0x0101"])) as u64,
            }
        );
        assert_eq!(
            counts[method::ARCHIVE_HEADER],
            Count {
                requests: 1,
                request_bytes: json_size(&json!(format!("0x{}", "01".repeat(32)))) as u64,
                response_bytes: json_size(&json!("0x020202")) as u64,
            }
        );
        assert_eq!(counts["archive_v1_call Test_function"].requests, 1);
        assert!(!counts.contains_key(method::ARCHIVE_CALL));
        let batches = rpc.counters().batches();
        assert_eq!(batches.batches, 3);
        assert_eq!(
            batches.largest_response_bytes,
            2 * json_size(&json!(["0x0101"])) as u64
        );
    }

    #[tokio::test]
    async fn test_subscription_accounting() {
        let notifications = vec![json!({ "event": "initialized" }), json!("0x0102")];
        let node = Arc::new(FakeNode::new(echo).with_subscriptions(vec![notifications.clone()]));
        let rpc = node_rpc(node, 4, 1);

        let received = rpc
            .subscribe(
                method::CHAIN_HEAD_FOLLOW,
                vec![json!(false)],
                method::CHAIN_HEAD_UNFOLLOW,
            )
            .await
            .expect("subscription succeeds")
            .notifications
            .take(notifications.len())
            .try_collect::<Vec<_>>()
            .await
            .expect("notifications are received");

        assert_eq!(received, notifications);
        assert_eq!(
            rpc.counters().counts()[method::CHAIN_HEAD_FOLLOW],
            Count {
                requests: 1,
                request_bytes: json_size(&json!(false)) as u64,
                response_bytes: notifications.iter().map(json_size).sum::<usize>() as u64,
            }
        );
    }

    #[tokio::test]
    async fn test_subscriptions_are_bounded() {
        let node = Arc::new(FakeNode::new(echo).with_subscribe(|_, _| Some(vec![])));
        let rpc = node_rpc(node, 4, 1);
        let subscribe = || {
            rpc.subscribe(
                method::ARCHIVE_STORAGE,
                vec![],
                method::ARCHIVE_STOP_STORAGE,
            )
        };

        let mut open = stream::iter(0..MAX_SUBSCRIPTIONS)
            .then(|_| subscribe())
            .try_collect::<Vec<_>>()
            .await
            .expect("subscriptions succeed");
        assert!(
            timeout(Duration::from_millis(50), subscribe())
                .await
                .is_err()
        );

        open.pop();
        timeout(Duration::from_millis(50), subscribe())
            .await
            .expect("subscription is not blocked")
            .expect("subscription succeeds");
    }

    #[test]
    fn test_json_size() {
        let value = json!({ "a": [1, "bc", null, true], "d": { "e": false } });
        assert_eq!(json_size(&value), value.to_string().len());
    }

    #[tokio::test]
    async fn test_reconnect() {
        let node = Arc::new(FakeNode::new(echo).with_disconnects(1));
        let rpc = node_rpc(node.clone(), 4, 1);

        rpc.batch(heights(2))
            .await
            .expect("batch succeeds after reconnecting");

        assert_eq!(node.reconnects(), 1);
    }

    #[tokio::test]
    async fn test_unreachable() {
        let node = Arc::new(
            FakeNode::new(echo)
                .with_disconnects(1)
                .with_failing_reconnects(),
        );
        let rpc = node_rpc(node.clone(), 4, 1);

        let error = rpc
            .batch(heights(2))
            .await
            .expect_err("node is unreachable");

        assert!(matches!(error, Error::Unreachable { attempts: 3, .. }));
        assert_eq!(node.reconnects(), 3);
    }

    #[tokio::test]
    async fn test_retry() {
        let down = || TransportError::Disconnected("node is down".into());

        // Up after two failed retries.
        let attempts = AtomicUsize::new(0);
        let value = POLICY
            .retry(down(), || async {
                match attempts.fetch_add(1, Ordering::SeqCst) {
                    0 | 1 => Err(down()),
                    _ => Ok(7),
                }
            })
            .await
            .expect("third retry succeeds");
        assert_eq!(value, 7);
        assert_eq!(attempts.load(Ordering::SeqCst), 3);

        // Never up: the policy's retries, then unreachable.
        let attempts = AtomicUsize::new(0);
        let error = POLICY
            .retry(down(), || async {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>(down())
            })
            .await
            .expect_err("node stays down");
        assert!(matches!(error, Error::Unreachable { attempts: 3, .. }));
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_check_methods() {
        let node = Arc::new(FakeNode::new(|call| match call.method {
            method::RPC_METHODS => Ok(json!({
                "methods": REQUIRED_METHODS
                    .iter()
                    .filter(|method| !method.starts_with("archive_v1_"))
                    .collect::<Vec<_>>()
            })),
            _ => Ok(Value::Null),
        }));
        let rpc = node_rpc(node, 4, 1);

        let error = rpc
            .check_methods()
            .await
            .expect_err("archive methods are missing");

        let Error::MissingMethods { missing } = &error else {
            panic!("unexpected error {error:?}");
        };
        assert_eq!(missing.len(), 7);
        assert!(
            missing
                .iter()
                .all(|method| method.starts_with("archive_v1_"))
        );
        assert!(error.to_string().contains("--state-pruning archive"));
    }
}
