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

use futures::{StreamExt, TryStreamExt, future::try_join_all};
use indexer_common::domain::{BlockHash, BlockNumber};
use log::{debug, error, info, warn};
use metrics::{counter, gauge};
use serde_json::Value;
use std::{
    num::NonZeroUsize,
    sync::Arc,
    time::{Duration, Instant},
};
use thiserror::Error;
use tokio::{
    sync::{Mutex, Semaphore},
    time::sleep,
};

mod counters;
#[cfg(test)]
pub(crate) mod testing;
#[cfg(test)]
mod tests;
mod transport;

use self::counters::{count_key, json_size};
pub use self::{
    counters::{BatchCount, Count, Counters},
    transport::{
        Call, CallError, CallResult, Notifications, Subscription, Transport, TransportError,
        WsTransport,
    },
};

/// The most subscriptions open at once: half of a node's default per-connection limit
/// (`--rpc-max-subscriptions-per-connection`, 1024).
pub const MAX_SUBSCRIPTIONS: usize = 512;

/// Names of the RPC client's metrics. Request and byte counts carry a `call` label: the method,
/// the method and runtime function, or the storage item, as [Counters] keys them.
pub mod metric {
    pub const REQUEST_COUNT: &str = "indexer_rpc_request_count";
    pub const REQUEST_BYTES: &str = "indexer_rpc_request_bytes";
    pub const RESPONSE_BYTES: &str = "indexer_rpc_response_bytes";
    pub const BATCH_COUNT: &str = "indexer_rpc_batch_count";
    pub const RECONNECT_COUNT: &str = "indexer_rpc_reconnect_count";
    /// 1 while connected to the node, 0 while reconnecting.
    pub const CONNECTED: &str = "indexer_node_connected";
    /// How long the node has been unreachable, while reconnecting.
    pub const UNREACHABLE_SECONDS: &str = "indexer_node_unreachable_seconds";
}

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

/// A set of calls to send together, built with one consuming method per RPC method.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Batch(Vec<Call>);

impl Batch {
    /// `archive_v1_hashByHeight`: the hashes of the blocks at the given height.
    pub fn hash_by_height(self, height: BlockNumber) -> Self {
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
    pub(crate) fn delay(&self, attempt: usize) -> Duration {
        let millis = 10u64.saturating_mul(2u64.saturating_pow(attempt as u32));
        Duration::from_millis(millis).min(self.max_delay)
    }

    /// Retry `attempt`, waiting before each try, until it succeeds. After every `max_attempts`
    /// failed tries the node counts as unreachable and an error is logged.
    pub(crate) async fn retry<T, F: Future<Output = Result<T, TransportError>>>(
        &self,
        mut attempt: impl FnMut() -> F,
    ) -> T {
        let lost = Instant::now();
        let mut attempts = 0;

        loop {
            sleep(self.delay(attempts)).await;
            attempts += 1;
            match attempt().await {
                Ok(value) => {
                    debug!(attempts; "connected to node");
                    return value;
                }
                Err(error) => {
                    gauge!(metric::UNREACHABLE_SECONDS).set(lost.elapsed().as_secs_f64());
                    if attempts % self.max_attempts.max(1) == 0 {
                        error!(
                            error:%,
                            attempts,
                            unreachable_for:? = lost.elapsed();
                            "node unreachable, still retrying"
                        );
                    }
                }
            }
        }
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

/// JSON-RPC access to a node over a [Transport]: batches split and bounded in flight, at most
/// [MAX_SUBSCRIPTIONS] subscriptions, lost connections replaced, and every call counted.
pub struct NodeRpc<T> {
    transport: Arc<T>,
    batch_size: NonZeroUsize,
    batches_in_flight: NonZeroUsize,
    in_flight: Arc<Semaphore>,
    subscriptions: Arc<Semaphore>,
    reconnect_policy: ReconnectPolicy,
    /// The generation of the connection, incremented by each reconnect; locked while reconnecting.
    connection: Arc<Mutex<u64>>,
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
            connection: self.connection.clone(),
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
        let rpc = Self {
            transport: Arc::new(transport),
            batch_size,
            batches_in_flight,
            in_flight: Arc::new(Semaphore::new(batches_in_flight.get())),
            subscriptions: Arc::new(Semaphore::new(MAX_SUBSCRIPTIONS)),
            reconnect_policy,
            connection: Default::default(),
            counters: Default::default(),
        };
        gauge!(metric::CONNECTED).set(1.0);

        rpc
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
        let Subscription { id, notifications } = self
            .recovering(|| {
                self.transport
                    .subscribe(method, params.clone(), unsubscribe)
            })
            .await
            .map_err(|source| Error::Subscribe { method, source })?;

        let request_bytes = params.iter().map(json_size).sum::<usize>() as u64;
        self.counters.record(method, 1, request_bytes, 0);

        let counters = self.counters.clone();
        let notifications = notifications
            .inspect_ok(move |notification| {
                let _permit = &permit;
                counters.record(method, 0, 0, json_size(notification) as u64)
            })
            .boxed();

        Ok(Subscription { id, notifications })
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

        let results = self
            .recovering(|| self.transport.batch(calls.clone()))
            .await
            .map_err(|source| Error::BatchRejected {
                batch_size: calls.len(),
                source,
            })?;
        self.count(&calls, &results);

        Ok(results)
    }

    /// Run `attempt` until it succeeds or fails for good. A lost connection is replaced and the
    /// attempt run again, and so is a timeout, as a half-open connection only times out; that ends
    /// every call on the connection, to be retried as well. Losing a freshly made connection again
    /// fails, most likely a proxy refusing the request's size.
    async fn recovering<R, F: Future<Output = Result<R, TransportError>>>(
        &self,
        mut attempt: impl FnMut() -> F,
    ) -> Result<R, TransportError> {
        let mut reconnected = false;

        loop {
            let connection = *self.connection.lock().await;
            match attempt().await {
                Ok(value) => return Ok(value),
                Err(error @ TransportError::Disconnected(_)) if !reconnected => {
                    self.reconnect(connection, error).await;
                    reconnected = true;
                }
                Err(error @ TransportError::Timeout(_)) => self.reconnect(connection, error).await,
                Err(error) => return Err(error),
            }
        }
    }

    /// Replace the connection of the given generation after it was lost, unless another call has
    /// already replaced it; retried until the node is reachable again.
    async fn reconnect(&self, connection: u64, error: TransportError) {
        let mut current = self.connection.lock().await;
        if *current == connection {
            warn!(error:%; "node connection lost, reconnecting");
            counter!(metric::RECONNECT_COUNT).increment(1);
            gauge!(metric::CONNECTED).set(0.0);
            let lost = Instant::now();

            self.reconnect_policy
                .retry(|| self.transport.reconnect())
                .await;

            *current += 1;
            gauge!(metric::CONNECTED).set(1.0);
            gauge!(metric::UNREACHABLE_SECONDS).set(0.0);
            info!(after:? = lost.elapsed(); "node connection restored");
        }
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
