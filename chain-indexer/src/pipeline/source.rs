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
        rpc::{self, Batch, CallResult, NodeRpc, Subscription, Transport, hex, method},
    },
};
use futures::{StreamExt, TryStreamExt, future::try_join, stream};
use indexer_common::domain::{BlockHash, ByteArray, ByteVec, ProtocolVersionError};
use log::{debug, warn};
use parity_scale_codec::{Decode, Encode};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::HashMap, ops::RangeInclusive, sync::Arc, time::Duration};
use subxt::{
    ArcMetadata, Metadata, config::substrate::SubstrateHeader,
    ext::frame_decode::storage::encode_storage_key_prefix, utils::H256,
};
use thiserror::Error;
use tokio::{
    sync::{Mutex, watch},
    time::timeout,
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
    /// The genesis block, built from the chain spec: it has no parent, extrinsics or events.
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
}

/// The Finalized stage: follow the node's finalized blocks with `chainHead_v1_follow` and publish
/// each [Finalized] to `finalized`. Every block the subscription reports is unpinned as soon as it
/// is reported; nothing else is fetched except one header per subscription, for the tip's height.
/// The subscription is renewed on `stop`, when it ends, and when no event arrives within
/// `recovery_timeout`. Returns once `finalized` has no receivers.
pub async fn follow_finalized<T>(
    rpc: &NodeRpc<T>,
    recovery_timeout: Duration,
    finalized: &watch::Sender<Option<Finalized>>,
) -> Result<(), Error>
where
    T: Transport,
{
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
async fn unpin<T>(rpc: &NodeRpc<T>, subscription: &Value, hashes: &[BlockHash])
where
    T: Transport,
{
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
async fn header_height<T>(rpc: &NodeRpc<T>, hash: BlockHash) -> Result<u64, Error>
where
    T: Transport,
{
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
pub async fn resolve<T>(
    rpc: &NodeRpc<T>,
    heights: RangeInclusive<u64>,
) -> Result<Vec<Option<BlockHash>>, Error>
where
    T: Transport,
{
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
/// block; its state supplies the first block's authority set and the system-parameter storage the
/// first block is compared with. With `first_of_run`, the first block carries the system
/// parameters whatever its storage says; any other block carries them only if their storage differs
/// from its parent's.
///
/// All calls for all blocks go out together: one set of batches with each block's header, body and
/// state roots, one storage query per block, and one for the parent. Only system parameters, where
/// due, and the metadata of a runtime not seen before take a second round.
pub async fn source<T>(
    rpc: &NodeRpc<T>,
    metadata: &MetadataCache,
    start: u64,
    hashes: &[BlockHash],
    parent: Option<BlockHash>,
    first_of_run: bool,
) -> Result<Chunk, Error>
where
    T: Transport,
{
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

    let mut authority_set = parent_storage.as_deref().map(authority_set_of);
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
        let protocol_version = decoded_header
            .protocol_version()
            .map_err(|error| Error::ProtocolVersion(hash, error))?
            .ok_or(Error::MissingProtocolVersion(hash))?;
        let parent_hash = ByteArray(decoded_header.parent_hash.0);

        // The block's runtime is the one in its parent's state; genesis has only its own.
        let metadata_at = if height == 0 { hash } else { parent_hash };
        let metadata = metadata
            .get(rpc, u32::from(protocol_version), metadata_at)
            .await?;

        let system_parameter_hashes = system_parameter_hashes(&storage);
        let system_parameters_due = (i == 0 && first_of_run)
            || previous_system_parameters.as_ref() != Some(&system_parameter_hashes);
        previous_system_parameters = Some(system_parameter_hashes);

        let parent_authority_set = authority_set.replace(authority_set_of(&storage));

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
            parent_authority_set,
            system_parameters_due,
        });
    }

    let mut system_parameters = system_parameters(rpc, &sourced).await?;
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
        .collect();

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
async fn system_parameters<T>(
    rpc: &NodeRpc<T>,
    sourced: &[Sourced],
) -> Result<HashMap<BlockHash, (ByteVec, ByteVec)>, Error>
where
    T: Transport,
{
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

/// The genesis ledger state from the chain spec, and the cNight mappings at genesis.
async fn genesis<T>(rpc: &NodeRpc<T>, hash: BlockHash) -> Result<Genesis, Error>
where
    T: Transport,
{
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
async fn query_storage<T>(
    rpc: &NodeRpc<T>,
    hash: BlockHash,
    items: Vec<Value>,
) -> Result<Vec<StorageItem>, Error>
where
    T: Transport,
{
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

/// A block's storage query: its events and authority set, and the hashes of its system parameters.
fn block_storage_items() -> Vec<Value> {
    std::iter::once(storage_query(SYSTEM_EVENTS_ITEM, "value"))
        .chain(parent_storage_items())
        .collect()
}

/// A parent's storage query: its authority set and the hashes of its system parameters.
fn parent_storage_items() -> Vec<Value> {
    AUTHORITY_SET_ITEMS
        .into_iter()
        .map(|item| storage_query(item, "value"))
        .chain(
            SYSTEM_PARAMETERS_ITEMS
                .into_iter()
                .map(|item| storage_query(item, "hash")),
        )
        .collect()
}

fn find_item<'a>(items: &'a [StorageItem], item: (&str, &str)) -> Option<&'a StorageItem> {
    let key = storage_key(item);
    items.iter().find(|stored| *stored.key == key)
}

fn authority_set_of(items: &[StorageItem]) -> Vec<(ByteVec, ByteVec)> {
    AUTHORITY_SET_ITEMS
        .into_iter()
        .filter_map(|item| find_item(items, item))
        .filter_map(|item| {
            item.value
                .as_ref()
                .map(|value| (item.key.to_owned(), value.to_owned()))
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
    /// The metadata of the given spec version, fetched at the given block, whose state runs that
    /// version, if not cached.
    pub async fn get<T>(
        &self,
        rpc: &NodeRpc<T>,
        spec_version: u32,
        at: BlockHash,
    ) -> Result<ArcMetadata, Error>
    where
        T: Transport,
    {
        let mut cache = self.0.lock().await;
        if let Some(metadata) = cache.get(&spec_version) {
            return Ok(metadata.clone());
        }

        let metadata = Arc::new(fetch_metadata(rpc, at).await?);
        cache.insert(spec_version, metadata.clone());

        Ok(metadata)
    }
}

/// Fetch metadata as subxt does: the highest stable version `Metadata_metadata_versions` offers,
/// falling back to `Metadata_metadata`.
async fn fetch_metadata<T>(rpc: &NodeRpc<T>, at: BlockHash) -> Result<Metadata, Error>
where
    T: Transport,
{
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
        infra::subxt_node::{
            fake_node::FakeNode,
            rpc::{Call, CallResult, NodeRpc, ReconnectPolicy, method},
        },
        pipeline::source::{
            AUTHORITY_SET_ITEMS, Block, CNIGHT_MAPPINGS_ITEMS, MetadataCache, SYSTEM_EVENTS_ITEM,
            SYSTEM_PARAMETERS_ITEMS, resolve, source, storage_key,
        },
    };
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

    /// A 2.1 runtime spec version.
    const SPEC_VERSION: u32 = 2_001_000;

    static METADATA: LazyLock<Vec<u8>> = LazyLock::new(|| {
        fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.node/2.1.0-rc.4/metadata.scale"))
            .expect("metadata of node 2.1 can be read")
    });

    /// Block `n` has hash `[n; 32]`, height `n`, and the system parameters `system_parameters[n]`.
    struct Chain {
        system_parameters: Vec<u8>,
        calls: Mutex<Vec<Call>>,
    }

    fn hash(n: u8) -> BlockHash {
        ByteArray([n; 32])
    }

    fn hex(bytes: impl AsRef<[u8]>) -> Value {
        const_hex::encode_prefixed(bytes).into()
    }

    fn n_of(param: &Value) -> u8 {
        const_hex::decode(param.as_str().expect("hex param")).expect("hex")[0]
    }

    fn header(n: u8) -> Vec<u8> {
        SubstrateHeader::<H256> {
            parent_hash: H256([n.saturating_sub(1); 32]),
            number: n as u64,
            state_root: H256::zero(),
            extrinsics_root: H256::zero(),
            digest: Digest {
                logs: vec![DigestItem::Consensus(*b"MNSV", SPEC_VERSION.encode())],
            },
        }
        .encode()
    }

    fn success(bytes: impl AsRef<[u8]>) -> CallResult {
        Ok(json!({ "success": true, "value": hex(bytes) }))
    }

    impl Chain {
        fn new(system_parameters: Vec<u8>) -> Arc<Self> {
            Arc::new(Self {
                system_parameters,
                calls: Mutex::default(),
            })
        }

        fn respond(&self, call: &Call) -> CallResult {
            self.calls.lock().push(call.clone());
            match call.method {
                method::ARCHIVE_HASH_BY_HEIGHT => {
                    let n = call.params[0].as_u64().expect("height");
                    match n {
                        // No block, and two blocks, at these heights.
                        100 => Ok(json!([])),
                        101 => Ok(json!([hex([1; 32]), hex([2; 32])])),
                        n => Ok(json!([hex([n as u8; 32])])),
                    }
                }
                method::ARCHIVE_HEADER => Ok(hex(header(n_of(&call.params[0])))),
                method::ARCHIVE_BODY => Ok(json!([hex([n_of(&call.params[0])])])),
                method::ARCHIVE_CALL => match call.params[1].as_str().expect("function") {
                    "Metadata_metadata_versions" => success(vec![14u32, 15, u32::MAX].encode()),
                    "Metadata_metadata_at_version" => success(Some(METADATA.clone()).encode()),
                    function => success(function.as_bytes()),
                },
                method::CHAIN_SPEC_PROPERTIES => Ok(json!({ "genesis_state": "0xabcd" })),
                _ => Ok(Value::Null),
            }
        }

        fn storage(&self, params: &[Value]) -> Vec<Value> {
            let n = n_of(&params[0]);
            let items = params[1].as_array().expect("items");
            items
                .iter()
                .filter_map(|item| {
                    let key = const_hex::decode(item["key"].as_str().expect("key")).expect("hex");
                    let is = |item| key == storage_key(item);

                    if is(SYSTEM_EVENTS_ITEM) {
                        Some(json!({ "event": "storage", "key": item["key"], "value": hex([n]) }))
                    } else if is(AUTHORITY_SET_ITEMS[0]) {
                        let authorities = vec![[n; 32]].encode();
                        Some(json!({ "event": "storage", "key": item["key"], "value": hex(authorities) }))
                    } else if SYSTEM_PARAMETERS_ITEMS.into_iter().any(is) {
                        let value_hash = [self.system_parameters[n as usize]; 32];
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

        fn node(self: &Arc<Self>) -> FakeNode {
            let chain = self.clone();
            let storage_chain = self.clone();
            FakeNode::new(move |call| chain.respond(call)).with_subscribe(move |method, params| {
                (method == method::ARCHIVE_STORAGE).then(|| storage_chain.storage(params))
            })
        }

        /// The block hashes of every call of the given runtime function.
        fn calls_of(&self, function: &str) -> Vec<BlockHash> {
            self.calls
                .lock()
                .iter()
                .filter(|call| call.method == method::ARCHIVE_CALL && call.params[1] == function)
                .map(|call| hash(n_of(&call.params[0])))
                .collect()
        }
    }

    fn node_rpc(node: FakeNode, batch_size: usize, in_flight: usize) -> NodeRpc<FakeNode> {
        NodeRpc::new(
            node,
            NonZeroUsize::new(batch_size).unwrap(),
            NonZeroUsize::new(in_flight).unwrap(),
            ReconnectPolicy {
                max_delay: Duration::from_millis(10),
                max_attempts: 3,
            },
        )
    }

    fn hashes(heights: std::ops::RangeInclusive<u8>) -> Vec<BlockHash> {
        heights.map(hash).collect()
    }

    #[tokio::test]
    async fn test_resolve() {
        let chain = Chain::new(vec![]);
        let rpc = node_rpc(chain.node(), 64, 4);

        let resolved = resolve(&rpc, 98..=101).await.expect("hashes resolve");

        assert_eq!(resolved, vec![Some(hash(98)), Some(hash(99)), None, None]);
    }

    #[tokio::test]
    async fn test_source_from_genesis() {
        let chain = Chain::new(vec![1; 4]);
        let rpc = node_rpc(chain.node(), 64, 4);
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
            assert_eq!(*height, n as u64);
            assert_eq!(parent.hash, hash(n as u8 - 1));
            // The parent's authority set: its own Aura authorities.
            assert_eq!(parent.authority_set.len(), 1);
            assert_eq!(*parent.authority_set[0].1, vec![[n as u8 - 1; 32]].encode());
            assert_eq!(extrinsics.len(), 1);
            assert_eq!(*extrinsics[0], [n as u8]);
            assert_eq!(**events, [n as u8]);
        }

        // Metadata is fetched once for the whole run.
        assert_eq!(chain.calls_of("Metadata_metadata_at_version").len(), 1);
    }

    #[tokio::test]
    async fn test_system_parameters_change_only() {
        // The system parameters change at block 3 and again at block 6.
        let chain = Chain::new(vec![1, 1, 1, 2, 2, 2, 3]);
        let rpc = node_rpc(chain.node(), 64, 4);
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
        assert_eq!(
            chain.calls_of("SystemParametersApi_get_d_parameter"),
            vec![hash(1), hash(3), hash(6)]
        );
        assert_eq!(
            chain.calls_of("SystemParametersApi_get_terms_and_conditions"),
            vec![hash(1), hash(3), hash(6)]
        );
    }

    #[tokio::test]
    async fn test_no_serial_fetch() {
        let chain = Chain::new(vec![1; 33]);
        let node = chain.node().with_delay(Duration::from_millis(20));
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
