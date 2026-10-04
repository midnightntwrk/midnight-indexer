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

use crate::pipeline::sourcing::{
    AUTHORITY_SET_ITEMS, Block, Error, MetadataCache, metadata_spec_version,
    source::{LEDGER_STATE_ROOT_FUNCTION, ZSWAP_STATE_ROOT_FUNCTION, source},
    storage_key,
    tests::chain::{Chain, SPEC_VERSION, SPEC_VERSION_1_0, hash, hashes, hex, node_rpc},
};
use indexer_common::domain::BlockNumber;
use parity_scale_codec::Encode;
use std::{sync::Arc, time::Duration};

#[tokio::test(start_paused = true)]
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
        let n = n as BlockNumber;
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

#[tokio::test(start_paused = true)]
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

    use crate::pipeline::sourcing::Block::*;
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

#[tokio::test(start_paused = true)]
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

    use crate::pipeline::sourcing::Block::*;
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

#[tokio::test(start_paused = true)]
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

#[tokio::test(start_paused = true)]
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

#[tokio::test(start_paused = true)]
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

#[tokio::test(start_paused = true)]
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

#[tokio::test(start_paused = true)]
async fn test_metadata_of_another_runtime_is_rejected() {
    // Every block is stamped 2.1, but no state runs it.
    let error = metadata_versions(Chain {
        state_2_1_from: Some(BlockNumber::MAX),
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

    use crate::pipeline::sourcing::Block::*;
    Ok(chunk
        .iter()
        .map(|block| match block {
            Block { metadata, .. } | Genesis { metadata, .. } => metadata_spec_version(metadata),
        })
        .collect())
}
