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

//! The decode stage: [sourcing::Block]s into [node::Block]s, on a dedicated CPU pool.

use crate::{
    domain::node,
    infra::subxt_node::{
        AURA_ENGINE_ID, BABE_ENGINE_ID, CONSENSUS_ENGINE_RUNTIME_API, SubxtNodeError,
        author_from_digest_logs, header::SubstrateHeaderExt, runtimes,
    },
    pipeline::{
        metric::{self, Timer},
        sourcing::{self, AUTHORITY_SET_ITEMS, Parent, storage_key},
    },
};
use futures::{Stream, StreamExt, TryStreamExt, executor::block_on, stream};
use indexer_common::domain::{
    BlockAuthor, BlockHash, BlockNumber, ByteArray, ByteVec, NodeVersion, ProtocolVersion,
    ProtocolVersionError, ledger::ZswapMerkleTreeRoot,
};
use parity_scale_codec::Decode;
use rayon::{ThreadPool, ThreadPoolBuildError, ThreadPoolBuilder, prelude::*};
use std::{num::NonZeroUsize, sync::Arc};
use subxt::{
    ArcMetadata, Metadata, OfflineClient, SubstrateConfig,
    config::substrate::{DigestItem, SpecVersionForRange, SubstrateHeader},
    utils::H256,
};
use thiserror::Error;
use tokio::sync::oneshot;

#[cfg(test)]
mod tests;

/// `sp_consensus_babe::ConsensusLog::NextEpochData`'s index: a block carrying it opens a new epoch.
const BABE_NEXT_EPOCH_DATA: u8 = 1;

/// Error of the decode stage.
#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Source(#[from] sourcing::Error),
    #[error("cannot decode block {hash} at height {height}")]
    Decode {
        hash: BlockHash,
        height: BlockNumber,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("block {0} has no protocol version header")]
    MissingProtocolVersion(BlockHash),
    #[error("unsupported protocol version in block {0}")]
    ProtocolVersion(BlockHash, #[source] ProtocolVersionError),
    #[error(
        "block {hash} is authored by {engine}, but the metadata of its runtime {spec_version} has \
         no {pallet}.{entry} storage"
    )]
    MissingAuthoritySet {
        hash: BlockHash,
        engine: &'static str,
        spec_version: u32,
        pallet: &'static str,
        entry: &'static str,
    },
    #[error("the decode pool is gone")]
    PoolGone,
}

/// A dedicated pool of threads for CPU-bound pipeline work.
pub struct CpuPool(ThreadPool);

impl CpuPool {
    pub fn new(threads: NonZeroUsize) -> Result<Self, ThreadPoolBuildError> {
        ThreadPoolBuilder::new()
            .num_threads(threads.get())
            .thread_name(|n| format!("decode-{n}"))
            .build()
            .map(Self)
    }

    pub fn threads(&self) -> usize {
        self.0.current_num_threads()
    }
}

/// The number of chunks decoding at once: two, so that threads freed by a chunk's last blocks pick
/// up the next chunk, and more only if one chunk has fewer blocks than the pool has threads.
fn chunks_in_decode(threads: usize, chunk_size: usize) -> usize {
    2.max(threads.div_ceil(chunk_size) + 1)
}

/// The decode stage: each chunk is one job on the pool, its blocks decoded in parallel; blocks come
/// out in height order.
pub fn decode<S: Stream<Item = Result<sourcing::Chunk, sourcing::Error>>>(
    chunks: S,
    pool: Arc<CpuPool>,
    chunk_size: NonZeroUsize,
) -> impl Stream<Item = Result<node::Block, Error>> {
    let in_decode = chunks_in_decode(pool.threads(), chunk_size.get());

    chunks
        .map(move |chunk| {
            let pool = pool.clone();
            async move {
                let chunk = chunk?;
                let (blocks_tx, blocks_rx) = oneshot::channel();
                pool.0.spawn(move || {
                    let _timer = Timer::start(metric::DECODE_CHUNK_DURATION);
                    let blocks = chunk
                        .into_par_iter()
                        .map(|block| {
                            let _timer = Timer::start(metric::DECODE_BLOCK_DURATION);
                            node::Block::try_from(block)
                        })
                        .collect::<Result<Vec<_>, _>>();
                    let _ = blocks_tx.send(blocks);
                });
                blocks_rx.await.map_err(|_| Error::PoolGone)?
            }
        })
        .buffered(in_decode)
        .map_ok(|blocks| stream::iter(blocks).map(Ok))
        .try_flatten()
}

impl TryFrom<sourcing::Block> for node::Block {
    type Error = Error;

    fn try_from(block: sourcing::Block) -> Result<Self, Self::Error> {
        use sourcing::Block::*;

        match block {
            Genesis {
                hash,
                header,
                zswap_state_root,
                ledger_state_root,
                system_parameters,
                metadata,
                ledger_state,
                cnight_mappings,
                extrinsics,
                events,
            } => {
                let decoder = Decoder::new(hash, 0, &header, metadata)?;
                let details = decoder.details(
                    extrinsics.into_iter().map(Into::into).collect(),
                    events.into(),
                )?;
                let mut dust_registration_events = details.dust_registration_events;
                let cnight_mappings = cnight_mappings
                    .into_iter()
                    .map(|(key, value)| (key.into(), value.into()))
                    .collect::<Vec<_>>();
                dust_registration_events.extend(decoder.wrap(
                    runtimes::decode_genesis_cnight_registrations(
                        decoder.node_version,
                        &decoder.client()?,
                        &cnight_mappings,
                    ),
                )?);
                let (d_parameter, terms_and_conditions) =
                    decoder.system_parameters(Some(system_parameters))?;

                Ok(node::Block {
                    hash,
                    height: 0,
                    protocol_version: decoder.protocol_version,
                    parent_hash: ByteArray(decoder.header.parent_hash.0),
                    author: None,
                    timestamp: details.timestamp.unwrap_or(0),
                    zswap_merkle_tree_root: decoder.zswap_merkle_tree_root(&zswap_state_root)?,
                    ledger_state_root: decoder.ledger_state_root(&ledger_state_root)?,
                    transactions: details.transactions,
                    dust_registration_events,
                    bridge_events: details.bridge_events,
                    d_parameter,
                    terms_and_conditions,
                    genesis_ledger_state: Some(ledger_state),
                })
            }
            Block {
                hash,
                height,
                header,
                zswap_state_root,
                ledger_state_root,
                system_parameters,
                metadata,
                parent,
                extrinsics,
                events,
            } => {
                let decoder = Decoder::new(hash, height, &header, metadata)?;
                let details = decoder.details(
                    extrinsics.into_iter().map(Into::into).collect(),
                    events.into(),
                )?;
                let (d_parameter, terms_and_conditions) =
                    decoder.system_parameters(system_parameters)?;

                Ok(node::Block {
                    hash,
                    height: height.into(),
                    protocol_version: decoder.protocol_version,
                    parent_hash: parent.hash,
                    author: decoder.author(&parent)?,
                    timestamp: details.timestamp.unwrap_or(0),
                    zswap_merkle_tree_root: decoder.zswap_merkle_tree_root(&zswap_state_root)?,
                    ledger_state_root: decoder.ledger_state_root(&ledger_state_root)?,
                    transactions: details.transactions,
                    dust_registration_events: details.dust_registration_events,
                    bridge_events: details.bridge_events,
                    d_parameter,
                    terms_and_conditions,
                    genesis_ledger_state: None,
                })
            }
        }
    }
}

/// Decodes one block against the metadata of the runtime that executed it.
struct Decoder {
    hash: BlockHash,
    height: BlockNumber,
    header: SubstrateHeader<H256>,
    protocol_version: ProtocolVersion,
    node_version: NodeVersion,
    metadata: ArcMetadata,
}

impl Decoder {
    fn new(
        hash: BlockHash,
        height: BlockNumber,
        header: &[u8],
        metadata: ArcMetadata,
    ) -> Result<Self, Error> {
        let header =
            SubstrateHeader::<H256>::decode(&mut &*header).map_err(|error| Error::Decode {
                hash,
                height,
                source: error.into(),
            })?;
        let protocol_version = header
            .protocol_version()
            .map_err(|error| Error::ProtocolVersion(hash, error))?
            .ok_or(Error::MissingProtocolVersion(hash))?;

        Ok(Self {
            hash,
            height,
            header,
            protocol_version,
            node_version: protocol_version.node_version(),
            metadata,
        })
    }

    /// An offline client at this block, decoding against its runtime's metadata.
    fn client(&self) -> Result<subxt::client::OfflineClientAtBlock<SubstrateConfig>, Error> {
        let spec_version = u32::from(self.protocol_version);
        let config = SubstrateConfig::builder()
            .set_metadata_for_spec_versions([(spec_version, self.metadata.clone())])
            .set_spec_version_for_block_ranges([SpecVersionForRange {
                block_range: u64::from(self.height)..u64::from(self.height) + 1,
                spec_version,
                transaction_version: 0,
            }])
            .build();

        OfflineClient::new_with_config(config)
            .at_block(u64::from(self.height))
            .map_err(|error| self.error(error))
    }

    fn details(
        &self,
        extrinsics: Vec<Vec<u8>>,
        events: Vec<u8>,
    ) -> Result<runtimes::BlockDetails, Error> {
        let client = self.client()?;
        self.wrap(block_on(runtimes::decode_block_details(
            self.node_version,
            &client,
            extrinsics,
            events,
        )))
    }

    fn zswap_merkle_tree_root(&self, result: &[u8]) -> Result<ZswapMerkleTreeRoot, Error> {
        let root = self.wrap(runtimes::decode_zswap_merkle_tree_root(
            self.node_version,
            &self.client()?,
            result,
        ))?;

        ZswapMerkleTreeRoot::deserialize(root, self.protocol_version.ledger_version())
            .map_err(|error| self.error(error))
    }

    fn ledger_state_root(&self, result: &[u8]) -> Result<Option<ByteVec>, Error> {
        let root = self.wrap(runtimes::decode_ledger_state_root(
            self.node_version,
            &self.client()?,
            result,
        ))?;

        Ok(root.map(Into::into))
    }

    fn system_parameters(
        &self,
        results: Option<(ByteVec, ByteVec)>,
    ) -> Result<
        (
            Option<crate::domain::DParameter>,
            Option<crate::domain::TermsAndConditions>,
        ),
        Error,
    > {
        let Some((d_parameter, terms_and_conditions)) = results else {
            return Ok((None, None));
        };

        let client = self.client()?;
        let d_parameter = self.wrap(runtimes::decode_d_parameter(
            self.node_version,
            &client,
            &d_parameter,
        ))?;
        let terms_and_conditions = self.wrap(runtimes::decode_terms_and_conditions(
            self.node_version,
            &client,
            &terms_and_conditions,
        ))?;

        Ok((Some(d_parameter), terms_and_conditions))
    }

    /// The author, resolved from the pre-runtime digest against the parent's authority set of the
    /// digest's consensus engine.
    fn author(&self, parent: &Parent) -> Result<Option<BlockAuthor>, Error> {
        let babe_supported = self
            .metadata
            .runtime_api_trait_by_name(CONSENSUS_ENGINE_RUNTIME_API)
            .is_some();

        let authorities = block_authorities(
            &self.header,
            &parent.authority_set,
            babe_supported,
            |item| has_storage(&self.metadata, item),
        )
        .map_err(|error| {
            use AuthoritySetError::*;
            match error {
                Missing {
                    engine,
                    item: (pallet, entry),
                } => Error::MissingAuthoritySet {
                    hash: self.hash,
                    engine,
                    spec_version: u32::from(self.protocol_version),
                    pallet,
                    entry,
                },
                Decode(error) => self.error(error),
            }
        })?;
        let Some(authorities) = authorities else {
            return Ok(None);
        };

        self.wrap(author_from_digest_logs(
            &self.header.digest.logs,
            &authorities,
            self.node_version,
            babe_supported,
        ))
    }

    fn wrap<T>(&self, result: Result<T, SubxtNodeError>) -> Result<T, Error> {
        result.map_err(|error| self.error(error))
    }

    fn error(&self, error: impl std::error::Error + Send + Sync + 'static) -> Error {
        Error::Decode {
            hash: self.hash,
            height: self.height,
            source: error.into(),
        }
    }
}

type StorageItem = (&'static str, &'static str);

/// Why a block's authority set is unavailable.
#[derive(Debug)]
enum AuthoritySetError {
    /// The block's runtime has no such storage item for its consensus engine.
    Missing {
        engine: &'static str,
        item: StorageItem,
    },
    Decode(SubxtNodeError),
}

impl From<SubxtNodeError> for AuthoritySetError {
    fn from(error: SubxtNodeError) -> Self {
        Self::Decode(error)
    }
}

/// The authority set that verifies a block's author: that of the consensus engine of the block's
/// first recognized pre-runtime digest, from the parent's state. `None` if no digest is recognized.
/// Fails with the engine and storage item if `has_storage` reports that the block's runtime has no
/// such item. BABE uses the parent's next authorities when the block opens a new epoch.
fn block_authorities(
    header: &SubstrateHeader<H256>,
    parent_authority_set: &[(ByteVec, ByteVec)],
    babe_supported: bool,
    has_storage: impl Fn(StorageItem) -> bool,
) -> Result<Option<Vec<[u8; 32]>>, AuthoritySetError> {
    use DigestItem::*;
    let engine = header.digest.logs.iter().find_map(|log| match log {
        PreRuntime(AURA_ENGINE_ID, _) => Some("Aura"),
        PreRuntime(BABE_ENGINE_ID, _) if babe_supported => Some("BABE"),
        _ => None,
    });

    let (engine, item) = match engine {
        None => return Ok(None),
        Some("Aura") => ("Aura", AUTHORITY_SET_ITEMS[0]),
        Some(engine) if opens_babe_epoch(header) => (engine, AUTHORITY_SET_ITEMS[2]),
        Some(engine) => (engine, AUTHORITY_SET_ITEMS[1]),
    };

    if !has_storage(item) {
        return Err(AuthoritySetError::Missing { engine, item });
    }

    let key = storage_key(item);
    let value = parent_authority_set
        .iter()
        .find(|(stored_key, _)| **stored_key == key)
        .map(|(_, value)| value);

    let authorities = match (engine, value) {
        (_, None) => vec![],
        ("Aura", Some(value)) => runtimes::decode_authorities(value)?,
        (_, Some(value)) => decode_babe_authorities(value)?,
    };

    Ok(Some(authorities))
}

/// Decode a BABE authority set: SCALE-encoded `(public key, weight)` pairs, fixed by
/// `sp_consensus_babe`.
fn decode_babe_authorities(mut value: &[u8]) -> Result<Vec<[u8; 32]>, SubxtNodeError> {
    let authorities = Vec::<([u8; 32], u64)>::decode(&mut value)?;
    Ok(authorities.into_iter().map(|(key, _)| key).collect())
}

/// Whether the header carries BABE's next-epoch announcement, i.e. opens a new epoch.
fn opens_babe_epoch(header: &SubstrateHeader<H256>) -> bool {
    header.digest.logs.iter().any(|log| {
        matches!(
            log,
            DigestItem::Consensus(BABE_ENGINE_ID, data)
                if data.first() == Some(&BABE_NEXT_EPOCH_DATA)
        )
    })
}

fn has_storage(metadata: &Metadata, (pallet, entry): StorageItem) -> bool {
    metadata
        .pallet_by_name(pallet)
        .and_then(|pallet| pallet.storage())
        .and_then(|storage| storage.entry_by_name(entry))
        .is_some()
}
