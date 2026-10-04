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

use crate::{
    domain::BlockRef,
    infra::subxt_node::rpc::{
        Call, CallError, CallResult, NodeRpc, ReconnectPolicy, method, testing::FakeNode,
    },
    pipeline::sourcing::{
        AUTHORITY_SET_ITEMS, Block, CNIGHT_MAPPINGS_ITEMS, Chunk, Config, Error, MetadataCache,
        SYSTEM_EVENTS_ITEM, SYSTEM_PARAMETERS_ITEMS, Source, metadata_spec_version, resolve,
        source::{LEDGER_STATE_ROOT_FUNCTION, ZSWAP_STATE_ROOT_FUNCTION, source},
        storage_key,
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

#[tokio::test]
async fn test_resolve() {
    let (_, node) = Chain::default().node();
    let rpc = node_rpc(Arc::new(node), 64, 4);

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
    let rpc = node_rpc(Arc::new(node), 64, 4);
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
    let rpc = node_rpc(Arc::new(node), 64, 4);
    let metadata = MetadataCache::default();

    let first = source(&rpc, &metadata, 1, &hashes(1..=3), Some(hash(0)), true)
        .await
        .expect("first chunk is sourced");
    let second = source(&rpc, &metadata, 4, &hashes(4..=6), Some(hash(3)), false)
        .await
        .expect("second chunk is sourced");

    use super::Block::*;
    let carried = first
        .iter()
        .chain(&second)
        .map(|block| match block {
            Block {
                system_parameters, ..
            } => system_parameters.is_some(),
            Genesis { .. } => true,
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
    let rpc = node_rpc(Arc::new(node), 64, 4);
    let metadata = MetadataCache::default();

    let first = source(&rpc, &metadata, 1, &hashes(1..=3), Some(hash(0)), true)
        .await
        .expect("first chunk is sourced");
    let second = source(&rpc, &metadata, 4, &hashes(4..=6), Some(hash(3)), false)
        .await
        .expect("second chunk is sourced");

    use super::Block::*;
    let parent_sets = first
        .iter()
        .chain(&second)
        .map(|block| match block {
            Block { parent, .. } => parent.authority_set.clone(),
            Genesis { .. } => panic!("no genesis"),
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
    let node = Arc::new(node.with_delay(Duration::from_millis(20)));
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
    let rpc = node_rpc(Arc::new(node), 64, 4);
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
    let node = Arc::new(
        node.with_notification_interval(Duration::from_millis(5))
            .with_subscriptions(vec![follow(10, 20)]),
    );
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
    let source = Source::new(Arc::new(node), config(10, 4));

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
    let source = Source::new(Arc::new(node), config(10, 2));

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
    let source = Source::new(Arc::new(node), config(20, 2));

    let blocks = run_to_end(&source, start(949), 1_000).await;

    assert_canonical(&blocks, 950..=1_000);
}

#[tokio::test]
async fn test_chunk_overlap() {
    let (_, node) = Chain::default().node();
    let node = Arc::new(
        node.with_subscriptions(vec![follow(1_000, 1_000)])
            .with_delay(Duration::from_millis(10)),
    );
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
    let source = Source::new(Arc::new(node), config(2, 1));
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
    let node = Arc::new(node.with_subscriptions(vec![follow(1_000, 1_000)]));
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
    let node_versions =
        fs::read_to_string(node_dir.join("../NODE_VERSIONS")).expect("NODE_VERSIONS can be read");

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

fn bytes_of(param: &Value) -> Vec<u8> {
    const_hex::decode(param.as_str().expect("hex param")).expect("hex")
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
            move |method, params| (method == method::ARCHIVE_STORAGE).then(|| chain.storage(params))
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

/// `chainHead_v1_follow` events: initialized at `tip`, then finalized one block at a time up
/// to `last`.
fn follow(tip: u64, last: u64) -> Vec<Value> {
    std::iter::once(json!({ "event": "initialized", "finalizedBlockHashes": [hex(hash(tip))] }))
        .chain((tip + 1..=last).map(|n| {
            json!({ "event": "finalized", "finalizedBlockHashes": [hex(hash(n))], "prunedBlockHashes": [] })
        }))
        .collect()
}

fn fork(n: u64) -> BlockHash {
    let mut hash = hash(n);
    hash.0[31] = FORK;
    hash
}

/// The canonical block at height `n` has hash `hash(n)`; a fork sibling at height `n` has
/// `fork(n)`, with the same parent.
fn hash(n: u64) -> BlockHash {
    let mut hash = [0; 32];
    hash[..8].copy_from_slice(&n.to_le_bytes());
    ByteArray(hash)
}

fn hashes(heights: std::ops::RangeInclusive<u64>) -> Vec<BlockHash> {
    heights.map(hash).collect()
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

fn height_of(hash: &[u8]) -> u64 {
    u64::from_le_bytes(hash[..8].try_into().expect("8 bytes"))
}

fn hex(bytes: impl AsRef<[u8]>) -> Value {
    const_hex::encode_prefixed(bytes).into()
}

/// The metadata spec versions of the blocks of a chunk sourced at heights 5 to 7.
async fn metadata_versions(chain: Chain) -> Result<Vec<Option<u32>>, Error> {
    let (_, node) = chain.node();
    let rpc = node_rpc(Arc::new(node), 64, 4);
    let chunk = source(
        &rpc,
        &MetadataCache::default(),
        5,
        &hashes(5..=7),
        Some(hash(4)),
        true,
    )
    .await?;

    use super::Block::*;
    Ok(chunk
        .iter()
        .map(|block| match block {
            Block { metadata, .. } | Genesis { metadata, .. } => metadata_spec_version(metadata),
        })
        .collect())
}

fn node_metadata(node_version: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../.node")
        .join(node_version)
        .join("metadata.scale");
    fs::read(path).expect("node metadata can be read")
}

fn node_rpc(node: Arc<FakeNode>, batch_size: usize, in_flight: usize) -> NodeRpc<Arc<FakeNode>> {
    NodeRpc::new(
        node,
        size(batch_size),
        size(in_flight),
        config(1, 1).reconnect_policy,
    )
}

/// Run the pipeline to `end` and collect its blocks.
async fn run_to_end(source: &Source<Arc<FakeNode>>, start: Option<BlockRef>, end: u64) -> Chunk {
    let (chunks, _finalized) = source.run(start, Some(end));
    timeout(Duration::from_secs(10), chunks.try_concat())
        .await
        .expect("pipeline finishes in time")
        .expect("pipeline succeeds")
}

fn size(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n).unwrap()
}

fn start(height: u64) -> Option<BlockRef> {
    Some(BlockRef {
        hash: hash(height),
        height,
    })
}

fn success(bytes: impl AsRef<[u8]>) -> CallResult {
    Ok(json!({ "success": true, "value": hex(bytes) }))
}
