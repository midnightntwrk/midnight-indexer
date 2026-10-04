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

//! The Emit stage: send verified blocks downstream in height order.

use crate::{
    domain::BlockRef,
    infra::subxt_node::rpc::Transport,
    pipeline::{
        metric::{self, Timer},
        sourcing::{Block, Chunk, Error, Producer},
    },
};
use indexer_common::domain::BlockHash;

/// Emission state: the last block emitted, and the verified blocks held back.
#[derive(Default)]
pub(super) struct Emission {
    pub(super) emitted: Option<BlockRef>,
    pub(super) held: Vec<Block>,
}

impl Emission {
    /// The hash the next chunk's first block must have as its parent.
    pub(super) fn last_hash(&self) -> Option<BlockHash> {
        self.held
            .last()
            .map(Block::hash)
            .or(self.emitted.map(|emitted| emitted.hash))
    }

    /// The height of the first block not yet emitted.
    pub(super) fn next_height(&self) -> u64 {
        self.emitted.map(|emitted| emitted.height + 1).unwrap_or(0)
    }
}

impl<T: Transport> Producer<T> {
    pub(super) async fn emit(
        &mut self,
        emission: &mut Emission,
        chunk: Chunk,
    ) -> Result<(), Error> {
        let Some(last) = chunk.last() else {
            return Ok(());
        };

        emission.emitted = Some(BlockRef {
            hash: last.hash(),
            height: last.height(),
        });
        // A closed channel means the stream is gone; the task is about to be aborted.
        let _timer = Timer::start(metric::EMIT_DURATION);
        let _ = self.chunks.send(Ok(chunk)).await;

        Ok(())
    }
}
