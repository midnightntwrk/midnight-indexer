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

use crate::infra::subxt_node::rpc::{
    Batch, Call, CallResult, Count, Error, MAX_SUBSCRIPTIONS, NodeRpc, REQUIRED_METHODS,
    ReconnectPolicy, TransportError, json_size, method, testing::FakeNode,
};
use futures::{StreamExt, TryStreamExt, stream};
use indexer_common::domain::{BlockNumber, ByteArray};
use serde_json::{Value, json};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::time::timeout;

const POLICY: ReconnectPolicy = ReconnectPolicy {
    max_delay: Duration::from_millis(10),
    max_attempts: 3,
};

#[tokio::test]
async fn test_packing() {
    let node = Arc::new(FakeNode::new(echo));
    let rpc = node_rpc(node.clone(), 4, 2);

    let results = rpc.batch(heights(10)).await.expect("batch succeeds");

    assert_eq!(node.batch_sizes(), vec![4, 4, 2]);
    let heights = results
        .into_iter()
        .map(|result| result.expect("call succeeds")["params"][0].clone())
        .collect::<Vec<_>>();
    assert_eq!(heights, (0..10).map(Value::from).collect::<Vec<_>>());
}

#[tokio::test]
async fn test_batches_in_flight_are_bounded() {
    let node = Arc::new(FakeNode::new(echo).with_delay(Duration::from_millis(20)));
    let rpc = node_rpc(node.clone(), 2, 3);

    rpc.batch(heights(20)).await.expect("batch succeeds");

    assert_eq!(node.batch_sizes().len(), 10);
    assert_eq!(node.max_in_flight(), 3);
}

#[tokio::test]
async fn test_rejected_batch() {
    let node = Arc::new(FakeNode::new(echo).with_max_batch_size(3));
    let rpc = node_rpc(node, 4, 1);

    let error = rpc.batch(heights(4)).await.expect_err("batch is rejected");

    assert!(matches!(error, Error::BatchRejected { batch_size: 4, .. }));
    assert!(error.to_string().contains("rpc_batch_size"));
}

#[tokio::test]
async fn test_payload_accounting() {
    let node = Arc::new(FakeNode::new(|call| match call.method {
        method::ARCHIVE_HASH_BY_HEIGHT => Ok(json!(["0x0101"])),
        method::ARCHIVE_HEADER => Ok(json!("0x020202")),
        _ => Ok(Value::Null),
    }));
    let rpc = node_rpc(node, 2, 1);

    let batch =
        heights(3)
            .header(ByteArray([1; 32]))
            .call(ByteArray([1; 32]), "Test_function", &[]);
    rpc.batch(batch).await.expect("batch succeeds");

    let counts = rpc.counters().counts();
    assert_eq!(
        counts[method::ARCHIVE_HASH_BY_HEIGHT],
        Count {
            requests: 3,
            request_bytes: 3,
            response_bytes: 3 * json_size(&json!(["0x0101"])) as u64,
        }
    );
    assert_eq!(
        counts[method::ARCHIVE_HEADER],
        Count {
            requests: 1,
            request_bytes: json_size(&json!(format!("0x{}", "01".repeat(32)))) as u64,
            response_bytes: json_size(&json!("0x020202")) as u64,
        }
    );
    assert_eq!(counts["archive_v1_call Test_function"].requests, 1);
    assert!(!counts.contains_key(method::ARCHIVE_CALL));
    let batches = rpc.counters().batches();
    assert_eq!(batches.batches, 3);
    assert_eq!(
        batches.largest_response_bytes,
        2 * json_size(&json!(["0x0101"])) as u64
    );
}

#[tokio::test]
async fn test_subscription_accounting() {
    let notifications = vec![json!({ "event": "initialized" }), json!("0x0102")];
    let node = Arc::new(FakeNode::new(echo).with_subscriptions(vec![notifications.clone()]));
    let rpc = node_rpc(node, 4, 1);

    let received = rpc
        .subscribe(
            method::CHAIN_HEAD_FOLLOW,
            vec![json!(false)],
            method::CHAIN_HEAD_UNFOLLOW,
        )
        .await
        .expect("subscription succeeds")
        .notifications
        .take(notifications.len())
        .try_collect::<Vec<_>>()
        .await
        .expect("notifications are received");

    assert_eq!(received, notifications);
    assert_eq!(
        rpc.counters().counts()[method::CHAIN_HEAD_FOLLOW],
        Count {
            requests: 1,
            request_bytes: json_size(&json!(false)) as u64,
            response_bytes: notifications.iter().map(json_size).sum::<usize>() as u64,
        }
    );
}

#[tokio::test]
async fn test_subscriptions_are_bounded() {
    let node = Arc::new(FakeNode::new(echo).with_subscribe(|_, _| Some(vec![])));
    let rpc = node_rpc(node, 4, 1);
    let subscribe = || {
        rpc.subscribe(
            method::ARCHIVE_STORAGE,
            vec![],
            method::ARCHIVE_STOP_STORAGE,
        )
    };

    let mut open = stream::iter(0..MAX_SUBSCRIPTIONS)
        .then(|_| subscribe())
        .try_collect::<Vec<_>>()
        .await
        .expect("subscriptions succeed");
    assert!(
        timeout(Duration::from_millis(50), subscribe())
            .await
            .is_err()
    );

    open.pop();
    timeout(Duration::from_millis(50), subscribe())
        .await
        .expect("subscription is not blocked")
        .expect("subscription succeeds");
}

#[test]
fn test_json_size() {
    let value = json!({ "a": [1, "bc", null, true], "d": { "e": false } });
    assert_eq!(json_size(&value), value.to_string().len());
}

#[tokio::test]
async fn test_reconnect() {
    let node = Arc::new(FakeNode::new(echo).with_disconnects(1));
    let rpc = node_rpc(node.clone(), 4, 1);

    rpc.batch(heights(2))
        .await
        .expect("batch succeeds after reconnecting");

    assert_eq!(node.reconnects(), 1);
}

#[tokio::test]
async fn test_unreachable() {
    let node = Arc::new(
        FakeNode::new(echo)
            .with_disconnects(1)
            .with_failing_reconnects(),
    );
    let rpc = node_rpc(node.clone(), 4, 1);

    let error = rpc
        .batch(heights(2))
        .await
        .expect_err("node is unreachable");

    assert!(matches!(error, Error::Unreachable { attempts: 3, .. }));
    assert_eq!(node.reconnects(), 3);
}

#[tokio::test]
async fn test_retry() {
    let down = || TransportError::Disconnected("node is down".into());

    // Up after two failed retries.
    let attempts = AtomicUsize::new(0);
    let value = POLICY
        .retry(down(), || async {
            match attempts.fetch_add(1, Ordering::SeqCst) {
                0 | 1 => Err(down()),
                _ => Ok(7),
            }
        })
        .await
        .expect("third retry succeeds");
    assert_eq!(value, 7);
    assert_eq!(attempts.load(Ordering::SeqCst), 3);

    // Never up: the policy's retries, then unreachable.
    let attempts = AtomicUsize::new(0);
    let error = POLICY
        .retry(down(), || async {
            attempts.fetch_add(1, Ordering::SeqCst);
            Err::<(), _>(down())
        })
        .await
        .expect_err("node stays down");
    assert!(matches!(error, Error::Unreachable { attempts: 3, .. }));
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_check_methods() {
    let node = Arc::new(FakeNode::new(|call| match call.method {
        method::RPC_METHODS => Ok(json!({
            "methods": REQUIRED_METHODS
                .iter()
                .filter(|method| !method.starts_with("archive_v1_"))
                .collect::<Vec<_>>()
        })),
        _ => Ok(Value::Null),
    }));
    let rpc = node_rpc(node, 4, 1);

    let error = rpc
        .check_methods()
        .await
        .expect_err("archive methods are missing");

    let Error::MissingMethods { missing } = &error else {
        panic!("unexpected error {error:?}");
    };
    assert_eq!(missing.len(), 7);
    assert!(
        missing
            .iter()
            .all(|method| method.starts_with("archive_v1_"))
    );
    assert!(error.to_string().contains("--state-pruning archive"));
}

/// Echo the call, so results can be matched to calls.
fn echo(call: &Call) -> CallResult {
    Ok(json!({ "method": call.method, "params": call.params }))
}

fn heights(n: BlockNumber) -> Batch {
    (0..n).fold(Batch::default(), Batch::hash_by_height)
}

fn node_rpc(node: Arc<FakeNode>, batch_size: usize, in_flight: usize) -> NodeRpc<Arc<FakeNode>> {
    NodeRpc::new(
        node,
        NonZeroUsize::new(batch_size).unwrap(),
        NonZeroUsize::new(in_flight).unwrap(),
        POLICY,
    )
}
