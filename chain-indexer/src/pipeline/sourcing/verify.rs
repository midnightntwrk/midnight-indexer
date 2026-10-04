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
//! hashes walked back from the finalized tip.

use crate::{
    infra::subxt_node::rpc::{Batch, Transport},
    pipeline::{
        metric::{self, Timer},
        sourcing::{
            Block, Chunk, Error, Producer, chunk::Planned, decode_header, emit::Emission,
            header_bytes, source::source,
        },
    },
};
use indexer_common::domain::{BlockHash, ByteArray};
use log::warn;
use std::ops::RangeInclusive;

impl<T: Transport> Producer<T> {
    pub(super) async fn verify_and_emit(
        &mut self,
        emission: &mut Emission,
        planned: Planned,
        chunk: Chunk,
        genesis_hash: Option<BlockHash>,
        run_start: u64,
    ) -> Result<(), Error> {
        let timer = Timer::start(metric::VERIFY_DURATION);
        let expected_parent = emission.last_hash();
        let linked = links(&chunk, &planned.spec.heights, expected_parent, genesis_hash);
        let anchored = planned
            .anchor
            .is_none_or(|anchor| chunk.last().map(Block::hash) == Some(anchor.hash));

        let chunk = if linked && anchored {
            chunk
        } else {
            // Re-source the held blocks and this chunk from hashes walked back from the tip.
            let held_start = emission.held.first().map(Block::height);
            let start = held_start.unwrap_or(*planned.spec.heights.start());
            let end = *planned.spec.heights.end();
            warn!(start, end; "block hashes do not link, walking parent hashes from the tip");

            emission.held.clear();
            let chunk = self.walk_and_source(start, end, run_start).await?;
            let expected_parent = emission.emitted.map(|emitted| emitted.hash);
            if !links(&chunk, &(start..=end), expected_parent, genesis_hash) {
                return Err(Error::Unlinked(start));
            }
            chunk
        };

        emission.held.extend(chunk);

        let emit = if planned.spec.near {
            // Near blocks wait until they link to the finalized tip.
            if planned.anchor.is_some() {
                std::mem::take(&mut emission.held)
            } else {
                vec![]
            }
        } else {
            // Deep blocks wait for their child to confirm them: all but the last.
            let last = emission.held.pop();
            let emit = std::mem::take(&mut emission.held);
            emission.held.extend(last);
            emit
        };
        drop(timer);

        self.emit(emission, emit).await
    }

    /// Source the blocks at heights `start..=end`, with hashes from walking parent hashes back from
    /// the finalized tip.
    async fn walk_and_source(&self, start: u64, end: u64, run_start: u64) -> Result<Chunk, Error> {
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
    heights: &RangeInclusive<u64>,
    expected_parent: Option<BlockHash>,
    genesis_hash: Option<BlockHash>,
) -> bool {
    let expected_heights = chunk.len() as u64 == heights.end() - heights.start() + 1
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
