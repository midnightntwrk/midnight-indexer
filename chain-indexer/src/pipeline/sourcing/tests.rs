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
    infra::subxt_node::rpc::{method, testing::FakeNode},
    pipeline::sourcing::{
        Block, Chunk, Error, Source,
        tests::chain::{Chain, bytes_of, config, follow, hash, height_of, start},
    },
};
use futures::{StreamExt, TryStreamExt};
use std::{sync::Arc, time::Duration};
use tokio::time::{sleep, timeout};

pub(crate) mod chain;

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

/// Run the pipeline to `end` and collect its blocks.
async fn run_to_end(source: &Source<Arc<FakeNode>>, start: Option<BlockRef>, end: u64) -> Chunk {
    let (chunks, _finalized) = source.run(start, Some(end));
    timeout(Duration::from_secs(10), chunks.try_concat())
        .await
        .expect("pipeline finishes in time")
        .expect("pipeline succeeds")
}
