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

//! The block sourcing pipeline: Finalized → Chunk → Resolve → Source → Verify → Emit. It sources
//! raw node data through [NodeRpc] and decodes nothing but block headers.

use crate::{
    domain::BlockRef,
    infra::subxt_node::rpc::{
        self, Batch, CallResult, Counters, NodeRpc, ReconnectPolicy, Transport, WsTransport, method,
    },
    pipeline::{
        metric,
        sourcing::{emit::Emission, finalized::follow_finalized},
    },
};
use async_stream::stream;
use futures::{
    StreamExt,
    stream::{BoxStream, FuturesOrdered},
};
use http::{HeaderMap, HeaderValue, header::USER_AGENT};
use indexer_common::domain::{BlockHash, ByteArray, ByteVec, ProtocolVersionError};
use log::warn;
use metrics::gauge;
use parity_scale_codec::Decode;
use serde::Deserialize;
use std::{num::NonZeroUsize, sync::Arc, time::Duration};
use subxt::{
    ArcMetadata, config::substrate::SubstrateHeader,
    ext::frame_decode::storage::encode_storage_key_prefix, utils::H256,
};
use thiserror::Error;
use tokio::{
    select,
    sync::{mpsc, oneshot, watch},
    task::{self, JoinHandle},
    time::sleep,
};

mod chunk;
mod emit;
mod finalized;
mod metadata;
mod resolve;
mod source;
mod storage;
#[cfg(test)]
mod tests;
mod verify;

pub(crate) use self::metadata::MetadataCache;
#[cfg(test)]
pub(crate) use self::source::source;
pub use self::{metadata::metadata_spec_version, resolve::resolve};

/// Storage items holding a consensus engine's authority set. An item a runtime lacks is absent from
/// a block's authority set.
pub const AUTHORITY_SET_ITEMS: [(&str, &str); 3] = [
    ("Aura", "Authorities"),
    ("Babe", "Authorities"),
    ("Babe", "NextAuthorities"),
];
/// The storage item holding a block's events.
pub const SYSTEM_EVENTS_ITEM: (&str, &str) = ("System", "Events");
/// Storage items holding the D-Parameter and the terms and conditions.
pub const SYSTEM_PARAMETERS_ITEMS: [(&str, &str); 2] = [
    ("SystemParameters", "DParameterStorage"),
    ("SystemParameters", "TermsAndConditionsStorage"),
];
/// Storage maps holding the genesis cNight registrations: `Mappings` up to node 1.0, `Mapping`
/// from node 2.0.
pub const CNIGHT_MAPPINGS_ITEMS: [(&str, &str); 2] = [
    ("CNightObservation", "Mappings"),
    ("CNightObservation", "Mapping"),
];

/// The storage key of a plain storage item, or the key prefix of a storage map.
pub fn storage_key((pallet, entry): (&str, &str)) -> [u8; 32] {
    encode_storage_key_prefix(pallet, entry)
}

/// A contiguous, verified run of blocks in ascending height order.
pub type Chunk = Vec<Block>;

/// Raw node data for one block.
#[derive(Debug)]
pub enum Block {
    /// The genesis block: it has no parent and no author.
    Genesis {
        hash: BlockHash,
        header: ByteVec,
        zswap_state_root: ByteVec,
        ledger_state_root: ByteVec,
        /// SCALE-encoded results of the D-Parameter and terms and conditions runtime calls.
        system_parameters: (ByteVec, ByteVec),
        metadata: ArcMetadata,
        /// The chain spec's serialized genesis ledger state.
        ledger_state: ByteVec,
        /// Serialized key-value pairs of the cNight mapping storage.
        cnight_mappings: Vec<(ByteVec, ByteVec)>,
        extrinsics: Vec<ByteVec>,
        /// The serialized `System.Events` value.
        events: ByteVec,
    },
    Block {
        hash: BlockHash,
        height: u64,
        header: ByteVec,
        zswap_state_root: ByteVec,
        ledger_state_root: ByteVec,
        /// SCALE-encoded results of the D-Parameter and terms and conditions runtime calls; present
        /// for the first block of a run and where their storage changed.
        system_parameters: Option<(ByteVec, ByteVec)>,
        /// Metadata of the runtime that executed this block.
        metadata: ArcMetadata,
        parent: Parent,
        extrinsics: Vec<ByteVec>,
        /// The serialized `System.Events` value.
        events: ByteVec,
    },
}

impl Block {
    pub fn hash(&self) -> BlockHash {
        match self {
            Self::Genesis { hash, .. } | Self::Block { hash, .. } => *hash,
        }
    }

    pub fn height(&self) -> u64 {
        match self {
            Self::Genesis { .. } => 0,
            Self::Block { height, .. } => *height,
        }
    }
}

/// What a block takes from its parent.
#[derive(Debug)]
pub struct Parent {
    pub hash: BlockHash,
    /// The [AUTHORITY_SET_ITEMS] present in the parent's state, as storage key and value.
    pub authority_set: Vec<(ByteVec, ByteVec)>,
}

/// The latest finalized block and the hashes finalized with it, in ascending height order: the
/// hash at index `i` is at height `tip.height + 1 - hashes.len() + i`.
#[derive(Debug, Clone)]
pub struct Finalized {
    pub hashes: Vec<BlockHash>,
    pub tip: BlockRef,
}

/// Error of the block sourcing pipeline.
#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Rpc(#[from] rpc::Error),
    #[error("cannot decode a chainHead_v1_follow event")]
    FollowEvent(#[source] serde_json::Error),
    #[error("cannot decode hash {0}")]
    Hash(String),
    #[error("node has no header for block {0}")]
    MissingHeader(BlockHash),
    #[error("cannot decode the header of block {0}")]
    Header(
        BlockHash,
        #[source] Box<dyn std::error::Error + Send + Sync>,
    ),
    #[error("block {0} has no protocol version header")]
    MissingProtocolVersion(BlockHash),
    #[error("unsupported protocol version in block {0}")]
    ProtocolVersion(BlockHash, #[source] ProtocolVersionError),
    #[error("node has no body for block {0}")]
    MissingBody(BlockHash),
    #[error("runtime call {function} at block {hash} failed: {error}")]
    RuntimeCall {
        function: &'static str,
        hash: BlockHash,
        error: String,
    },
    #[error("storage query at block {hash} failed: {error}")]
    Storage { hash: BlockHash, error: String },
    #[error("cannot decode the {what} of block {hash}")]
    Decode {
        what: &'static str,
        hash: BlockHash,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("no genesis ledger state in the chain spec's properties")]
    MissingGenesisLedgerState,
    #[error("block {hash} is at height {header_height}, not {height}")]
    HeightMismatch {
        hash: BlockHash,
        height: u64,
        header_height: u64,
    },
    #[error(
        "no metadata of runtime {spec_version} for block {hash}; the parent's and the block's \
         state declare {found:?}"
    )]
    MetadataVersion {
        hash: BlockHash,
        spec_version: u32,
        found: Vec<Option<u32>>,
    },
    #[error("no single block at height {0}")]
    Unresolved(u64),
    #[error("blocks from height {0} do not link to the finalized chain")]
    Unlinked(u64),
    #[error("following finalized blocks failed")]
    Follow(#[source] Box<Error>),
    #[error("following finalized blocks ended")]
    FinalizedEnded,
}

/// Settings of the block sourcing pipeline.
#[derive(Debug, Clone, Copy)]
pub struct Config {
    /// The most heights per chunk.
    pub chunk_size: NonZeroUsize,
    /// The most chunks in progress, and the most chunks sourced but not yet received.
    pub chunks_ahead: NonZeroUsize,
    /// The most calls per JSON-RPC batch.
    pub rpc_batch_size: NonZeroUsize,
    /// The most JSON-RPC batches in flight.
    pub rpc_batches_in_flight: NonZeroUsize,
    /// How long the finalized-block subscription may stay silent before it is renewed.
    pub recovery_timeout: Duration,
    pub reconnect_policy: ReconnectPolicy,
}

/// The block sourcing pipeline over a [Transport].
pub struct Source<T> {
    rpc: NodeRpc<T>,
    config: Config,
    metadata: Arc<MetadataCache>,
}

impl Source<WsTransport> {
    /// Connect to the node at the given URL, retrying per the config's reconnect policy, and check
    /// that it serves every required RPC method.
    pub async fn connect(url: &str, config: Config) -> Result<Self, Error> {
        let user_agent = HeaderValue::from_static(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ));
        let headers = HeaderMap::from_iter([(USER_AGENT, user_agent)]);
        let connect = || WsTransport::new(url, headers.clone());
        let transport = match connect().await {
            Ok(transport) => transport,
            Err(error) => {
                warn!(error:%; "cannot connect to node, retrying");
                config.reconnect_policy.retry(error, connect).await?
            }
        };

        let source = Self::new(transport, config);
        source.rpc.check_methods().await?;

        Ok(source)
    }
}

impl<T: Transport> Source<T> {
    pub fn new(transport: T, config: Config) -> Self {
        let rpc = NodeRpc::new(
            transport,
            config.rpc_batch_size,
            config.rpc_batches_in_flight,
            config.reconnect_policy,
        );

        Self {
            rpc,
            config,
            metadata: Default::default(),
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn rpc(&self) -> &NodeRpc<T> {
        &self.rpc
    }

    /// The request and byte counts of every RPC call made.
    pub fn counters(&self) -> &Counters {
        self.rpc.counters()
    }

    /// Run the pipeline from the block after `start`, or from genesis, up to and including the block
    /// at height `end`, or without end. Returns the verified chunks in height order, and the latest
    /// finalized block.
    ///
    /// After an error, the stream yields it and, if polled again, resumes after the last block it
    /// yielded. Dropping the stream stops every task of the pipeline.
    pub fn run(
        &self,
        start: Option<BlockRef>,
        end: Option<u64>,
    ) -> (
        BoxStream<'static, Result<Chunk, Error>>,
        watch::Receiver<Option<Finalized>>,
    ) {
        let (finalized_tx, finalized_rx) = watch::channel(None);
        let (follow_error_tx, follow_error_rx) = oneshot::channel();
        let follow = task::spawn({
            let rpc = self.rpc.clone();
            let recovery_timeout = self.config.recovery_timeout;
            async move {
                if let Err(error) = follow_finalized(&rpc, recovery_timeout, &finalized_tx).await {
                    let _ = follow_error_tx.send(error);
                }
            }
        });

        let (chunk_tx, mut chunk_rx) = mpsc::channel(self.config.chunks_ahead.get());
        let producer = Producer {
            rpc: self.rpc.clone(),
            metadata: self.metadata.clone(),
            config: self.config,
            finalized: finalized_rx.clone(),
            chunks: chunk_tx,
            start,
            end,
        };
        let producer = task::spawn(async move {
            let _follow = AbortOnDrop(follow);
            producer.produce(follow_error_rx).await
        });

        let chunks = stream! {
            let _producer = AbortOnDrop(producer);
            while let Some(chunk) = chunk_rx.recv().await {
                gauge!(metric::BUFFERED_CHUNK_COUNT).set(chunk_rx.len() as f64);
                yield chunk;
            }
        };

        (chunks.boxed(), finalized_rx)
    }
}

/// Aborts the task when dropped.
struct AbortOnDrop<O>(JoinHandle<O>);

impl<O> Drop for AbortOnDrop<O> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// The task sourcing, verifying and emitting chunks.
struct Producer<T> {
    rpc: NodeRpc<T>,
    metadata: Arc<MetadataCache>,
    config: Config,
    finalized: watch::Receiver<Option<Finalized>>,
    chunks: mpsc::Sender<Result<Chunk, Error>>,
    start: Option<BlockRef>,
    end: Option<u64>,
}

impl<T: Transport> Producer<T> {
    async fn produce(mut self, mut follow_error: oneshot::Receiver<Error>) {
        let mut emission = Emission {
            emitted: self.start,
            held: vec![],
        };

        loop {
            match self
                .produce_until_error(&mut emission, &mut follow_error)
                .await
            {
                Ok(()) => return,
                Err(error) => {
                    warn!(error:% = error; "block sourcing failed");
                    // Without finalized blocks nothing can be sourced any more.
                    let terminal = matches!(error, Error::FinalizedEnded | Error::Follow(_));
                    if self.chunks.send(Err(error)).await.is_err() || terminal {
                        return;
                    }

                    // Resume after the last block emitted.
                    emission.held.clear();
                    sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }

    async fn produce_until_error(
        &mut self,
        emission: &mut Emission,
        follow_error: &mut oneshot::Receiver<Error>,
    ) -> Result<(), Error> {
        let genesis_hash = self.genesis_hash().await?;
        let run_start = emission.next_height();
        let mut next = run_start;
        let mut in_progress = FuturesOrdered::new();

        loop {
            if self.end.is_some_and(|end| next > end) && in_progress.is_empty() {
                let held = std::mem::take(&mut emission.held);
                return self.emit(emission, held).await;
            }

            // Plan as many chunks as allowed and possible.
            while in_progress.len() < self.config.chunks_ahead.get()
                && self.end.is_none_or(|end| next <= end)
                && let Some(planned) = self.plan(next, run_start)
            {
                next = planned.spec.heights.end() + 1;
                gauge!(metric::PLANNED_HEIGHT).set((next - 1) as f64);
                in_progress.push_back(self.source_planned(planned));
            }

            select! {
                Some(sourced) = in_progress.next() => {
                    let (planned, chunk) = sourced?;
                    self.verify_and_emit(emission, planned, chunk, genesis_hash, run_start)
                        .await?;
                }
                changed = self.finalized.changed(), if in_progress.is_empty() => {
                    if changed.is_err() {
                        return Err(Error::FinalizedEnded);
                    }
                }
                error = &mut *follow_error => {
                    return Err(error.map_or(Error::FinalizedEnded, |error| Error::Follow(Box::new(error))));
                }
            }
        }
    }

    /// The genesis hash, for verifying block 0, if the run starts there.
    async fn genesis_hash(&self) -> Result<Option<BlockHash>, Error> {
        if self.start.is_some() {
            return Ok(None);
        }

        let batch = Batch::default().genesis_hash();
        let hash = self
            .rpc
            .batch(batch)
            .await?
            .pop()
            .expect("one result per call")
            .map_err(|source| rpc::Error::Call {
                method: method::ARCHIVE_GENESIS_HASH,
                source,
            })?;
        let hash = serde_json::from_value::<String>(hash).map_err(|source| rpc::Error::Decode {
            method: method::ARCHIVE_GENESIS_HASH,
            source,
        })?;

        block_hash_of(hash).map(Some)
    }
}

fn header_bytes(header: CallResult, hash: BlockHash) -> Result<ByteVec, Error> {
    let header = header.map_err(|source| rpc::Error::Call {
        method: method::ARCHIVE_HEADER,
        source,
    })?;
    let header = header.as_str().ok_or(Error::MissingHeader(hash))?;

    const_hex::decode(header)
        .map(Into::into)
        .map_err(|error| Error::Header(hash, error.into()))
}

fn decode_header(header: &[u8], hash: BlockHash) -> Result<SubstrateHeader<H256>, Error> {
    SubstrateHeader::<H256>::decode(&mut &*header)
        .map_err(|error| Error::Header(hash, error.into()))
}

fn block_hash_of(hash: String) -> Result<BlockHash, Error> {
    const_hex::decode_to_array(&hash)
        .map(ByteArray)
        .map_err(|_| Error::Hash(hash))
}

/// The SCALE-encoded result of an `archive_v1_call`.
fn call_value(
    result: CallResult,
    function: &'static str,
    hash: BlockHash,
) -> Result<ByteVec, Error> {
    #[derive(Deserialize)]
    struct CallOutcome {
        success: bool,
        value: Option<String>,
        error: Option<String>,
    }

    let outcome = result.map_err(|source| rpc::Error::Call {
        method: method::ARCHIVE_CALL,
        source,
    })?;
    let outcome = serde_json::from_value::<Option<CallOutcome>>(outcome).map_err(|source| {
        rpc::Error::Decode {
            method: method::ARCHIVE_CALL,
            source,
        }
    })?;

    match outcome {
        Some(CallOutcome {
            success: true,
            value: Some(value),
            ..
        }) => const_hex::decode(value)
            .map(ByteVec::from)
            .map_err(|error| Error::Decode {
                what: "runtime call result",
                hash,
                source: error.into(),
            }),
        Some(CallOutcome { error, .. }) => Err(Error::RuntimeCall {
            function,
            hash,
            error: error.unwrap_or_default(),
        }),
        None => Err(Error::RuntimeCall {
            function,
            hash,
            error: "block not found".to_owned(),
        }),
    }
}
