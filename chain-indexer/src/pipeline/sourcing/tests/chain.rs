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

//! A scripted chain answering a [FakeNode]'s calls, and helpers around it.

use crate::{
    domain::BlockRef,
    infra::subxt_node::rpc::{
        Call, CallError, CallResult, NodeRpc, ReconnectPolicy, method, testing::FakeNode,
    },
    pipeline::sourcing::{
        AUTHORITY_SET_ITEMS, CNIGHT_MAPPINGS_ITEMS, Config, SYSTEM_EVENTS_ITEM,
        SYSTEM_PARAMETERS_ITEMS, storage_key,
    },
};
use indexer_common::domain::{BlockHash, BlockNumber, ByteArray};
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
    config::substrate::{Digest, DigestItem, SubstrateHeader},
    utils::H256,
};

/// Spec versions of the 1.0.300 and 2.1 runtimes.
pub(crate) const SPEC_VERSION_1_0: u32 = 1_000_300;
pub(crate) const SPEC_VERSION: u32 = 2_001_000;
/// Marks a fork sibling's hash.
const FORK: u8 = 0xff;
static METADATA: LazyLock<Vec<u8>> = LazyLock::new(|| node_metadata("2.1.0-rc.4"));
static METADATA_1_0: LazyLock<Vec<u8>> = LazyLock::new(|| node_metadata("1.0.300"));

pub(crate) fn bytes_of(param: &Value) -> Vec<u8> {
    const_hex::decode(param.as_str().expect("hex param")).expect("hex")
}

/// A chain answering archive calls for any height, with system parameters `system_parameters`
/// by height (1 beyond its end), fork siblings resolved at the `forks` heights, and failing
/// headers at the `failing` heights. Blocks run the 2.1 runtime, except that headers below
/// `stamped_2_1_from` are stamped 1.0.300 and states below `state_2_1_from` run 1.0.300.
#[derive(Default)]
pub(crate) struct Chain {
    pub(crate) system_parameters: Vec<u8>,
    /// The authority set at each height, as a set number; past the end, each block's own.
    pub(crate) authority_sets: Vec<u8>,
    pub(crate) forks: Vec<BlockNumber>,
    /// Heights at which the node reports the canonical block and a fork sibling.
    pub(crate) siblings: Vec<BlockNumber>,
    pub(crate) failing: Vec<BlockNumber>,
    pub(crate) stamped_2_1_from: Option<BlockNumber>,
    pub(crate) state_2_1_from: Option<BlockNumber>,
    pub(crate) calls: Mutex<Vec<Call>>,
    /// The heights at which authority-set values were queried.
    pub(crate) authority_set_reads: Mutex<Vec<BlockNumber>>,
}

impl Chain {
    /// The authority set at the given block: one authority, the set number repeated or the
    /// block hash.
    pub(crate) fn authority_set(&self, n: BlockNumber, block: &[u8]) -> [u8; 32] {
        match self.authority_sets.get(n as usize) {
            Some(&set) => [set; 32],
            None => block.try_into().unwrap(),
        }
    }

    pub(crate) fn stamped_spec_version(&self, n: BlockNumber) -> u32 {
        match self.stamped_2_1_from {
            Some(from) if n < from => SPEC_VERSION_1_0,
            _ => SPEC_VERSION,
        }
    }

    pub(crate) fn state_metadata(&self, n: BlockNumber) -> &'static [u8] {
        match self.state_2_1_from {
            Some(from) if n < from => &METADATA_1_0,
            _ => &METADATA,
        }
    }

    pub(crate) fn respond(&self, call: &Call) -> CallResult {
        self.calls.lock().push(call.clone());
        match call.method {
            method::ARCHIVE_HASH_BY_HEIGHT => {
                let n = height(&call.params[0]);
                match n {
                    // No block, and two blocks, at these heights.
                    1_000_000 => Ok(json!([])),
                    1_000_001 => Ok(json!([hex(hash(n)), hex(fork(n))])),
                    n if self.forks.contains(&n) => Ok(json!([hex(fork(n))])),
                    n if self.siblings.contains(&n) => Ok(json!([hex(hash(n)), hex(fork(n))])),
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

    pub(crate) fn storage(&self, params: &[Value]) -> Vec<Value> {
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

    pub(crate) fn node(self) -> (Arc<Self>, FakeNode) {
        let chain = Arc::new(self);
        let node = FakeNode::new({
            let chain = chain.clone();
            move |call| chain.respond(call)
        })
        .with_subscribe({
            let chain = chain.clone();
            move |method, params| (method == method::ARCHIVE_STORAGE).then(|| chain.storage(params))
        });

        (chain, node)
    }

    /// The block hashes of every call of the given runtime function.
    pub(crate) fn calls_of(&self, function: &str) -> Vec<BlockHash> {
        self.calls
            .lock()
            .iter()
            .filter(|call| call.method == method::ARCHIVE_CALL && call.params[1] == function)
            .map(|call| ByteArray(bytes_of(&call.params[0]).try_into().unwrap()))
            .collect()
    }

    /// The number of `archive_v1_header` calls.
    pub(crate) fn header_calls(&self) -> usize {
        self.calls
            .lock()
            .iter()
            .filter(|call| call.method == method::ARCHIVE_HEADER)
            .count()
    }
}

pub(crate) fn config(chunk_size: usize, chunks_ahead: usize) -> Config {
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

/// `chainHead_v1_follow` events: initialized at `tip`, then finalized one block at a time up
/// to `last`.
pub(crate) fn follow(tip: BlockNumber, last: BlockNumber) -> Vec<Value> {
    std::iter::once(json!({ "event": "initialized", "finalizedBlockHashes": [hex(hash(tip))] }))
        .chain((tip + 1..=last).map(|n| {
            json!({ "event": "finalized", "finalizedBlockHashes": [hex(hash(n))], "prunedBlockHashes": [] })
        }))
        .collect()
}

fn fork(n: BlockNumber) -> BlockHash {
    let mut hash = hash(n);
    hash.0[31] = FORK;
    hash
}

/// The canonical block at height `n` has hash `hash(n)`; a fork sibling at height `n` has
/// `fork(n)`, with the same parent.
pub(crate) fn hash(n: BlockNumber) -> BlockHash {
    let mut hash = [0; 32];
    hash[..4].copy_from_slice(&n.to_le_bytes());
    ByteArray(hash)
}

pub(crate) fn hashes(heights: std::ops::RangeInclusive<BlockNumber>) -> Vec<BlockHash> {
    heights.map(hash).collect()
}

fn header(n: BlockNumber, spec_version: u32) -> Vec<u8> {
    SubstrateHeader::<H256> {
        parent_hash: H256(hash(n.saturating_sub(1)).0),
        number: n.into(),
        state_root: H256::zero(),
        extrinsics_root: H256::zero(),
        digest: Digest {
            logs: vec![DigestItem::Consensus(*b"MNSV", spec_version.encode())],
        },
    }
    .encode()
}

pub(crate) fn height_of(hash: &[u8]) -> BlockNumber {
    BlockNumber::from_le_bytes(hash[..4].try_into().expect("4 bytes"))
}

pub(crate) fn hex(bytes: impl AsRef<[u8]>) -> Value {
    const_hex::encode_prefixed(bytes).into()
}

fn node_metadata(node_version: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../.node")
        .join(node_version)
        .join("metadata.scale");
    fs::read(path).expect("node metadata can be read")
}

pub(crate) fn node_rpc(
    node: Arc<FakeNode>,
    batch_size: usize,
    in_flight: usize,
) -> NodeRpc<Arc<FakeNode>> {
    NodeRpc::new(
        node,
        size(batch_size),
        size(in_flight),
        config(1, 1).reconnect_policy,
    )
}

fn size(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n).unwrap()
}

pub(crate) fn start(height: BlockNumber) -> Option<BlockRef> {
    Some(BlockRef {
        hash: hash(height),
        height: height.into(),
    })
}

fn success(bytes: impl AsRef<[u8]>) -> CallResult {
    Ok(json!({ "success": true, "value": hex(bytes) }))
}

/// The height parameter of an `archive_v1_hashByHeight` call.
fn height(param: &Value) -> BlockNumber {
    let height = param.as_u64().expect("height");
    BlockNumber::try_from(height).expect("height is a block number")
}
