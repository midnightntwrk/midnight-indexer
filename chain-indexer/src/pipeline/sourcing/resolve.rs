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
use indexer_common::domain::{BlockHash, BlockNumber};
use std::ops::RangeInclusive;

/// The Resolve stage: the hash of the block at each height, `None` where the node reports no block
/// or several.
pub async fn resolve<T: Transport>(
    rpc: &NodeRpc<T>,
    heights: RangeInclusive<BlockNumber>,
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

#[cfg(test)]
mod tests {
    use crate::pipeline::sourcing::{
        resolve,
        tests::chain::{Chain, hash, node_rpc},
    };
    use std::sync::Arc;

    #[tokio::test]
    async fn test_resolve() {
        let (_, node) = Chain::default().node();
        let rpc = node_rpc(Arc::new(node), 64, 4);

        let resolved = resolve(&rpc, 999_998..=1_000_001)
            .await
            .expect("hashes resolve");

        assert_eq!(
            resolved,
            vec![Some(hash(999_998)), Some(hash(999_999)), None, None]
        );
    }
}
