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

pub mod chunk;

use crate::{
    domain::BlockRef,
    infra::subxt_node::{
        header::SubstrateHeaderExt,
        rpc::{
            self, Batch, CallResult, Counters, NodeRpc, ReconnectPolicy, Subscription, Transport,
            WsTransport, hex, method,
        },
    },
    pipeline::{
        metric::{self, Timer},
        source::chunk::{ChunkSpec, next_chunk},
    },
};
use async_stream::stream;
use futures::{
    StreamExt, TryStreamExt,
    future::try_join,
    stream::{self, BoxStream, FuturesOrdered},
};
use http::{HeaderMap, HeaderValue, header::USER_AGENT};
use indexer_common::domain::{BlockHash, ByteArray, ByteVec, ProtocolVersionError};
use log::{debug, warn};
use metrics::counter;
use parity_scale_codec::{Decode, Encode};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap, future::Future, num::NonZeroUsize, ops::RangeInclusive, sync::Arc,
    time::Duration,
};
use subxt::{
    ArcMetadata, Metadata, config::substrate::SubstrateHeader,
    ext::frame_decode::storage::encode_storage_key_prefix, utils::H256,
};
use thiserror::Error;
use tokio::{
    select,
    sync::{Mutex, mpsc, oneshot, watch},
    task::{self, JoinHandle},
    time::{sleep, timeout},
};

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
const ZSWAP_STATE_ROOT_FUNCTION: &str = "MidnightRuntimeApi_get_zswap_state_root";
const LEDGER_STATE_ROOT_FUNCTION: &str = "MidnightRuntimeApi_get_ledger_state_root";
const D_PARAMETER_FUNCTION: &str = "SystemParametersApi_get_d_parameter";
const TERMS_AND_CONDITIONS_FUNCTION: &str = "SystemParametersApi_get_terms_and_conditions";

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

/// The Finalized stage: follow the node's finalized blocks with `chainHead_v1_follow` and publish
/// each [Finalized] to `finalized`. Every block the subscription reports is unpinned as soon as it
/// is reported; nothing else is fetched except one header per subscription, for the tip's height.
/// The subscription is renewed on `stop`, when it ends, and when no event arrives within
/// `recovery_timeout`. Returns once `finalized` has no receivers.
pub async fn follow_finalized<T: Transport>(
    rpc: &NodeRpc<T>,
    recovery_timeout: Duration,
    finalized: &watch::Sender<Option<Finalized>>,
) -> Result<(), Error> {
    while !finalized.is_closed() {
        let Subscription {
            id,
            mut notifications,
        } = rpc
            .subscribe(
                method::CHAIN_HEAD_FOLLOW,
                vec![false.into()],
                method::CHAIN_HEAD_UNFOLLOW,
            )
            .await?;
        let mut tip = None;

        loop {
            let event = match timeout(recovery_timeout, notifications.next()).await {
                Ok(Some(Ok(event))) => event,
                Ok(Some(Err(error))) => {
                    warn!(error:%; "chainHead_v1_follow failed, resubscribing");
                    break;
                }
                Ok(None) => {
                    warn!("chainHead_v1_follow ended, resubscribing");
                    break;
                }
                Err(_) => {
                    warn!(recovery_timeout:?; "no chainHead_v1_follow event, resubscribing");
                    break;
                }
            };

            match serde_json::from_value(event).map_err(Error::FollowEvent)? {
                FollowEvent::Initialized {
                    finalized_block_hashes,
                } => {
                    let hashes = block_hashes(finalized_block_hashes)?;
                    unpin(rpc, &id, &hashes).await;

                    if let Some(&hash) = hashes.last() {
                        let height = header_height(rpc, hash).await?;
                        tip = Some(BlockRef { hash, height });
                        publish(finalized, hashes, BlockRef { hash, height });
                    }
                }
                FollowEvent::NewBlock { block_hash } => {
                    unpin(rpc, &id, &[block_hash_of(block_hash)?]).await
                }
                FollowEvent::Finalized {
                    finalized_block_hashes,
                } => {
                    let hashes = block_hashes(finalized_block_hashes)?;
                    if let (Some(BlockRef { height, .. }), Some(&hash)) = (tip, hashes.last()) {
                        let height = height + hashes.len() as u64;
                        tip = Some(BlockRef { hash, height });
                        publish(finalized, hashes, BlockRef { hash, height });
                    }
                }
                FollowEvent::Stop => {
                    warn!("chainHead_v1_follow stopped, resubscribing");
                    break;
                }
                FollowEvent::Other => {}
            }

            if finalized.is_closed() {
                return Ok(());
            }
        }
    }

    Ok(())
}

/// A `chainHead_v1_follow` event, with `withRuntime` false.
#[derive(Debug, Deserialize)]
#[serde(
    tag = "event",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum FollowEvent {
    Initialized {
        finalized_block_hashes: Vec<String>,
    },
    NewBlock {
        block_hash: String,
    },
    Finalized {
        finalized_block_hashes: Vec<String>,
    },
    Stop,
    #[serde(other)]
    Other,
}

fn publish(finalized: &watch::Sender<Option<Finalized>>, hashes: Vec<BlockHash>, tip: BlockRef) {
    debug!(hash:% = tip.hash, height = tip.height; "block finalized");
    finalized.send_replace(Some(Finalized { hashes, tip }));
}

/// Unpin the given blocks; a failure only means the node has dropped them already.
async fn unpin<T: Transport>(rpc: &NodeRpc<T>, subscription: &Value, hashes: &[BlockHash]) {
    let mut batch = Batch::default();
    batch.unpin(subscription.to_owned(), hashes);

    match rpc.batch(batch).await.map(|mut results| results.pop()) {
        Ok(Some(Ok(_))) => {}
        Ok(Some(Err(error))) => debug!(error:%; "cannot unpin blocks"),
        Ok(None) => {}
        Err(error) => debug!(error:%; "cannot unpin blocks"),
    }
}

/// The height of the given block, from its header.
async fn header_height<T: Transport>(rpc: &NodeRpc<T>, hash: BlockHash) -> Result<u64, Error> {
    let mut batch = Batch::default();
    batch.header(hash);
    let header = rpc.batch(batch).await?.pop().expect("one result per call");
    let header = header_bytes(header, hash)?;

    Ok(decode_header(&header, hash)?.number)
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

fn block_hashes(hashes: Vec<String>) -> Result<Vec<BlockHash>, Error> {
    hashes.into_iter().map(block_hash_of).collect()
}

fn block_hash_of(hash: String) -> Result<BlockHash, Error> {
    const_hex::decode_to_array(&hash)
        .map(ByteArray)
        .map_err(|_| Error::Hash(hash))
}

/// The Resolve stage: the hash of the block at each height, `None` where the node reports no block
/// or several.
pub async fn resolve<T: Transport>(
    rpc: &NodeRpc<T>,
    heights: RangeInclusive<u64>,
) -> Result<Vec<Option<BlockHash>>, Error> {
    let _timer = Timer::start(metric::RESOLVE_DURATION);
    let batch = heights.fold(Batch::default(), |mut batch, height| {
        batch.hash_by_height(height);
        batch
    });

    rpc.batch(batch)
        .await?
        .into_iter()
        .map(|hashes| {
            let hashes = hashes.map_err(|source| rpc::Error::Call {
                method: method::ARCHIVE_HASH_BY_HEIGHT,
                source,
            })?;
            let hashes = serde_json::from_value::<Vec<String>>(hashes).map_err(|source| {
                rpc::Error::Decode {
                    method: method::ARCHIVE_HASH_BY_HEIGHT,
                    source,
                }
            })?;

            match <[String; 1]>::try_from(hashes) {
                Ok([hash]) => block_hash_of(hash).map(Some),
                Err(_) => Ok(None),
            }
        })
        .collect()
}

/// The Source stage: the raw data of the blocks with the given consecutive hashes, the first at
/// height `start`.
///
/// `parent` is the hash of the block before the first, `None` only if the first is the genesis
/// block; its state supplies the first block's authority set and the authority-set and
/// system-parameter storage hashes the first block is compared with. With `first_of_run`, the first
/// block carries the system parameters whatever its storage says; any other block carries them only
/// if their storage differs from its parent's.
///
/// All calls for all blocks go out together: one set of batches with each block's header, body and
/// state roots, one storage query per block, and one for the parent. Only system parameters, where
/// due, authority sets, where their storage differs from the previous block's, and the metadata of a
/// runtime not seen before take a second round.
pub async fn source<T: Transport>(
    rpc: &NodeRpc<T>,
    metadata: &MetadataCache,
    start: u64,
    hashes: &[BlockHash],
    parent: Option<BlockHash>,
    first_of_run: bool,
) -> Result<Chunk, Error> {
    let _timer = Timer::start(metric::SOURCE_DURATION);
    let batch = hashes.iter().fold(Batch::default(), |mut batch, &hash| {
        batch
            .header(hash)
            .body(hash)
            .call(hash, ZSWAP_STATE_ROOT_FUNCTION, &[])
            .call(hash, LEDGER_STATE_ROOT_FUNCTION, &[]);
        batch
    });

    let block_items = block_storage_items();
    let storage = stream::iter(hashes.iter().copied())
        .map(|hash| query_storage(rpc, hash, block_items.clone()))
        .buffered(rpc.max_calls_in_flight())
        .try_collect::<Vec<_>>();
    let parent_storage = async {
        match parent {
            Some(parent) => query_storage(rpc, parent, parent_storage_items())
                .await
                .map(Some),
            None => Ok(None),
        }
    };

    let (results, (storage, parent_storage)) = try_join(
        async { rpc.batch(batch).await.map_err(Error::from) },
        try_join(storage, parent_storage),
    )
    .await?;

    let parent_authority_set = parent_storage.as_deref().map(authority_set_of);
    let mut previous_authority_set_hashes = parent_storage.as_deref().map(authority_set_hashes);
    let mut previous_system_parameters = parent_storage.as_deref().map(system_parameter_hashes);
    let mut results = results.into_iter();
    let mut sourced = Vec::with_capacity(hashes.len());

    for (i, (&hash, storage)) in hashes.iter().zip(storage).enumerate() {
        let mut next = || results.next().expect("four results per block");
        let header = header_bytes(next(), hash)?;
        let extrinsics = body(next(), hash)?;
        let zswap_state_root = call_value(next(), ZSWAP_STATE_ROOT_FUNCTION, hash)?;
        let ledger_state_root = call_value(next(), LEDGER_STATE_ROOT_FUNCTION, hash)?;

        let height = start + i as u64;
        let decoded_header = decode_header(&header, hash)?;
        if decoded_header.number != height {
            return Err(Error::HeightMismatch {
                hash,
                height,
                header_height: decoded_header.number,
            });
        }
        let protocol_version = decoded_header
            .protocol_version()
            .map_err(|error| Error::ProtocolVersion(hash, error))?
            .ok_or(Error::MissingProtocolVersion(hash))?;
        let parent_hash = ByteArray(decoded_header.parent_hash.0);

        let metadata = metadata
            .get(
                rpc,
                u32::from(protocol_version),
                hash,
                (height != 0).then_some(parent_hash),
            )
            .await?;

        let system_parameter_hashes = system_parameter_hashes(&storage);
        let system_parameters_due = (i == 0 && first_of_run)
            || previous_system_parameters.as_ref() != Some(&system_parameter_hashes);
        previous_system_parameters = Some(system_parameter_hashes);

        let authority_set_hashes = authority_set_hashes(&storage);
        let authority_set_changed =
            previous_authority_set_hashes.as_ref() != Some(&authority_set_hashes);
        previous_authority_set_hashes = Some(authority_set_hashes);

        sourced.push(Sourced {
            hash,
            height,
            header,
            parent_hash,
            zswap_state_root,
            ledger_state_root,
            metadata,
            extrinsics,
            events: events_of(&storage),
            parent_authority_set: None,
            authority_set_changed,
            system_parameters_due,
        });
    }

    let (mut system_parameters, mut authority_sets) = try_join(
        system_parameters(rpc, &sourced),
        changed_authority_sets(rpc, &sourced),
    )
    .await?;
    // Each block's parent set is its predecessor's own: read where it changed, else carried on.
    sourced
        .iter_mut()
        .fold(parent_authority_set, |authority_set, block| {
            block.parent_authority_set = authority_set.clone();
            authority_sets.remove(&block.hash).or(authority_set)
        });
    let genesis = match sourced.first() {
        Some(block) if block.height == 0 => Some(genesis(rpc, block.hash).await?),
        _ => None,
    };

    let chunk = sourced
        .into_iter()
        .zip(
            genesis
                .into_iter()
                .map(Some)
                .chain(std::iter::repeat_with(|| None)),
        )
        .map(|(block, genesis)| {
            let system_parameters = system_parameters.remove(&block.hash);
            block.into_block(system_parameters, genesis)
        })
        .collect::<Chunk>();
    counter!(metric::SOURCED_BLOCK_COUNT).increment(chunk.len() as u64);

    Ok(chunk)
}

/// A block's data before it becomes a [Block].
struct Sourced {
    hash: BlockHash,
    height: u64,
    header: ByteVec,
    parent_hash: BlockHash,
    zswap_state_root: ByteVec,
    ledger_state_root: ByteVec,
    metadata: ArcMetadata,
    extrinsics: Vec<ByteVec>,
    events: ByteVec,
    parent_authority_set: Option<Vec<(ByteVec, ByteVec)>>,
    authority_set_changed: bool,
    system_parameters_due: bool,
}

/// The genesis block's data from the chain spec and from its cNight mapping storage.
struct Genesis {
    ledger_state: ByteVec,
    cnight_mappings: Vec<(ByteVec, ByteVec)>,
}

impl Sourced {
    fn into_block(
        self,
        system_parameters: Option<(ByteVec, ByteVec)>,
        genesis: Option<Genesis>,
    ) -> Block {
        match (genesis, system_parameters) {
            (Some(genesis), Some(system_parameters)) => Block::Genesis {
                hash: self.hash,
                header: self.header,
                zswap_state_root: self.zswap_state_root,
                ledger_state_root: self.ledger_state_root,
                system_parameters,
                metadata: self.metadata,
                ledger_state: genesis.ledger_state,
                cnight_mappings: genesis.cnight_mappings,
                extrinsics: self.extrinsics,
                events: self.events,
            },
            (_, system_parameters) => Block::Block {
                hash: self.hash,
                height: self.height,
                header: self.header,
                zswap_state_root: self.zswap_state_root,
                ledger_state_root: self.ledger_state_root,
                system_parameters,
                metadata: self.metadata,
                parent: Parent {
                    hash: self.parent_hash,
                    authority_set: self.parent_authority_set.unwrap_or_default(),
                },
                extrinsics: self.extrinsics,
                events: self.events,
            },
        }
    }
}

/// The system parameters of every block they are due for, by block hash.
async fn system_parameters<T: Transport>(
    rpc: &NodeRpc<T>,
    sourced: &[Sourced],
) -> Result<HashMap<BlockHash, (ByteVec, ByteVec)>, Error> {
    let due = sourced
        .iter()
        .filter(|block| block.system_parameters_due)
        .map(|block| block.hash)
        .collect::<Vec<_>>();
    if due.is_empty() {
        return Ok(HashMap::new());
    }

    let batch = due.iter().fold(Batch::default(), |mut batch, &hash| {
        batch
            .call(hash, D_PARAMETER_FUNCTION, &[])
            .call(hash, TERMS_AND_CONDITIONS_FUNCTION, &[]);
        batch
    });
    let mut results = rpc.batch(batch).await?.into_iter();

    due.into_iter()
        .map(|hash| {
            let d_parameter = call_value(
                results.next().expect("two results per block"),
                D_PARAMETER_FUNCTION,
                hash,
            )?;
            let terms_and_conditions = call_value(
                results.next().expect("two results per block"),
                TERMS_AND_CONDITIONS_FUNCTION,
                hash,
            )?;
            Ok((hash, (d_parameter, terms_and_conditions)))
        })
        .collect()
}

/// The authority set of every block whose authority-set storage differs from its predecessor's, by
/// block hash.
async fn changed_authority_sets<T: Transport>(
    rpc: &NodeRpc<T>,
    sourced: &[Sourced],
) -> Result<HashMap<BlockHash, Vec<(ByteVec, ByteVec)>>, Error> {
    let changed = sourced
        .iter()
        .filter(|block| block.authority_set_changed)
        .map(|block| block.hash)
        .collect::<Vec<_>>();
    let items = authority_set_items("value").collect::<Vec<_>>();

    stream::iter(changed)
        .map(|hash| {
            let storage = query_storage(rpc, hash, items.clone());
            async move { Ok((hash, authority_set_of(&storage.await?))) }
        })
        .buffered(rpc.max_calls_in_flight())
        .try_collect()
        .await
}

/// The genesis ledger state from the chain spec, and the cNight mappings at genesis.
async fn genesis<T: Transport>(rpc: &NodeRpc<T>, hash: BlockHash) -> Result<Genesis, Error> {
    let properties = async {
        let mut batch = Batch::default();
        batch.chain_spec_properties();
        let properties = rpc
            .batch(batch)
            .await?
            .pop()
            .expect("one result per call")
            .map_err(|source| rpc::Error::Call {
                method: method::CHAIN_SPEC_PROPERTIES,
                source,
            })?;

        let ledger_state = properties
            .get("genesis_state")
            .and_then(Value::as_str)
            .ok_or(Error::MissingGenesisLedgerState)?;
        const_hex::decode(ledger_state)
            .map(ByteVec::from)
            .map_err(|error| Error::Decode {
                what: "genesis ledger state",
                hash,
                source: error.into(),
            })
    };

    let items = CNIGHT_MAPPINGS_ITEMS
        .into_iter()
        .map(|item| storage_query(item, "descendantsValues"))
        .collect();
    let (ledger_state, mappings) = try_join(properties, query_storage(rpc, hash, items)).await?;

    let cnight_mappings = mappings
        .into_iter()
        .filter_map(|item| item.value.map(|value| (item.key, value)))
        .collect();

    Ok(Genesis {
        ledger_state,
        cnight_mappings,
    })
}

/// One result of an `archive_v1_storage` query.
#[derive(Debug)]
struct StorageItem {
    key: ByteVec,
    value: Option<ByteVec>,
    hash: Option<ByteVec>,
}

/// An `archive_v1_storage` event.
#[derive(Debug, Deserialize)]
#[serde(tag = "event", rename_all = "camelCase")]
enum StorageEvent {
    Storage {
        key: String,
        value: Option<String>,
        hash: Option<String>,
    },
    StorageError {
        error: String,
    },
    StorageDone,
}

/// Run an `archive_v1_storage` query at the given block and collect its results.
async fn query_storage<T: Transport>(
    rpc: &NodeRpc<T>,
    hash: BlockHash,
    items: Vec<Value>,
) -> Result<Vec<StorageItem>, Error> {
    let Subscription {
        mut notifications, ..
    } = rpc
        .subscribe(
            method::ARCHIVE_STORAGE,
            vec![hex(hash.0), items.into(), Value::Null],
            method::ARCHIVE_STOP_STORAGE,
        )
        .await?;

    let mut items = vec![];
    loop {
        let event = notifications
            .next()
            .await
            .ok_or_else(|| Error::Storage {
                hash,
                error: "subscription ended before storageDone".to_owned(),
            })?
            .map_err(|error| Error::Storage {
                hash,
                error: error.to_string(),
            })?;

        let event = serde_json::from_value(event).map_err(|error| Error::Decode {
            what: "storage event",
            hash,
            source: error.into(),
        })?;
        match event {
            StorageEvent::Storage {
                key,
                value,
                hash: value_hash,
            } => {
                let decode = |bytes: String| {
                    const_hex::decode(bytes)
                        .map(ByteVec::from)
                        .map_err(|error| Error::Decode {
                            what: "storage item",
                            hash,
                            source: error.into(),
                        })
                };
                let key = decode(key)?;
                let value = value.map(decode).transpose()?;
                let value_hash = value_hash.map(decode).transpose()?;

                let bytes = value
                    .as_ref()
                    .or(value_hash.as_ref())
                    .map(|v| v.len())
                    .unwrap_or(0);
                rpc.counters().record(
                    &format!("{} {}", method::ARCHIVE_STORAGE, storage_label(&key)),
                    1,
                    0,
                    bytes as u64,
                );

                items.push(StorageItem {
                    key,
                    value,
                    hash: value_hash,
                });
            }
            StorageEvent::StorageError { error } => return Err(Error::Storage { hash, error }),
            StorageEvent::StorageDone => return Ok(items),
        }
    }
}

/// The item a storage key belongs to, as `Pallet.Entry`.
fn storage_label(key: &[u8]) -> String {
    std::iter::once(SYSTEM_EVENTS_ITEM)
        .chain(AUTHORITY_SET_ITEMS)
        .chain(SYSTEM_PARAMETERS_ITEMS)
        .chain(CNIGHT_MAPPINGS_ITEMS)
        .find(|&item| key.starts_with(&storage_key(item)))
        .map(|(pallet, entry)| format!("{pallet}.{entry}"))
        .unwrap_or_else(|| "other".to_owned())
}

fn storage_query(item: (&str, &str), query_type: &str) -> Value {
    json!({ "key": hex(storage_key(item)), "type": query_type })
}

/// A block's storage query: its events, and the hashes of its authority set and system parameters.
fn block_storage_items() -> Vec<Value> {
    std::iter::once(storage_query(SYSTEM_EVENTS_ITEM, "value"))
        .chain(authority_set_items("hash"))
        .chain(system_parameter_items())
        .collect()
}

/// A parent's storage query: its authority set, and the hashes of its authority set and system
/// parameters.
fn parent_storage_items() -> Vec<Value> {
    authority_set_items("value")
        .chain(authority_set_items("hash"))
        .chain(system_parameter_items())
        .collect()
}

fn authority_set_items(query_type: &str) -> impl Iterator<Item = Value> {
    AUTHORITY_SET_ITEMS
        .into_iter()
        .map(move |item| storage_query(item, query_type))
}

fn system_parameter_items() -> impl Iterator<Item = Value> {
    SYSTEM_PARAMETERS_ITEMS
        .into_iter()
        .map(|item| storage_query(item, "hash"))
}

fn find_item<'a>(items: &'a [StorageItem], item: (&str, &str)) -> Option<&'a StorageItem> {
    let key = storage_key(item);
    items.iter().find(|stored| *stored.key == key)
}

/// The authority-set items present, as key and value, in [AUTHORITY_SET_ITEMS] order.
fn authority_set_of(items: &[StorageItem]) -> Vec<(ByteVec, ByteVec)> {
    authority_set_entries(items, |item| item.value.as_ref())
}

/// The authority-set items present, as key and value hash, in [AUTHORITY_SET_ITEMS] order.
fn authority_set_hashes(items: &[StorageItem]) -> Vec<(ByteVec, ByteVec)> {
    authority_set_entries(items, |item| item.hash.as_ref())
}

fn authority_set_entries(
    items: &[StorageItem],
    field: impl Fn(&StorageItem) -> Option<&ByteVec>,
) -> Vec<(ByteVec, ByteVec)> {
    AUTHORITY_SET_ITEMS
        .into_iter()
        .filter_map(|item| {
            let key = storage_key(item);
            items
                .iter()
                .filter(|stored| *stored.key == key)
                .find_map(|stored| {
                    field(stored).map(|bytes| (stored.key.to_owned(), bytes.to_owned()))
                })
        })
        .collect()
}

fn system_parameter_hashes(items: &[StorageItem]) -> [Option<ByteVec>; 2] {
    SYSTEM_PARAMETERS_ITEMS.map(|item| find_item(items, item).and_then(|item| item.hash.to_owned()))
}

/// The serialized `System.Events` value; absent storage means no events.
fn events_of(items: &[StorageItem]) -> ByteVec {
    find_item(items, SYSTEM_EVENTS_ITEM)
        .and_then(|item| item.value.to_owned())
        .unwrap_or_else(|| Vec::<()>::new().encode().into())
}

fn body(body: CallResult, hash: BlockHash) -> Result<Vec<ByteVec>, Error> {
    let body = body.map_err(|source| rpc::Error::Call {
        method: method::ARCHIVE_BODY,
        source,
    })?;
    let body = serde_json::from_value::<Option<Vec<String>>>(body)
        .map_err(|source| rpc::Error::Decode {
            method: method::ARCHIVE_BODY,
            source,
        })?
        .ok_or(Error::MissingBody(hash))?;

    body.into_iter()
        .map(|extrinsic| {
            const_hex::decode(extrinsic)
                .map(ByteVec::from)
                .map_err(|error| Error::Decode {
                    what: "body",
                    hash,
                    source: error.into(),
                })
        })
        .collect()
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

/// Metadata by runtime spec version, fetched from the node once per spec version.
#[derive(Default)]
pub struct MetadataCache(Mutex<HashMap<u32, ArcMetadata>>);

impl MetadataCache {
    /// The metadata of the runtime with the given spec version, which executed the given block.
    /// Unless cached, it is fetched from the parent's state, which runs that runtime after a
    /// `set_code` upgrade, else from the block's own state, which runs it after a switch without
    /// one. Fetched metadata must declare the spec version.
    pub async fn get<T: Transport>(
        &self,
        rpc: &NodeRpc<T>,
        spec_version: u32,
        block: BlockHash,
        parent: Option<BlockHash>,
    ) -> Result<ArcMetadata, Error> {
        let mut cache = self.0.lock().await;
        if let Some(metadata) = cache.get(&spec_version) {
            return Ok(metadata.clone());
        }

        let mut found = vec![];
        for at in parent.into_iter().chain([block]) {
            let metadata = fetch_metadata(rpc, at).await?;
            match metadata_spec_version(&metadata) {
                Some(version) if version == spec_version => {
                    let metadata = Arc::new(metadata);
                    cache.insert(spec_version, metadata.clone());
                    return Ok(metadata);
                }
                version => found.push(version),
            }
        }

        Err(Error::MetadataVersion {
            hash: block,
            spec_version,
            found,
        })
    }
}

/// The spec version a runtime's metadata declares in its `System.Version` constant.
pub fn metadata_spec_version(metadata: &Metadata) -> Option<u32> {
    let version = metadata
        .pallet_by_name("System")?
        .constant_by_name("Version")?;
    // `RuntimeVersion` starts with `spec_name`, `impl_name`, `authoring_version`, `spec_version`.
    let (_, _, _, spec_version) =
        <(String, String, u32, u32)>::decode(&mut version.value()).ok()?;
    Some(spec_version)
}

/// Fetch metadata as subxt does: the highest stable version `Metadata_metadata_versions` offers,
/// falling back to `Metadata_metadata`.
async fn fetch_metadata<T: Transport>(rpc: &NodeRpc<T>, at: BlockHash) -> Result<Metadata, Error> {
    let call = |function: &'static str, parameters: Vec<u8>| async move {
        let mut batch = Batch::default();
        batch.call(at, function, &parameters);
        let result = rpc.batch(batch).await?.pop().expect("one result per call");
        call_value(result, function, at)
    };
    let decode_error = |error: parity_scale_codec::Error| Error::Decode {
        what: "metadata",
        hash: at,
        source: error.into(),
    };

    let version = call("Metadata_metadata_versions", vec![])
        .await
        .ok()
        .and_then(|versions| Vec::<u32>::decode(&mut &versions[..]).ok())
        .and_then(|versions| versions.into_iter().filter(|v| *v != u32::MAX).max());

    let metadata = match version {
        Some(version) => {
            let metadata = call("Metadata_metadata_at_version", version.encode()).await?;
            Option::<Vec<u8>>::decode(&mut &metadata[..])
                .map_err(decode_error)?
                .ok_or_else(|| Error::RuntimeCall {
                    function: "Metadata_metadata_at_version",
                    hash: at,
                    error: format!("no metadata of version {version}"),
                })?
        }
        None => {
            let metadata = call("Metadata_metadata", vec![]).await?;
            Vec::<u8>::decode(&mut &metadata[..]).map_err(decode_error)?
        }
    };

    Metadata::decode(&mut &*metadata).map_err(decode_error)
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

/// A planned chunk, and the finalized tip it must link to before it is emitted, if near.
struct Planned {
    spec: ChunkSpec,
    anchor: Option<BlockRef>,
    first_of_run: bool,
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

/// Emission state: the last block emitted, and the verified blocks held back.
#[derive(Default)]
struct Emission {
    emitted: Option<BlockRef>,
    held: Vec<Block>,
}

impl Emission {
    /// The hash the next chunk's first block must have as its parent.
    fn last_hash(&self) -> Option<BlockHash> {
        self.held
            .last()
            .map(Block::hash)
            .or(self.emitted.map(|emitted| emitted.hash))
    }

    /// The height of the first block not yet emitted.
    fn next_height(&self) -> u64 {
        self.emitted.map(|emitted| emitted.height + 1).unwrap_or(0)
    }
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

        let mut batch = Batch::default();
        batch.genesis_hash();
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

    /// The next chunk from height `next`, if the finalized tip has reached it.
    fn plan(&self, next: u64, run_start: u64) -> Option<Planned> {
        let finalized = self.finalized.borrow();
        let finalized = finalized.as_ref()?;
        let mut spec = next_chunk(next, finalized, self.config.chunk_size)?;
        if let Some(end) = self.end {
            let chunk_end = (*spec.heights.end()).min(end);
            spec.heights = *spec.heights.start()..=chunk_end;
            if let Some(hashes) = spec.hashes.as_mut() {
                hashes.truncate((chunk_end - next + 1) as usize);
            }
        }

        let anchor =
            (spec.near && *spec.heights.end() == finalized.tip.height).then_some(finalized.tip);

        Some(Planned {
            spec,
            anchor,
            first_of_run: next == run_start,
        })
    }

    /// Resolve and source a planned chunk.
    fn source_planned(
        &self,
        planned: Planned,
    ) -> impl Future<Output = Result<(Planned, Chunk), Error>> + use<T> {
        let rpc = self.rpc.clone();
        let metadata = self.metadata.clone();

        async move {
            let start = *planned.spec.heights.start();
            let end = *planned.spec.heights.end();

            let (parent, hashes) = match &planned.spec.hashes {
                Some(hashes) if start == 0 => (None, hashes.iter().copied().map(Some).collect()),
                Some(hashes) => {
                    let parent = resolve(&rpc, start - 1..=start - 1).await?.pop().flatten();
                    (Some(parent), hashes.iter().copied().map(Some).collect())
                }
                None if start == 0 => (None, resolve(&rpc, 0..=end).await?),
                None => {
                    let mut hashes = resolve(&rpc, start - 1..=end).await?;
                    let parent = hashes.remove(0);
                    (Some(parent), hashes)
                }
            };

            let hashes = hashes.into_iter().collect::<Option<Vec<_>>>();
            let parent = parent.map(|parent| parent.ok_or(Error::Unresolved(start - 1)));
            let chunk = match (hashes, parent.transpose()) {
                (Some(hashes), Ok(parent)) => {
                    match source(
                        &rpc,
                        &metadata,
                        start,
                        &hashes,
                        parent,
                        planned.first_of_run,
                    )
                    .await
                    {
                        // A hash at the wrong height: leave the chunk empty, so that Verify walks
                        // the parent links.
                        Err(Error::HeightMismatch { .. }) => vec![],
                        chunk => chunk?,
                    }
                }
                // A height without exactly one block: leave the chunk empty, so that Verify walks
                // the parent links.
                _ => vec![],
            };

            Ok((planned, chunk))
        }
    }

    async fn verify_and_emit(
        &mut self,
        emission: &mut Emission,
        planned: Planned,
        chunk: Chunk,
        genesis_hash: Option<BlockHash>,
        run_start: u64,
    ) -> Result<(), Error> {
        let timer = Timer::start(metric::VERIFY_DURATION);
        let expected_parent = emission.last_hash();
        let linked = links(&chunk, &planned.spec.heights, expected_parent, genesis_hash);
        let anchored = planned
            .anchor
            .is_none_or(|anchor| chunk.last().map(Block::hash) == Some(anchor.hash));

        let chunk = if linked && anchored {
            chunk
        } else {
            // Re-source the held blocks and this chunk from hashes walked back from the tip.
            let held_start = emission.held.first().map(Block::height);
            let start = held_start.unwrap_or(*planned.spec.heights.start());
            let end = *planned.spec.heights.end();
            warn!(start, end; "block hashes do not link, walking parent hashes from the tip");

            emission.held.clear();
            let chunk = self.walk_and_source(start, end, run_start).await?;
            let expected_parent = emission.emitted.map(|emitted| emitted.hash);
            if !links(&chunk, &(start..=end), expected_parent, genesis_hash) {
                return Err(Error::Unlinked(start));
            }
            chunk
        };

        emission.held.extend(chunk);

        let emit = if planned.spec.near {
            // Near blocks wait until they link to the finalized tip.
            if planned.anchor.is_some() {
                std::mem::take(&mut emission.held)
            } else {
                vec![]
            }
        } else {
            // Deep blocks wait for their child to confirm them: all but the last.
            let last = emission.held.pop();
            let emit = std::mem::take(&mut emission.held);
            emission.held.extend(last);
            emit
        };
        drop(timer);

        self.emit(emission, emit).await
    }

    /// Source the blocks at heights `start..=end`, with hashes from walking parent hashes back from
    /// the finalized tip.
    async fn walk_and_source(&self, start: u64, end: u64, run_start: u64) -> Result<Chunk, Error> {
        let tip = self
            .finalized
            .borrow()
            .as_ref()
            .map(|finalized| finalized.tip)
            .ok_or(Error::FinalizedEnded)?;

        let mut hashes = vec![];
        let mut hash = tip.hash;
        let mut height = tip.height;
        loop {
            if height <= end {
                hashes.push(hash);
            }
            if height == start || height == 0 {
                break;
            }

            let mut batch = Batch::default();
            batch.header(hash);
            let header = self
                .rpc
                .batch(batch)
                .await?
                .pop()
                .expect("one result per call");
            let header = header_bytes(header, hash)?;
            hash = ByteArray(decode_header(&header, hash)?.parent_hash.0);
            height -= 1;
        }
        hashes.reverse();

        let parent = if start == 0 {
            None
        } else {
            let mut batch = Batch::default();
            batch.header(hashes[0]);
            let header = self
                .rpc
                .batch(batch)
                .await?
                .pop()
                .expect("one result per call");
            let header = header_bytes(header, hashes[0])?;
            Some(ByteArray(decode_header(&header, hashes[0])?.parent_hash.0))
        };

        source(
            &self.rpc,
            &self.metadata,
            start,
            &hashes,
            parent,
            start == run_start,
        )
        .await
    }

    async fn emit(&mut self, emission: &mut Emission, chunk: Chunk) -> Result<(), Error> {
        let Some(last) = chunk.last() else {
            return Ok(());
        };

        emission.emitted = Some(BlockRef {
            hash: last.hash(),
            height: last.height(),
        });
        // A closed channel means the stream is gone; the task is about to be aborted.
        let _timer = Timer::start(metric::EMIT_DURATION);
        let _ = self.chunks.send(Ok(chunk)).await;

        Ok(())
    }
}

/// Whether the chunk holds exactly the given heights, the first block has the expected parent (or
/// is the genesis block with the genesis hash), and every other block's parent is its predecessor.
fn links(
    chunk: &[Block],
    heights: &RangeInclusive<u64>,
    expected_parent: Option<BlockHash>,
    genesis_hash: Option<BlockHash>,
) -> bool {
    let expected_heights = chunk.len() as u64 == heights.end() - heights.start() + 1
        && chunk
            .iter()
            .zip(heights.clone())
            .all(|(block, height)| block.height() == height);

    let mut previous = expected_parent;
    let linked = chunk.iter().all(|block| {
        let linked = match block {
            Block::Genesis { hash, .. } => Some(*hash) == genesis_hash,
            Block::Block { parent, .. } => Some(parent.hash) == previous,
        };
        previous = Some(block.hash());
        linked
    });

    expected_heights && linked
}

#[cfg(test)]
mod tests {
    use crate::{
        infra::subxt_node::{
            fake_node::FakeNode,
            rpc::{Call, NodeRpc, ReconnectPolicy, method},
        },
        pipeline::source::{Finalized, follow_finalized},
    };
    use indexer_common::domain::{BlockHash, ByteArray};
    use parity_scale_codec::Encode;
    use parking_lot::Mutex;
    use serde_json::{Value, json};
    use std::{num::NonZeroUsize, sync::Arc, time::Duration};
    use subxt::{
        config::substrate::{Digest, SubstrateHeader},
        utils::H256,
    };
    use tokio::{sync::watch, task, time::sleep};

    fn hash(n: u8) -> BlockHash {
        ByteArray([n; 32])
    }

    fn hex(n: u8) -> String {
        const_hex::encode_prefixed([n; 32])
    }

    fn header(number: u64) -> Value {
        let header = SubstrateHeader::<H256> {
            parent_hash: H256::zero(),
            number,
            state_root: H256::zero(),
            extrinsics_root: H256::zero(),
            digest: Digest::default(),
        };
        const_hex::encode_prefixed(header.encode()).into()
    }

    /// A node answering `archive_v1_header` with block `n` at height `n`, recording every call.
    fn node(calls: Arc<Mutex<Vec<Call>>>) -> FakeNode {
        FakeNode::new(move |call| {
            calls.lock().push(call.clone());
            match call.method {
                method::ARCHIVE_HEADER => {
                    let hash = call.params[0].as_str().expect("hash param");
                    let n = const_hex::decode(hash).expect("hex hash")[0];
                    Ok(header(n as u64))
                }
                _ => Ok(Value::Null),
            }
        })
    }

    fn node_rpc(node: FakeNode) -> NodeRpc<FakeNode> {
        NodeRpc::new(
            node,
            NonZeroUsize::new(64).unwrap(),
            NonZeroUsize::new(4).unwrap(),
            ReconnectPolicy {
                max_delay: Duration::from_millis(10),
                max_attempts: 3,
            },
        )
    }

    async fn latest(finalized: &mut watch::Receiver<Option<Finalized>>, height: u64) -> Finalized {
        let finalized = tokio::time::timeout(
            Duration::from_secs(5),
            finalized.wait_for(|finalized| {
                finalized
                    .as_ref()
                    .is_some_and(|finalized| finalized.tip.height == height)
            }),
        )
        .await
        .expect("finalized in time")
        .expect("sender alive");

        finalized.clone().expect("finalized")
    }

    #[tokio::test]
    async fn test_signal() {
        let calls = Arc::new(Mutex::new(vec![]));
        let node = node(calls.clone()).with_subscriptions(vec![vec![
            json!({ "event": "initialized", "finalizedBlockHashes": [hex(1), hex(2)] }),
            json!({ "event": "newBlock", "blockHash": hex(3), "parentBlockHash": hex(2) }),
            json!({ "event": "bestBlockChanged", "bestBlockHash": hex(3) }),
            json!({ "event": "newBlock", "blockHash": hex(4), "parentBlockHash": hex(3) }),
            json!({ "event": "finalized", "finalizedBlockHashes": [hex(3), hex(4)], "prunedBlockHashes": [] }),
        ]]);
        let rpc = node_rpc(node);
        let (sender, mut receiver) = watch::channel(None);

        let task =
            task::spawn(
                async move { follow_finalized(&rpc, Duration::from_secs(5), &sender).await },
            );
        let finalized = latest(&mut receiver, 4).await;
        task.abort();

        assert_eq!(finalized.hashes, vec![hash(3), hash(4)]);
        assert_eq!(finalized.tip.hash, hash(4));
        assert_eq!(finalized.tip.height, 4);

        let calls = calls.lock();
        let headers = calls
            .iter()
            .filter(|call| call.method == method::ARCHIVE_HEADER)
            .count();
        assert_eq!(headers, 1);
        let unpinned = calls
            .iter()
            .filter(|call| call.method == method::CHAIN_HEAD_UNPIN)
            .map(|call| call.params[1].clone())
            .collect::<Vec<_>>();
        assert_eq!(
            unpinned,
            vec![json!([hex(1), hex(2)]), json!([hex(3)]), json!([hex(4)])]
        );
    }

    #[tokio::test]
    async fn test_skipped_update() {
        let calls = Arc::new(Mutex::new(vec![]));
        let node = node(calls).with_subscriptions(vec![vec![
            json!({ "event": "initialized", "finalizedBlockHashes": [hex(1)] }),
            json!({ "event": "finalized", "finalizedBlockHashes": [hex(2), hex(3)], "prunedBlockHashes": [] }),
            json!({ "event": "finalized", "finalizedBlockHashes": [hex(4)], "prunedBlockHashes": [] }),
        ]]);
        let rpc = node_rpc(node);
        let (sender, mut receiver) = watch::channel(None);

        let task =
            task::spawn(
                async move { follow_finalized(&rpc, Duration::from_secs(5), &sender).await },
            );
        let finalized = latest(&mut receiver, 4).await;
        task.abort();

        // Only the newest value is kept: its hashes cover its own heights only.
        assert_eq!(finalized.hashes, vec![hash(4)]);
        assert_eq!(finalized.tip.height, 4);
    }

    #[tokio::test]
    async fn test_stop_resubscribes() {
        let calls = Arc::new(Mutex::new(vec![]));
        let node = node(calls).with_subscriptions(vec![
            vec![
                json!({ "event": "initialized", "finalizedBlockHashes": [hex(1)] }),
                json!({ "event": "stop" }),
            ],
            vec![json!({ "event": "initialized", "finalizedBlockHashes": [hex(2)] })],
        ]);
        let rpc = node_rpc(node.clone());
        let (sender, mut receiver) = watch::channel(None);

        let task =
            task::spawn(
                async move { follow_finalized(&rpc, Duration::from_secs(5), &sender).await },
            );
        latest(&mut receiver, 2).await;
        task.abort();

        assert_eq!(node.subscribes(), 2);
    }

    #[tokio::test]
    async fn test_watchdog() {
        let calls = Arc::new(Mutex::new(vec![]));
        let new_blocks = (10..20)
            .map(|n| json!({ "event": "newBlock", "blockHash": hex(n), "parentBlockHash": hex(n - 1) }))
            .collect::<Vec<_>>();
        let node = node(calls)
            .with_notification_interval(Duration::from_millis(20))
            .with_subscriptions(vec![new_blocks]);
        let rpc = node_rpc(node.clone());
        let (sender, _receiver) = watch::channel(None);

        let task = task::spawn(async move {
            follow_finalized(&rpc, Duration::from_millis(150), &sender).await
        });

        // Ten `newBlock` events 20 ms apart keep the subscription alive past the timeout, even
        // though no block is finalized.
        sleep(Duration::from_millis(300)).await;
        assert_eq!(node.subscribes(), 1);

        // Silence for the timeout renews it exactly once.
        sleep(Duration::from_millis(125)).await;
        assert_eq!(node.subscribes(), 2);

        task.abort();
    }
}

#[cfg(test)]
mod source_tests {
    use crate::{
        domain::BlockRef,
        infra::subxt_node::{
            fake_node::FakeNode,
            rpc::{Call, CallError, CallResult, NodeRpc, ReconnectPolicy, method},
        },
        pipeline::source::{
            AUTHORITY_SET_ITEMS, Block, CNIGHT_MAPPINGS_ITEMS, Chunk, Config, Error,
            LEDGER_STATE_ROOT_FUNCTION, MetadataCache, SYSTEM_EVENTS_ITEM, SYSTEM_PARAMETERS_ITEMS,
            Source, ZSWAP_STATE_ROOT_FUNCTION, metadata_spec_version, resolve, source, storage_key,
        },
    };
    use futures::{StreamExt, TryStreamExt};
    use indexer_common::domain::{BlockHash, ByteArray};
    use parity_scale_codec::Encode;
    use parking_lot::Mutex;
    use serde_json::{Value, json};
    use std::{
        fs,
        num::NonZeroUsize,
        path::Path,
        sync::{Arc, LazyLock},
        time::Duration,
    };
    use subxt::{
        Metadata,
        config::substrate::{Digest, DigestItem, SubstrateHeader},
        utils::H256,
    };
    use tokio::time::{sleep, timeout};

    /// Spec versions of the 1.0.300 and 2.1 runtimes.
    const SPEC_VERSION_1_0: u32 = 1_000_300;
    const SPEC_VERSION: u32 = 2_001_000;
    /// Marks a fork sibling's hash.
    const FORK: u8 = 0xff;

    static METADATA: LazyLock<Vec<u8>> = LazyLock::new(|| node_metadata("2.1.0-rc.4"));
    static METADATA_1_0: LazyLock<Vec<u8>> = LazyLock::new(|| node_metadata("1.0.300"));

    fn node_metadata(node_version: &str) -> Vec<u8> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../.node")
            .join(node_version)
            .join("metadata.scale");
        fs::read(path).expect("node metadata can be read")
    }

    /// The canonical block at height `n` has hash `hash(n)`; a fork sibling at height `n` has
    /// `fork(n)`, with the same parent.
    fn hash(n: u64) -> BlockHash {
        let mut hash = [0; 32];
        hash[..8].copy_from_slice(&n.to_le_bytes());
        ByteArray(hash)
    }

    fn fork(n: u64) -> BlockHash {
        let mut hash = hash(n);
        hash.0[31] = FORK;
        hash
    }

    fn height_of(hash: &[u8]) -> u64 {
        u64::from_le_bytes(hash[..8].try_into().expect("8 bytes"))
    }

    fn hex(bytes: impl AsRef<[u8]>) -> Value {
        const_hex::encode_prefixed(bytes).into()
    }

    fn bytes_of(param: &Value) -> Vec<u8> {
        const_hex::decode(param.as_str().expect("hex param")).expect("hex")
    }

    fn header(n: u64, spec_version: u32) -> Vec<u8> {
        SubstrateHeader::<H256> {
            parent_hash: H256(hash(n.saturating_sub(1)).0),
            number: n,
            state_root: H256::zero(),
            extrinsics_root: H256::zero(),
            digest: Digest {
                logs: vec![DigestItem::Consensus(*b"MNSV", spec_version.encode())],
            },
        }
        .encode()
    }

    fn success(bytes: impl AsRef<[u8]>) -> CallResult {
        Ok(json!({ "success": true, "value": hex(bytes) }))
    }

    /// A chain answering archive calls for any height, with system parameters `system_parameters`
    /// by height (1 beyond its end), fork siblings resolved at the `forks` heights, and failing
    /// headers at the `failing` heights. Blocks run the 2.1 runtime, except that headers below
    /// `stamped_2_1_from` are stamped 1.0.300 and states below `state_2_1_from` run 1.0.300.
    #[derive(Default)]
    struct Chain {
        system_parameters: Vec<u8>,
        /// The authority set at each height, as a set number; past the end, each block's own.
        authority_sets: Vec<u8>,
        forks: Vec<u64>,
        failing: Vec<u64>,
        stamped_2_1_from: Option<u64>,
        state_2_1_from: Option<u64>,
        calls: Mutex<Vec<Call>>,
        /// The heights at which authority-set values were queried.
        authority_set_reads: Mutex<Vec<u64>>,
    }

    impl Chain {
        /// The authority set at the given block: one authority, the set number repeated or the
        /// block hash.
        fn authority_set(&self, n: u64, block: &[u8]) -> [u8; 32] {
            match self.authority_sets.get(n as usize) {
                Some(&set) => [set; 32],
                None => block.try_into().unwrap(),
            }
        }

        fn stamped_spec_version(&self, n: u64) -> u32 {
            match self.stamped_2_1_from {
                Some(from) if n < from => SPEC_VERSION_1_0,
                _ => SPEC_VERSION,
            }
        }

        fn state_metadata(&self, n: u64) -> &'static [u8] {
            match self.state_2_1_from {
                Some(from) if n < from => &METADATA_1_0,
                _ => &METADATA,
            }
        }

        fn respond(&self, call: &Call) -> CallResult {
            self.calls.lock().push(call.clone());
            match call.method {
                method::ARCHIVE_HASH_BY_HEIGHT => {
                    let n = call.params[0].as_u64().expect("height");
                    match n {
                        // No block, and two blocks, at these heights.
                        1_000_000 => Ok(json!([])),
                        1_000_001 => Ok(json!([hex(hash(n)), hex(fork(n))])),
                        n if self.forks.contains(&n) => Ok(json!([hex(fork(n))])),
                        n => Ok(json!([hex(hash(n))])),
                    }
                }
                method::ARCHIVE_GENESIS_HASH => Ok(hex(hash(0))),
                method::ARCHIVE_HEADER => {
                    let n = height_of(&bytes_of(&call.params[0]));
                    if self.failing.contains(&n) {
                        Err(CallError {
                            code: -32000,
                            message: "cannot build block".to_owned(),
                        })
                    } else {
                        Ok(hex(header(n, self.stamped_spec_version(n))))
                    }
                }
                method::ARCHIVE_BODY => Ok(json!([hex(bytes_of(&call.params[0]))])),
                method::ARCHIVE_CALL => match call.params[1].as_str().expect("function") {
                    "Metadata_metadata_versions" => success(vec![14u32, 15, u32::MAX].encode()),
                    "Metadata_metadata_at_version" => {
                        let n = height_of(&bytes_of(&call.params[0]));
                        success(Some(self.state_metadata(n).to_vec()).encode())
                    }
                    function => success(function.as_bytes()),
                },
                method::CHAIN_SPEC_PROPERTIES => Ok(json!({ "genesis_state": "0xabcd" })),
                _ => Ok(Value::Null),
            }
        }

        fn storage(&self, params: &[Value]) -> Vec<Value> {
            let block = bytes_of(&params[0]);
            let n = height_of(&block);
            let system_parameters = self.system_parameters.get(n as usize).copied().unwrap_or(1);

            params[1]
                .as_array()
                .expect("items")
                .iter()
                .filter_map(|item| {
                    let key = bytes_of(&item["key"]);
                    let is = |item| key == storage_key(item);

                    if is(SYSTEM_EVENTS_ITEM) {
                        Some(json!({ "event": "storage", "key": item["key"], "value": hex(&block) }))
                    } else if is(AUTHORITY_SET_ITEMS[0]) {
                        let authority = self.authority_set(n, &block);
                        if item["type"] == "hash" {
                            Some(json!({ "event": "storage", "key": item["key"], "hash": hex(authority) }))
                        } else {
                            self.authority_set_reads.lock().push(n);
                            let authorities = vec![authority].encode();
                            Some(json!({ "event": "storage", "key": item["key"], "value": hex(authorities) }))
                        }
                    } else if SYSTEM_PARAMETERS_ITEMS.into_iter().any(is) {
                        let value_hash = [system_parameters; 32];
                        Some(json!({ "event": "storage", "key": item["key"], "hash": hex(value_hash) }))
                    } else if is(CNIGHT_MAPPINGS_ITEMS[1]) {
                        let mut key = key.clone();
                        key.push(7);
                        Some(json!({ "event": "storage", "key": hex(key), "value": "0x05" }))
                    } else {
                        None
                    }
                })
                .chain([json!({ "event": "storageDone" })])
                .collect()
        }

        fn node(self) -> (Arc<Self>, FakeNode) {
            let chain = Arc::new(self);
            let node = FakeNode::new({
                let chain = chain.clone();
                move |call| chain.respond(call)
            })
            .with_subscribe({
                let chain = chain.clone();
                move |method, params| {
                    (method == method::ARCHIVE_STORAGE).then(|| chain.storage(params))
                }
            });

            (chain, node)
        }

        /// The block hashes of every call of the given runtime function.
        fn calls_of(&self, function: &str) -> Vec<BlockHash> {
            self.calls
                .lock()
                .iter()
                .filter(|call| call.method == method::ARCHIVE_CALL && call.params[1] == function)
                .map(|call| ByteArray(bytes_of(&call.params[0]).try_into().unwrap()))
                .collect()
        }

        /// The heights of every `archive_v1_hashByHeight` call, in order.
        fn resolved_heights(&self) -> Vec<u64> {
            self.calls
                .lock()
                .iter()
                .filter(|call| call.method == method::ARCHIVE_HASH_BY_HEIGHT)
                .map(|call| call.params[0].as_u64().expect("height"))
                .collect()
        }
    }

    /// `chainHead_v1_follow` events: initialized at `tip`, then finalized one block at a time up
    /// to `last`.
    fn follow(tip: u64, last: u64) -> Vec<Value> {
        std::iter::once(json!({ "event": "initialized", "finalizedBlockHashes": [hex(hash(tip))] }))
            .chain((tip + 1..=last).map(|n| {
                json!({ "event": "finalized", "finalizedBlockHashes": [hex(hash(n))], "prunedBlockHashes": [] })
            }))
            .collect()
    }

    fn size(n: usize) -> NonZeroUsize {
        NonZeroUsize::new(n).unwrap()
    }

    fn config(chunk_size: usize, chunks_ahead: usize) -> Config {
        Config {
            chunk_size: size(chunk_size),
            chunks_ahead: size(chunks_ahead),
            rpc_batch_size: size(64),
            rpc_batches_in_flight: size(4),
            recovery_timeout: Duration::from_secs(5),
            reconnect_policy: ReconnectPolicy {
                max_delay: Duration::from_millis(10),
                max_attempts: 3,
            },
        }
    }

    fn node_rpc(node: FakeNode, batch_size: usize, in_flight: usize) -> NodeRpc<FakeNode> {
        NodeRpc::new(
            node,
            size(batch_size),
            size(in_flight),
            config(1, 1).reconnect_policy,
        )
    }

    fn hashes(heights: std::ops::RangeInclusive<u64>) -> Vec<BlockHash> {
        heights.map(hash).collect()
    }

    fn start(height: u64) -> Option<BlockRef> {
        Some(BlockRef {
            hash: hash(height),
            height,
        })
    }

    /// Run the pipeline to `end` and collect its blocks.
    async fn run_to_end(source: &Source<FakeNode>, start: Option<BlockRef>, end: u64) -> Chunk {
        let (chunks, _finalized) = source.run(start, Some(end));
        timeout(Duration::from_secs(10), chunks.try_concat())
            .await
            .expect("pipeline finishes in time")
            .expect("pipeline succeeds")
    }

    /// The heights and hashes of the blocks, and whether each block's parent is its predecessor.
    fn assert_canonical(blocks: &[Block], heights: std::ops::RangeInclusive<u64>) {
        assert_eq!(
            blocks.iter().map(Block::height).collect::<Vec<_>>(),
            heights.clone().collect::<Vec<_>>()
        );
        assert_eq!(
            blocks.iter().map(Block::hash).collect::<Vec<_>>(),
            heights.map(hash).collect::<Vec<_>>()
        );
        for block in blocks {
            if let Block::Block { height, parent, .. } = block {
                assert_eq!(parent.hash, hash(height - 1));
            }
        }
    }

    #[tokio::test]
    async fn test_resolve() {
        let (_, node) = Chain::default().node();
        let rpc = node_rpc(node, 64, 4);

        let resolved = resolve(&rpc, 999_998..=1_000_001)
            .await
            .expect("hashes resolve");

        assert_eq!(
            resolved,
            vec![Some(hash(999_998)), Some(hash(999_999)), None, None]
        );
    }

    #[tokio::test]
    async fn test_source_from_genesis() {
        let (chain, node) = Chain::default().node();
        let rpc = node_rpc(node, 64, 4);
        let metadata = MetadataCache::default();

        let chunk = source(&rpc, &metadata, 0, &hashes(0..=3), None, true)
            .await
            .expect("chunk is sourced");

        assert_eq!(chunk.len(), 4);
        let Block::Genesis {
            hash: genesis_hash,
            ledger_state,
            cnight_mappings,
            ..
        } = &chunk[0]
        else {
            panic!("block 0 is genesis");
        };
        assert_eq!(*genesis_hash, hash(0));
        assert_eq!(**ledger_state, [0xab, 0xcd]);
        assert_eq!(cnight_mappings.len(), 1);

        for (n, block) in chunk.iter().enumerate().skip(1) {
            let n = n as u64;
            let Block::Block {
                height,
                parent,
                extrinsics,
                events,
                ..
            } = block
            else {
                panic!("block {n} is not genesis");
            };
            assert_eq!(*height, n);
            assert_eq!(parent.hash, hash(n - 1));
            // The parent's authority set: its own Aura authorities.
            assert_eq!(parent.authority_set.len(), 1);
            assert_eq!(*parent.authority_set[0].1, vec![hash(n - 1).0].encode());
            assert_eq!(extrinsics.len(), 1);
            assert_eq!(*extrinsics[0], hash(n).0);
            assert_eq!(**events, hash(n).0);
        }

        // Metadata is fetched once for the whole run; `Core_version` never.
        assert_eq!(chain.calls_of("Metadata_metadata_at_version").len(), 1);
        assert!(chain.calls_of("Core_version").is_empty());
    }

    #[tokio::test]
    async fn test_system_parameters_change_only() {
        // The system parameters change at block 3 and again at block 6.
        let (chain, node) = Chain {
            system_parameters: vec![1, 1, 1, 2, 2, 2, 3],
            ..Default::default()
        }
        .node();
        let rpc = node_rpc(node, 64, 4);
        let metadata = MetadataCache::default();

        let first = source(&rpc, &metadata, 1, &hashes(1..=3), Some(hash(0)), true)
            .await
            .expect("first chunk is sourced");
        let second = source(&rpc, &metadata, 4, &hashes(4..=6), Some(hash(3)), false)
            .await
            .expect("second chunk is sourced");

        let carried = first
            .iter()
            .chain(&second)
            .map(|block| match block {
                Block::Block {
                    system_parameters, ..
                } => system_parameters.is_some(),
                Block::Genesis { .. } => true,
            })
            .collect::<Vec<_>>();
        // First of the run, then the changes at 3 and 6, also across the chunk boundary.
        assert_eq!(carried, vec![true, false, true, false, false, true]);
        for function in [
            "SystemParametersApi_get_d_parameter",
            "SystemParametersApi_get_terms_and_conditions",
        ] {
            assert_eq!(chain.calls_of(function), vec![hash(1), hash(3), hash(6)]);
        }
    }

    #[tokio::test]
    async fn test_authority_set_change_only() {
        // The authority set changes at block 3, the last of the first chunk, and at block 4, the
        // first of the second.
        let (chain, node) = Chain {
            authority_sets: vec![1, 1, 1, 2, 3, 3, 3],
            ..Default::default()
        }
        .node();
        let rpc = node_rpc(node, 64, 4);
        let metadata = MetadataCache::default();

        let first = source(&rpc, &metadata, 1, &hashes(1..=3), Some(hash(0)), true)
            .await
            .expect("first chunk is sourced");
        let second = source(&rpc, &metadata, 4, &hashes(4..=6), Some(hash(3)), false)
            .await
            .expect("second chunk is sourced");

        let parent_sets = first
            .iter()
            .chain(&second)
            .map(|block| match block {
                Block::Block { parent, .. } => parent.authority_set.clone(),
                Block::Genesis { .. } => panic!("no genesis"),
            })
            .collect::<Vec<_>>();
        let expected = [1u8, 1, 1, 2, 3, 3].map(|set| {
            vec![(
                storage_key(AUTHORITY_SET_ITEMS[0]).to_vec().into(),
                vec![[set; 32]].encode().into(),
            )]
        });
        assert_eq!(parent_sets, expected);

        // Values are read at each chunk's parent and where the set changed, never elsewhere.
        let mut reads = chain.authority_set_reads.lock().clone();
        reads.sort();
        assert_eq!(reads, vec![0, 3, 3, 4]);
    }

    #[tokio::test]
    async fn test_no_serial_fetch() {
        let (_, node) = Chain::default().node();
        let node = node.with_delay(Duration::from_millis(20));
        let rpc = node_rpc(node.clone(), 8, 4);
        let metadata = MetadataCache::default();

        source(&rpc, &metadata, 1, &hashes(1..=32), Some(hash(0)), false)
            .await
            .expect("chunk is sourced");

        // 32 blocks at 4 entries each in batches of 8: 16 batches, 4 at a time, none waiting on
        // another block's result.
        assert_eq!(node.max_in_flight(), 4);
        assert!(node.batch_sizes().iter().all(|&size| size <= 8));
    }

    /// The metadata spec versions of the blocks of a chunk sourced at heights 5 to 7.
    async fn metadata_versions(chain: Chain) -> Result<Vec<Option<u32>>, Error> {
        let (_, node) = chain.node();
        let rpc = node_rpc(node, 64, 4);
        let chunk = source(
            &rpc,
            &MetadataCache::default(),
            5,
            &hashes(5..=7),
            Some(hash(4)),
            true,
        )
        .await?;

        Ok(chunk
            .iter()
            .map(|block| match block {
                Block::Block { metadata, .. } | Block::Genesis { metadata, .. } => {
                    metadata_spec_version(metadata)
                }
            })
            .collect())
    }

    #[tokio::test]
    async fn test_metadata_after_set_code_upgrade() {
        // `set_code` lands in block 5: its state already runs 2.1, but block 6 is the first one
        // executed, and stamped, by 2.1. Each block's runtime is in its parent's state.
        let versions = metadata_versions(Chain {
            stamped_2_1_from: Some(6),
            state_2_1_from: Some(5),
            ..Default::default()
        })
        .await
        .expect("chunk is sourced");

        assert_eq!(
            versions,
            vec![
                Some(SPEC_VERSION_1_0),
                Some(SPEC_VERSION),
                Some(SPEC_VERSION)
            ]
        );
    }

    #[tokio::test]
    async fn test_enactment() {
        // `set_code` lands in block 5, as at mainnet 1,774,491: block 5 is the last executed by
        // 1.0.300 and block 6 the first executed by 2.1.
        let (chain, node) = Chain {
            stamped_2_1_from: Some(6),
            state_2_1_from: Some(5),
            ..Default::default()
        }
        .node();
        let rpc = node_rpc(node, 64, 4);
        source(
            &rpc,
            &MetadataCache::default(),
            5,
            &hashes(5..=7),
            Some(hash(4)),
            true,
        )
        .await
        .expect("chunk is sourced");

        // One metadata per runtime, each from the state of the parent of its first block.
        assert_eq!(
            chain.calls_of("Metadata_metadata_at_version"),
            vec![hash(4), hash(5)]
        );
        // No runtime version lookup, and the roots come from each block itself, never retried at
        // its parent or waiting for its successor.
        assert!(chain.calls_of("Core_version").is_empty());
        for function in [ZSWAP_STATE_ROOT_FUNCTION, LEDGER_STATE_ROOT_FUNCTION] {
            assert_eq!(chain.calls_of(function), hashes(5..=7));
        }
        assert!(
            chain
                .calls
                .lock()
                .iter()
                .all(|call| call.params.first() != Some(&hex(hash(8))))
        );
    }

    #[tokio::test]
    async fn test_metadata_after_switch_without_set_code() {
        // Block 6 is stamped 2.1 while its parent's state still runs 1.0.300, as on a chain that
        // switched runtimes like a hard fork: the metadata comes from block 6's own state.
        let versions = metadata_versions(Chain {
            stamped_2_1_from: Some(6),
            state_2_1_from: Some(6),
            ..Default::default()
        })
        .await
        .expect("chunk is sourced");

        assert_eq!(
            versions,
            vec![
                Some(SPEC_VERSION_1_0),
                Some(SPEC_VERSION),
                Some(SPEC_VERSION)
            ]
        );
    }

    #[tokio::test]
    async fn test_metadata_of_another_runtime_is_rejected() {
        // Every block is stamped 2.1, but no state runs it.
        let error = metadata_versions(Chain {
            state_2_1_from: Some(u64::MAX),
            ..Default::default()
        })
        .await
        .expect_err("no metadata of the stamped runtime");

        assert!(matches!(
            error,
            Error::MetadataVersion {
                spec_version: SPEC_VERSION,
                ref found,
                ..
            } if *found == vec![Some(SPEC_VERSION_1_0), Some(SPEC_VERSION_1_0)]
        ));
    }

    #[tokio::test]
    async fn test_genesis_catch_up() {
        // Finalized at 10 when following starts; the tip keeps moving to 20 while catching up.
        let (_, node) = Chain::default().node();
        let node = node
            .with_notification_interval(Duration::from_millis(5))
            .with_subscriptions(vec![follow(10, 20)]);
        let source = Source::new(node.clone(), config(4, 2));

        let blocks = run_to_end(&source, None, 20).await;

        assert!(matches!(blocks[0], Block::Genesis { .. }));
        assert_canonical(&blocks, 0..=20);
        let follows = node
            .subscribed()
            .into_iter()
            .filter(|method| *method == method::CHAIN_HEAD_FOLLOW)
            .count();
        assert_eq!(follows, 1, "no resubscription while the tip moves");
    }

    #[tokio::test]
    async fn test_ordering() {
        // Chunks of lower heights answer slower, so later chunks complete first.
        let (_, node) = Chain::default().node();
        let node = node
            .with_subscriptions(vec![follow(1_000, 1_000)])
            .with_delay_for(|calls| {
                let first_height = calls
                    .iter()
                    .find(|call| call.method == method::ARCHIVE_HEADER)
                    .map(|call| height_of(&bytes_of(&call.params[0])))
                    .unwrap_or(0);
                Duration::from_millis(160 - first_height.min(160))
            });
        let source = Source::new(node, config(10, 4));

        let blocks = run_to_end(&source, start(99), 150).await;

        assert_canonical(&blocks, 100..=150);
    }

    #[tokio::test]
    async fn test_anchoring_deep_fork_falls_back_to_parent_walk() {
        // A fork sibling resolves at the last height of a deep chunk; its child exposes it.
        let (_, node) = Chain {
            forks: vec![109],
            ..Default::default()
        }
        .node();
        let node = node.with_subscriptions(vec![follow(1_000, 1_000)]);
        let source = Source::new(node, config(10, 2));

        let blocks = run_to_end(&source, start(99), 130).await;

        assert_canonical(&blocks, 100..=130);
    }

    #[tokio::test]
    async fn test_anchoring_near_fork_falls_back_to_parent_walk() {
        // A fork sibling resolves mid-chunk within the margin.
        let (_, node) = Chain {
            forks: vec![955],
            ..Default::default()
        }
        .node();
        let node = node.with_subscriptions(vec![follow(1_000, 1_000)]);
        let source = Source::new(node, config(20, 2));

        let blocks = run_to_end(&source, start(949), 1_000).await;

        assert_canonical(&blocks, 950..=1_000);
    }

    #[tokio::test]
    async fn test_chunk_overlap() {
        let (_, node) = Chain::default().node();
        let node = node
            .with_subscriptions(vec![follow(1_000, 1_000)])
            .with_delay(Duration::from_millis(10));
        let source = Source::new(node.clone(), config(10, 2));
        let (mut chunks, _finalized) = source.run(start(99), Some(200));

        chunks
            .next()
            .await
            .expect("first chunk")
            .expect("first chunk is sourced");
        let batches = node.batch_sizes().len();

        // While the consumer sleeps on the first chunk, the next chunks are sourced.
        sleep(Duration::from_millis(200)).await;
        assert!(node.batch_sizes().len() > batches);
    }

    #[tokio::test]
    async fn test_failing_block_is_not_skipped_and_refetched() {
        // The block at height 105 cannot be built: the stream yields an error, then resumes after
        // the last block it yielded, so the very same block is fetched again.
        let (chain, node) = Chain {
            failing: vec![105],
            ..Default::default()
        }
        .node();
        let node = node.with_subscriptions(vec![follow(1_000, 1_000)]);
        let source = Source::new(node, config(2, 1));
        let (chunks, _finalized) = source.run(start(99), None);

        let items = timeout(Duration::from_secs(10), chunks.take(5).collect::<Vec<_>>())
            .await
            .expect("items in time");

        let heights = items
            .iter()
            .filter_map(|item| item.as_ref().ok())
            .flatten()
            .map(Block::height)
            .collect::<Vec<_>>();
        assert_eq!(heights, (100..=103).collect::<Vec<_>>());
        let errors = items.iter().filter(|item| item.is_err()).count();
        assert_eq!(errors, 2);

        // After the first error, resolving restarts at the parent of the first block not yielded.
        let resolved = chain.resolved_heights();
        assert!(resolved.contains(&102));
        assert!(matches!(items[2], Err(Error::Rpc(_))));
    }

    #[tokio::test]
    async fn test_shutdown() {
        let (_, node) = Chain::default().node();
        let node = node.with_subscriptions(vec![follow(1_000, 1_000)]);
        let source = Source::new(node.clone(), config(10, 2));
        let (mut chunks, _finalized) = source.run(start(99), None);

        chunks
            .next()
            .await
            .expect("first chunk")
            .expect("first chunk is sourced");
        assert!(node.live_subscriptions() > 0);

        drop(chunks);
        sleep(Duration::from_millis(100)).await;

        assert_eq!(node.live_subscriptions(), 0);
    }

    #[test]
    fn test_storage_keys() {
        // Published key of `System.Events`.
        assert_eq!(
            const_hex::encode(storage_key(SYSTEM_EVENTS_ITEM)),
            "26aa394eea5630e07c48ae0c9558cef780d41e5e16056765bc8461851072c9d7"
        );

        let node_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.node");
        let node_versions = fs::read_to_string(node_dir.join("../NODE_VERSIONS"))
            .expect("NODE_VERSIONS can be read");

        for node_version in node_versions
            .lines()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            let metadata = fs::read(node_dir.join(node_version).join("metadata.scale"))
                .expect("metadata can be read");
            let metadata = <Metadata as parity_scale_codec::Decode>::decode(&mut &*metadata)
                .expect("metadata can be decoded");

            let has = |(pallet, entry): (&str, &str)| {
                metadata
                    .pallet_by_name(pallet)
                    .and_then(|pallet| pallet.storage())
                    .and_then(|storage| storage.entry_by_name(entry))
                    .is_some()
            };

            assert!(has(SYSTEM_EVENTS_ITEM), "{node_version}: System.Events");
            assert!(
                has(AUTHORITY_SET_ITEMS[0]),
                "{node_version}: Aura.Authorities"
            );
            for item in SYSTEM_PARAMETERS_ITEMS {
                assert!(has(item), "{node_version}: {item:?}");
            }
            assert!(
                CNIGHT_MAPPINGS_ITEMS.into_iter().any(has),
                "{node_version}: cNight mappings"
            );
        }
    }
}
