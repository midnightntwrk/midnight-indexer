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
        sourcing::{Error, block_hash_of, parent_hash},
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
    let hashes = resolve_all(rpc, heights).await?;

    Ok(hashes
        .into_iter()
        .map(|hashes| match hashes.as_slice() {
            [hash] => Some(*hash),
            _ => None,
        })
        .collect())
}

/// The canonical hash at each height. A height with several blocks, such as fork siblings a node
/// keeps, takes the parent of the canonical block above it: `above` past the last height if given,
/// else resolved further up. A height without a block fails.
pub(super) async fn canonical<T: Transport>(
    rpc: &NodeRpc<T>,
    heights: RangeInclusive<BlockNumber>,
    above: Option<BlockHash>,
) -> Result<Vec<BlockHash>, Error> {
    let start = *heights.start();
    let mut hashes = resolve_all(rpc, heights).await?;

    let mut child = None;
    for (index, candidates) in hashes.iter_mut().enumerate().rev() {
        let height = start + index as BlockNumber;
        let hash = match candidates.as_slice() {
            [] => return Err(Error::Unresolved(height)),
            [hash] => *hash,
            _ => {
                let child = match (child, above) {
                    (Some(child), _) | (None, Some(child)) => child,
                    (None, None) => canonical_at(rpc, height + 1).await?,
                };
                parent_hash(rpc, child).await?
            }
        };
        *candidates = vec![hash];
        child = Some(hash);
    }

    Ok(hashes.into_iter().flatten().collect())
}

/// The canonical hash at `height`: its only block, else the parent of the canonical block above,
/// resolving up to the first height with a single block.
async fn canonical_at<T: Transport>(
    rpc: &NodeRpc<T>,
    height: BlockNumber,
) -> Result<BlockHash, Error> {
    let mut above = height;
    let mut hash = loop {
        let candidates = resolve_all(rpc, above..=above)
            .await?
            .pop()
            .expect("one result per height");
        match candidates.as_slice() {
            [] => return Err(Error::Unresolved(above)),
            [hash] => break *hash,
            _ => above += 1,
        }
    };
    for _ in height..above {
        hash = parent_hash(rpc, hash).await?;
    }

    Ok(hash)
}

/// Every block hash the node reports at each height.
async fn resolve_all<T: Transport>(
    rpc: &NodeRpc<T>,
    heights: RangeInclusive<BlockNumber>,
) -> Result<Vec<Vec<BlockHash>>, Error> {
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

            hashes.into_iter().map(block_hash_of).collect()
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

    #[tokio::test(start_paused = true)]
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
