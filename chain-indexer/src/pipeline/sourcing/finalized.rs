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

//! The Finalized stage: follow the node's finalized blocks.

use crate::{
    domain::BlockRef,
    infra::subxt_node::rpc::{Batch, NodeRpc, Subscription, Transport, method},
    pipeline::{
        metric,
        sourcing::{Error, Finalized, block_hash_of, decode_header, header_bytes},
    },
};
use futures::StreamExt;
use indexer_common::domain::BlockHash;
use log::{debug, warn};
use metrics::{counter, gauge};
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;
use tokio::{sync::watch, time::timeout};

/// The Finalized stage: follow the node's finalized blocks with `chainHead_v1_follow` and publish
/// each [Finalized] to `finalized`. Every block the subscription reports is unpinned as soon as it
/// is reported; nothing else is fetched except one header per subscription, for the tip's height.
/// The subscription is renewed on `stop`, when it ends, and when no event arrives within
/// `recovery_timeout`. Returns once `finalized` has no receivers.
pub(super) async fn follow_finalized<T: Transport>(
    rpc: &NodeRpc<T>,
    recovery_timeout: Duration,
    finalized: &watch::Sender<Option<Finalized>>,
) -> Result<(), Error> {
    while !finalized.is_closed() {
        let Subscription {
            id,
            mut notifications,
        } = rpc
            .subscribe(
                method::CHAIN_HEAD_FOLLOW,
                vec![false.into()],
                method::CHAIN_HEAD_UNFOLLOW,
            )
            .await?;
        counter!(metric::FOLLOW_SUBSCRIPTION_COUNT).increment(1);
        let mut tip = None;

        loop {
            let event = match timeout(recovery_timeout, notifications.next()).await {
                Ok(Some(Ok(event))) => event,
                Ok(Some(Err(error))) => {
                    warn!(error:%; "chainHead_v1_follow failed, resubscribing");
                    break;
                }
                Ok(None) => {
                    warn!("chainHead_v1_follow ended, resubscribing");
                    break;
                }
                Err(_) => {
                    warn!(recovery_timeout:?; "no chainHead_v1_follow event, resubscribing");
                    break;
                }
            };

            use FollowEvent::*;
            match serde_json::from_value(event).map_err(Error::FollowEvent)? {
                Initialized {
                    finalized_block_hashes,
                } => {
                    let hashes = block_hashes(finalized_block_hashes)?;
                    unpin(rpc, &id, &hashes).await;

                    if let Some(&hash) = hashes.last() {
                        let height = header_height(rpc, hash).await?;
                        tip = Some(BlockRef { hash, height });
                        publish(finalized, hashes, BlockRef { hash, height });
                    }
                }
                NewBlock { block_hash } => unpin(rpc, &id, &[block_hash_of(block_hash)?]).await,
                Finalized {
                    finalized_block_hashes,
                } => {
                    let hashes = block_hashes(finalized_block_hashes)?;
                    if let (Some(BlockRef { height, .. }), Some(&hash)) = (tip, hashes.last()) {
                        let height = height + hashes.len() as u64;
                        tip = Some(BlockRef { hash, height });
                        publish(finalized, hashes, BlockRef { hash, height });
                    }
                }
                Stop => {
                    warn!("chainHead_v1_follow stopped, resubscribing");
                    break;
                }
                Other => {}
            }

            if finalized.is_closed() {
                return Ok(());
            }
        }
    }

    Ok(())
}

/// A `chainHead_v1_follow` event, with `withRuntime` false.
#[derive(Debug, Deserialize)]
#[serde(
    tag = "event",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum FollowEvent {
    Initialized {
        finalized_block_hashes: Vec<String>,
    },
    NewBlock {
        block_hash: String,
    },
    Finalized {
        finalized_block_hashes: Vec<String>,
    },
    Stop,
    #[serde(other)]
    Other,
}

fn publish(finalized: &watch::Sender<Option<Finalized>>, hashes: Vec<BlockHash>, tip: BlockRef) {
    debug!(hash:% = tip.hash, height = tip.height; "block finalized");
    gauge!(metric::FINALIZED_HEIGHT).set(tip.height as f64);
    finalized.send_replace(Some(Finalized { hashes, tip }));
}

/// Unpin the given blocks; a failure only means the node has dropped them already.
async fn unpin<T: Transport>(rpc: &NodeRpc<T>, subscription: &Value, hashes: &[BlockHash]) {
    let batch = Batch::default().unpin(subscription.to_owned(), hashes);

    match rpc.batch(batch).await.map(|mut results| results.pop()) {
        Ok(Some(Ok(_))) => {}
        Ok(Some(Err(error))) => debug!(error:%; "cannot unpin blocks"),
        Ok(None) => {}
        Err(error) => debug!(error:%; "cannot unpin blocks"),
    }
}

/// The height of the given block, from its header.
async fn header_height<T: Transport>(rpc: &NodeRpc<T>, hash: BlockHash) -> Result<u64, Error> {
    let batch = Batch::default().header(hash);
    let header = rpc.batch(batch).await?.pop().expect("one result per call");
    let header = header_bytes(header, hash)?;

    Ok(decode_header(&header, hash)?.number)
}

fn block_hashes(hashes: Vec<String>) -> Result<Vec<BlockHash>, Error> {
    hashes.into_iter().map(block_hash_of).collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        infra::subxt_node::rpc::{Call, NodeRpc, ReconnectPolicy, method, testing::FakeNode},
        pipeline::sourcing::{Finalized, finalized::follow_finalized},
    };
    use indexer_common::domain::{BlockHash, ByteArray};
    use parity_scale_codec::Encode;
    use parking_lot::Mutex;
    use serde_json::{Value, json};
    use std::{num::NonZeroUsize, sync::Arc, time::Duration};
    use subxt::{
        config::substrate::{Digest, SubstrateHeader},
        utils::H256,
    };
    use tokio::{sync::watch, task, time::sleep};

    #[tokio::test]
    async fn test_signal() {
        let calls = Arc::new(Mutex::new(vec![]));
        let node = node(calls.clone()).with_subscriptions(vec![vec![
            json!({ "event": "initialized", "finalizedBlockHashes": [hex(1), hex(2)] }),
            json!({ "event": "newBlock", "blockHash": hex(3), "parentBlockHash": hex(2) }),
            json!({ "event": "bestBlockChanged", "bestBlockHash": hex(3) }),
            json!({ "event": "newBlock", "blockHash": hex(4), "parentBlockHash": hex(3) }),
            json!({ "event": "finalized", "finalizedBlockHashes": [hex(3), hex(4)], "prunedBlockHashes": [] }),
        ]]);
        let rpc = node_rpc(Arc::new(node));
        let (sender, mut receiver) = watch::channel(None);

        let task =
            task::spawn(
                async move { follow_finalized(&rpc, Duration::from_secs(5), &sender).await },
            );
        let finalized = latest(&mut receiver, 4).await;
        task.abort();

        assert_eq!(finalized.hashes, vec![hash(3), hash(4)]);
        assert_eq!(finalized.tip.hash, hash(4));
        assert_eq!(finalized.tip.height, 4);

        let calls = calls.lock();
        let headers = calls
            .iter()
            .filter(|call| call.method == method::ARCHIVE_HEADER)
            .count();
        assert_eq!(headers, 1);
        let unpinned = calls
            .iter()
            .filter(|call| call.method == method::CHAIN_HEAD_UNPIN)
            .map(|call| call.params[1].clone())
            .collect::<Vec<_>>();
        assert_eq!(
            unpinned,
            vec![json!([hex(1), hex(2)]), json!([hex(3)]), json!([hex(4)])]
        );
    }

    #[tokio::test]
    async fn test_skipped_update() {
        let calls = Arc::new(Mutex::new(vec![]));
        let node = node(calls).with_subscriptions(vec![vec![
            json!({ "event": "initialized", "finalizedBlockHashes": [hex(1)] }),
            json!({ "event": "finalized", "finalizedBlockHashes": [hex(2), hex(3)], "prunedBlockHashes": [] }),
            json!({ "event": "finalized", "finalizedBlockHashes": [hex(4)], "prunedBlockHashes": [] }),
        ]]);
        let rpc = node_rpc(Arc::new(node));
        let (sender, mut receiver) = watch::channel(None);

        let task =
            task::spawn(
                async move { follow_finalized(&rpc, Duration::from_secs(5), &sender).await },
            );
        let finalized = latest(&mut receiver, 4).await;
        task.abort();

        // Only the newest value is kept: its hashes cover its own heights only.
        assert_eq!(finalized.hashes, vec![hash(4)]);
        assert_eq!(finalized.tip.height, 4);
    }

    #[tokio::test]
    async fn test_stop_resubscribes() {
        let calls = Arc::new(Mutex::new(vec![]));
        let node = Arc::new(node(calls).with_subscriptions(vec![
            vec![
                json!({ "event": "initialized", "finalizedBlockHashes": [hex(1)] }),
                json!({ "event": "stop" }),
            ],
            vec![json!({ "event": "initialized", "finalizedBlockHashes": [hex(2)] })],
        ]));
        let rpc = node_rpc(node.clone());
        let (sender, mut receiver) = watch::channel(None);

        let task =
            task::spawn(
                async move { follow_finalized(&rpc, Duration::from_secs(5), &sender).await },
            );
        latest(&mut receiver, 2).await;
        task.abort();

        assert_eq!(node.subscribes(), 2);
    }

    #[tokio::test]
    async fn test_watchdog() {
        let calls = Arc::new(Mutex::new(vec![]));
        let new_blocks = (10..20)
            .map(|n| json!({ "event": "newBlock", "blockHash": hex(n), "parentBlockHash": hex(n - 1) }))
            .collect::<Vec<_>>();
        let node = Arc::new(
            node(calls)
                .with_notification_interval(Duration::from_millis(20))
                .with_subscriptions(vec![new_blocks]),
        );
        let rpc = node_rpc(node.clone());
        let (sender, _receiver) = watch::channel(None);

        let task = task::spawn(async move {
            follow_finalized(&rpc, Duration::from_millis(150), &sender).await
        });

        // Ten `newBlock` events 20 ms apart keep the subscription alive past the timeout, even
        // though no block is finalized.
        sleep(Duration::from_millis(300)).await;
        assert_eq!(node.subscribes(), 1);

        // Silence for the timeout renews it exactly once.
        sleep(Duration::from_millis(125)).await;
        assert_eq!(node.subscribes(), 2);

        task.abort();
    }

    fn hash(n: u8) -> BlockHash {
        ByteArray([n; 32])
    }

    fn header(number: u64) -> Value {
        let header = SubstrateHeader::<H256> {
            parent_hash: H256::zero(),
            number,
            state_root: H256::zero(),
            extrinsics_root: H256::zero(),
            digest: Digest::default(),
        };
        const_hex::encode_prefixed(header.encode()).into()
    }

    fn hex(n: u8) -> String {
        const_hex::encode_prefixed([n; 32])
    }

    async fn latest(finalized: &mut watch::Receiver<Option<Finalized>>, height: u64) -> Finalized {
        let finalized = tokio::time::timeout(
            Duration::from_secs(5),
            finalized.wait_for(|finalized| {
                finalized
                    .as_ref()
                    .is_some_and(|finalized| finalized.tip.height == height)
            }),
        )
        .await
        .expect("finalized in time")
        .expect("sender alive");

        finalized.clone().expect("finalized")
    }

    /// A node answering `archive_v1_header` with block `n` at height `n`, recording every call.
    fn node(calls: Arc<Mutex<Vec<Call>>>) -> FakeNode {
        FakeNode::new(move |call| {
            calls.lock().push(call.clone());
            match call.method {
                method::ARCHIVE_HEADER => {
                    let hash = call.params[0].as_str().expect("hash param");
                    let n = const_hex::decode(hash).expect("hex hash")[0];
                    Ok(header(n as u64))
                }
                _ => Ok(Value::Null),
            }
        })
    }

    fn node_rpc(node: Arc<FakeNode>) -> NodeRpc<Arc<FakeNode>> {
        NodeRpc::new(
            node,
            NonZeroUsize::new(64).unwrap(),
            NonZeroUsize::new(4).unwrap(),
            ReconnectPolicy {
                max_delay: Duration::from_millis(10),
                max_attempts: 3,
            },
        )
    }
}
