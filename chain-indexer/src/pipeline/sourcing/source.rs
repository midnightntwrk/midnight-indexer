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

//! The Source stage: the raw data of the blocks of a chunk.

use crate::{
    infra::subxt_node::{
        header::SubstrateHeaderExt,
        rpc::{self, Batch, CallResult, NodeRpc, Transport, method},
    },
    pipeline::{
        metric::{self, Timer},
        sourcing::{
            self, CNIGHT_MAPPINGS_ITEMS, Chunk, Error, Parent, Producer, block_number, call_value,
            chunk::Planned,
            decode_header, header_bytes,
            metadata::MetadataCache,
            resolve::resolve,
            storage::{
                authority_set_hashes, authority_set_items, authority_set_of, block_storage_items,
                events_of, parent_storage_items, query_storage, storage_query,
                system_parameter_hashes,
            },
        },
    },
};
use futures::{StreamExt, TryStreamExt, future::try_join, stream};
use indexer_common::domain::{BlockHash, BlockNumber, ByteArray, ByteVec};
use metrics::counter;
use serde_json::Value;
use std::{collections::HashMap, future::Future};
use subxt::ArcMetadata;

#[cfg(test)]
mod tests;

pub(super) const ZSWAP_STATE_ROOT_FUNCTION: &str = "MidnightRuntimeApi_get_zswap_state_root";
pub(super) const LEDGER_STATE_ROOT_FUNCTION: &str = "MidnightRuntimeApi_get_ledger_state_root";
pub(super) const D_PARAMETER_FUNCTION: &str = "SystemParametersApi_get_d_parameter";
pub(super) const TERMS_AND_CONDITIONS_FUNCTION: &str =
    "SystemParametersApi_get_terms_and_conditions";

impl<T: Transport> Producer<T> {
    /// Resolve and source a planned chunk.
    pub(super) fn source_planned(
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
pub(crate) async fn source<T: Transport>(
    rpc: &NodeRpc<T>,
    metadata: &MetadataCache,
    start: BlockNumber,
    hashes: &[BlockHash],
    parent: Option<BlockHash>,
    first_of_run: bool,
) -> Result<Chunk, Error> {
    let _timer = Timer::start(metric::SOURCE_DURATION);
    let batch = hashes.iter().fold(Batch::default(), |batch, &hash| {
        batch
            .header(hash)
            .body(hash)
            .call(hash, ZSWAP_STATE_ROOT_FUNCTION, &[])
            .call(hash, LEDGER_STATE_ROOT_FUNCTION, &[])
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

        let height = start + i as BlockNumber;
        let decoded_header = decode_header(&header, hash)?;
        let header_height = block_number(decoded_header.number)?;
        if header_height != height {
            return Err(Error::HeightMismatch {
                hash,
                height,
                header_height,
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

        sourced.push(Block {
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

/// A block as sourced, before it becomes a [sourcing::Block].
struct Block {
    hash: BlockHash,
    height: BlockNumber,
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

impl Block {
    fn into_block(
        self,
        system_parameters: Option<(ByteVec, ByteVec)>,
        genesis: Option<Genesis>,
    ) -> sourcing::Block {
        match (genesis, system_parameters) {
            (Some(genesis), Some(system_parameters)) => sourcing::Block::Genesis {
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
            (_, system_parameters) => sourcing::Block::Block {
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
    sourced: &[Block],
) -> Result<HashMap<BlockHash, (ByteVec, ByteVec)>, Error> {
    let due = sourced
        .iter()
        .filter(|block| block.system_parameters_due)
        .map(|block| block.hash)
        .collect::<Vec<_>>();
    if due.is_empty() {
        return Ok(HashMap::new());
    }

    let batch = due.iter().fold(Batch::default(), |batch, &hash| {
        batch
            .call(hash, D_PARAMETER_FUNCTION, &[])
            .call(hash, TERMS_AND_CONDITIONS_FUNCTION, &[])
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
    sourced: &[Block],
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
        let batch = Batch::default().chain_spec_properties();
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
