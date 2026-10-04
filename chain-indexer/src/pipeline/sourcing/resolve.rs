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

//! The Resolve stage: the hash of the block at each height.

use crate::{
    infra::subxt_node::rpc::{self, Batch, NodeRpc, Transport, method},
    pipeline::{
        metric::{self, Timer},
        sourcing::{Error, block_hash_of},
    },
};
use indexer_common::domain::BlockHash;
use std::ops::RangeInclusive;

/// The Resolve stage: the hash of the block at each height, `None` where the node reports no block
/// or several.
pub async fn resolve<T: Transport>(
    rpc: &NodeRpc<T>,
    heights: RangeInclusive<u64>,
) -> Result<Vec<Option<BlockHash>>, Error> {
    let _timer = Timer::start(metric::RESOLVE_DURATION);
    let batch = heights.fold(Batch::default(), Batch::hash_by_height);

    rpc.batch(batch)
        .await?
        .into_iter()
        .map(|hashes| {
            let hashes = hashes.map_err(|source| rpc::Error::Call {
                method: method::ARCHIVE_HASH_BY_HEIGHT,
                source,
            })?;
            let hashes = serde_json::from_value::<Vec<String>>(hashes).map_err(|source| {
                rpc::Error::Decode {
                    method: method::ARCHIVE_HASH_BY_HEIGHT,
                    source,
                }
            })?;

            match <[String; 1]>::try_from(hashes) {
                Ok([hash]) => block_hash_of(hash).map(Some),
                Err(_) => Ok(None),
            }
        })
        .collect()
}
