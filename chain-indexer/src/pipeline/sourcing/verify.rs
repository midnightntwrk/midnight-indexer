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
    infra::subxt_node::rpc::{Batch, Transport},
    pipeline::{
        metric::{self, Timer},
        sourcing::{
            Block, Chunk, Error, Producer, Progress, chunk::Planned, decode_header, header_bytes,
            source::source,
        },
    },
};
use indexer_common::domain::{BlockHash, BlockNumber, ByteArray};
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
            // Re-source the held blocks and this chunk from hashes walked back from the tip.
            let held_start = progress.held.first().map(Block::height);
            let start = held_start.unwrap_or(*planned.spec.heights.start());
            let end = *planned.spec.heights.end();
            warn!(start, end; "block hashes do not link, walking parent hashes from the tip");

            progress.held.clear();
            let chunk = self.walk_and_source(start, end, run_start).await?;
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

    /// Source the blocks at heights `start..=end`, with hashes from walking parent hashes back from
    /// the finalized tip.
    async fn walk_and_source(
        &self,
        start: BlockNumber,
        end: BlockNumber,
        run_start: BlockNumber,
    ) -> Result<Chunk, Error> {
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

            let batch = Batch::default().header(hash);
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
            let batch = Batch::default().header(hashes[0]);
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
