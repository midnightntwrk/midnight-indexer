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

//! Pipeline stages from the node to [node::Block]s.

pub mod decode;
pub mod source;

use crate::{
    domain::{BlockRef, node},
    infra::subxt_node::rpc::Transport,
    pipeline::{
        decode::CpuPool,
        source::{Finalized, Source},
    },
};
use futures::Stream;
use std::sync::Arc;
use tokio::sync::watch;

/// The blocks after `start`, or from genesis, up to and including the block at height `end`, or
/// without end, sourced and decoded on the pool in height order; and the latest finalized block.
pub fn blocks<T: Transport>(
    source: &Source<T>,
    pool: Arc<CpuPool>,
    start: Option<BlockRef>,
    end: Option<u64>,
) -> (
    impl Stream<Item = Result<node::Block, decode::Error>> + use<T>,
    watch::Receiver<Option<Finalized>>,
) {
    let (chunks, finalized) = source.run(start, end);
    let blocks = decode::decode(chunks, pool, source.config().chunk_size);

    (blocks, finalized)
}

/// Names of the pipeline metrics.
pub mod metric {
    use metrics::histogram;
    use std::time::Instant;

    pub const RESOLVE_DURATION: &str = "indexer_source_resolve_duration_seconds";
    pub const SOURCE_DURATION: &str = "indexer_source_source_duration_seconds";
    pub const VERIFY_DURATION: &str = "indexer_source_verify_duration_seconds";
    pub const EMIT_DURATION: &str = "indexer_source_emit_duration_seconds";
    pub const SOURCED_BLOCK_COUNT: &str = "indexer_source_block_count";
    pub const DECODE_CHUNK_DURATION: &str = "indexer_decode_chunk_duration_seconds";
    pub const DECODE_BLOCK_DURATION: &str = "indexer_decode_block_duration_seconds";

    /// Records the time from its start to its drop in the histogram of the given name.
    pub(crate) struct Timer(&'static str, Instant);

    impl Timer {
        pub(crate) fn start(name: &'static str) -> Self {
            Self(name, Instant::now())
        }
    }

    impl Drop for Timer {
        fn drop(&mut self) {
            histogram!(self.0).record(self.1.elapsed());
        }
    }
}
