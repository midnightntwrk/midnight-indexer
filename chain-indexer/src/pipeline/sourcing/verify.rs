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

//! The Verify stage: check that a chunk links to the blocks before it, else re-source it from
//! hashes walked back from the finalized tip, and release the blocks that are confirmed.

use crate::{
    domain::BlockRef,
    infra::subxt_node::rpc::Transport,
    pipeline::{
        metric::{self, Timer},
        sourcing::{
            Block, Chunk, Error, Producer, Progress, chunk::Planned, parent_hash, source::source,
        },
    },
};
use indexer_common::domain::{BlockHash, BlockNumber};
use log::warn;
use std::ops::RangeInclusive;

impl<T: Transport> Producer<T> {
    /// Verify a sourced chunk against the blocks before it, re-sourcing it if it does not link,
    /// and return the blocks now confirmed; the rest are held back in `progress`.
    pub(super) async fn verify(
        &self,
        progress: &mut Progress,
        planned: Planned,
        chunk: Chunk,
        genesis_hash: Option<BlockHash>,
        run_start: BlockNumber,
    ) -> Result<Chunk, Error> {
        let _timer = Timer::start(metric::VERIFY_DURATION);
        let expected_parent = progress.last_hash();
        let linked = links(&chunk, &planned.spec.heights, expected_parent, genesis_hash);
        let anchored = planned
            .anchor
            .is_none_or(|anchor| chunk.last().map(Block::hash) == Some(anchor.hash));

        let chunk = if linked && anchored {
            chunk
        } else {
            // Re-source the held blocks and this chunk from hashes walked down parent links: near
            // the tip from the finalized tip, within the margin; deep from the chunk's last block,
            // which was resolved from the canonical blocks above it.
            let held_start = progress.held.first().map(Block::height);
            let start = held_start.unwrap_or(*planned.spec.heights.start());
            let end = *planned.spec.heights.end();
            let from = if planned.spec.near {
                self.finalized
                    .borrow()
                    .as_ref()
                    .map(|finalized| finalized.tip)
                    .ok_or(Error::FinalizedEnded)?
            } else {
                let last = chunk.last().ok_or(Error::Unlinked(start))?;
                BlockRef {
                    hash: last.hash(),
                    height: last.height(),
                }
            };
            warn!(
                start,
                end,
                from = from.height;
                "block hashes do not link, walking parent hashes down"
            );

            progress.held.clear();
            let chunk = self.walk_and_source(from, start, end, run_start).await?;
            let expected_parent = progress.emitted.map(|emitted| emitted.hash);
            if !links(&chunk, &(start..=end), expected_parent, genesis_hash) {
                return Err(Error::Unlinked(start));
            }
            chunk
        };

        progress.held.extend(chunk);

        let confirmed = if planned.spec.near {
            // Near blocks wait until they link to the finalized tip.
            if planned.anchor.is_some() {
                std::mem::take(&mut progress.held)
            } else {
                vec![]
            }
        } else {
            // Deep blocks wait for their child to confirm them: all but the last.
            let last = progress.held.pop();
            let confirmed = std::mem::take(&mut progress.held);
            progress.held.extend(last);
            confirmed
        };

        Ok(confirmed)
    }

    /// Source the blocks at heights `start..=end`, with hashes from walking parent hashes down from
    /// the block `from`, at or above `end`.
    async fn walk_and_source(
        &self,
        from: BlockRef<BlockNumber>,
        start: BlockNumber,
        end: BlockNumber,
        run_start: BlockNumber,
    ) -> Result<Chunk, Error> {
        let mut hashes = vec![];
        let mut hash = from.hash;
        let mut height = from.height;
        loop {
            if height <= end {
                hashes.push(hash);
            }
            if height == start || height == 0 {
                break;
            }

            hash = parent_hash(&self.rpc, hash).await?;
            height -= 1;
        }
        hashes.reverse();

        let parent = if start == 0 {
            None
        } else {
            Some(parent_hash(&self.rpc, hashes[0]).await?)
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
}

/// Whether the chunk holds exactly the given heights, the first block has the expected parent (or
/// is the genesis block with the genesis hash), and every other block's parent is its predecessor.
fn links(
    chunk: &[Block],
    heights: &RangeInclusive<BlockNumber>,
    expected_parent: Option<BlockHash>,
    genesis_hash: Option<BlockHash>,
) -> bool {
    let expected_heights = chunk.len() == (heights.end() - heights.start() + 1) as usize
        && chunk
            .iter()
            .zip(heights.clone())
            .all(|(block, height)| block.height() == height);

    let mut previous = expected_parent;
    let linked = chunk.iter().all(|block| {
        use super::Block::*;
        let linked = match block {
            Genesis { hash, .. } => Some(*hash) == genesis_hash,
            Block { parent, .. } => Some(parent.hash) == previous,
        };
        previous = Some(block.hash());
        linked
    });

    expected_heights && linked
}
