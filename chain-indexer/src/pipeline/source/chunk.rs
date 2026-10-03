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

//! The Chunk stage: cut the heights up to the finalized tip into chunks.

use crate::pipeline::source::Finalized;
use indexer_common::domain::BlockHash;
use std::{num::NonZeroUsize, ops::RangeInclusive};

/// Distance below the finalized tip within which a chunk is *near*: it is only emitted once it links
/// to the finalized hash. Chunks further down are *deep*: anchored at their start and confirmed
/// block by block. Two GRANDPA sessions' worth of blocks (#1038).
pub const FINALIZATION_SAFETY_MARGIN: u64 = 400;

/// A run of heights to source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkSpec {
    pub heights: RangeInclusive<u64>,

    /// Whether the chunk lies within [FINALIZATION_SAFETY_MARGIN] of the finalized tip.
    pub near: bool,

    /// The hashes at these heights, if the [Finalized] window covers all of them.
    pub hashes: Option<Vec<BlockHash>>,
}

/// The chunk starting at height `next`: up to `max_size` heights, cut at the finalized tip and at
/// the boundary between deep and near heights; `None` if `next` is above the tip.
pub fn next_chunk(next: u64, finalized: &Finalized, max_size: NonZeroUsize) -> Option<ChunkSpec> {
    let tip = finalized.tip.height;
    if next > tip {
        return None;
    }

    let boundary = tip.saturating_sub(FINALIZATION_SAFETY_MARGIN);
    let near = next >= boundary;
    let end = (next + max_size.get() as u64 - 1).min(tip);
    let end = if near { end } else { end.min(boundary - 1) };

    let window_start = tip + 1 - finalized.hashes.len() as u64;
    let hashes = (next >= window_start).then(|| {
        let start = (next - window_start) as usize;
        let end = (end - window_start) as usize;
        finalized.hashes[start..=end].to_vec()
    });

    Some(ChunkSpec {
        heights: next..=end,
        near,
        hashes,
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        domain::BlockRef,
        pipeline::source::{
            Finalized,
            chunk::{ChunkSpec, FINALIZATION_SAFETY_MARGIN, next_chunk},
        },
    };
    use indexer_common::domain::ByteArray;
    use std::num::NonZeroUsize;

    fn finalized(tip: u64, window: u8) -> Finalized {
        Finalized {
            hashes: (0..window).map(|n| ByteArray([n; 32])).collect(),
            tip: BlockRef {
                hash: ByteArray([window.saturating_sub(1); 32]),
                height: tip,
            },
        }
    }

    fn size(n: usize) -> NonZeroUsize {
        NonZeroUsize::new(n).unwrap()
    }

    #[test]
    fn test_above_tip() {
        assert_eq!(next_chunk(101, &finalized(100, 1), size(10)), None);
    }

    #[test]
    fn test_up_to_size_and_tip() {
        let tip = 10_000;
        let deep = next_chunk(0, &finalized(tip, 1), size(512)).unwrap();
        assert_eq!(deep.heights, 0..=511);
        assert!(!deep.near);
        assert_eq!(deep.hashes, None);

        let last = next_chunk(tip - 10, &finalized(tip, 1), size(512)).unwrap();
        assert_eq!(last.heights, tip - 10..=tip);
        assert!(last.near);
    }

    #[test]
    fn test_cut_at_margin() {
        let tip = 10_000;
        let boundary = tip - FINALIZATION_SAFETY_MARGIN;

        let deep = next_chunk(boundary - 100, &finalized(tip, 1), size(512)).unwrap();
        assert_eq!(deep.heights, boundary - 100..=boundary - 1);
        assert!(!deep.near);

        let near = next_chunk(boundary, &finalized(tip, 1), size(512)).unwrap();
        assert_eq!(near.heights, boundary..=tip);
        assert!(near.near);
    }

    #[test]
    fn test_young_chain_is_near() {
        let chunk = next_chunk(0, &finalized(100, 1), size(512)).unwrap();
        assert_eq!(chunk.heights, 0..=100);
        assert!(chunk.near);
    }

    #[test]
    fn test_hashes_from_window() {
        // Window of 3 hashes at heights 98, 99, 100.
        let finalized = finalized(100, 3);

        assert_eq!(
            next_chunk(99, &finalized, size(512)),
            Some(ChunkSpec {
                heights: 99..=100,
                near: true,
                hashes: Some(finalized.hashes[1..=2].to_vec()),
            })
        );
        assert_eq!(next_chunk(97, &finalized, size(512)).unwrap().hashes, None);
    }
}
