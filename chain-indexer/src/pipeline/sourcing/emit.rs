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
        sourcing::{Chunk, Producer, Progress},
    },
};
use metrics::gauge;

impl<T: Transport> Producer<T> {
    /// Send the blocks downstream, and record the last as emitted.
    pub(super) async fn emit(&self, progress: &mut Progress, chunk: Chunk) {
        let Some(last) = chunk.last() else {
            return;
        };

        progress.emitted = Some(BlockRef {
            hash: last.hash(),
            height: last.height(),
        });
        // A closed channel means the stream is gone; the task is about to be aborted.
        let _timer = Timer::start(metric::EMIT_DURATION);
        let _ = self.chunks.send(Ok(chunk)).await;
        gauge!(metric::EMITTED_HEIGHT).set(progress.next_height().saturating_sub(1) as f64);
        gauge!(metric::BUFFERED_CHUNK_COUNT)
            .set((self.chunks.max_capacity() - self.chunks.capacity()) as f64);
    }
}
