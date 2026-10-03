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

//! The node's rate for each kind of call the source pipeline makes, one kind at a time, each on its
//! own range of heights so no kind is served from another's cache. Run against `NODE_URL` with
//! `SOURCE_FROM` (default 2,000,000), `SOURCE_COUNT` heights per kind (default 5,000) and
//! `PROBE_SUBSCRIPTIONS` storage queries open at once (default 256), and batches of
//! `RPC_BATCH_SIZE` calls (default 64), `RPC_BATCHES_IN_FLIGHT` at once (default 16).
//! `PROBE_KINDS` limits the run to kinds whose name contains one of its comma-separated parts.

use chain_indexer::{
    infra::subxt_node::rpc::{Batch, NodeRpc, ReconnectPolicy, Transport, hex, method},
    pipeline::source::{
        self, AUTHORITY_SET_ITEMS, SYSTEM_EVENTS_ITEM, Source, resolve, storage_key,
    },
};
use futures::{StreamExt, TryStreamExt, stream};
use indexer_common::domain::BlockHash;
use serde_json::{Value, json};
use std::{
    env,
    num::NonZeroUsize,
    time::{Duration, Instant},
};

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a node at NODE_URL"]
async fn node_calls() {
    let url = env::var("NODE_URL").expect("NODE_URL is set");
    let setting = |name, default| {
        env::var(name)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    };
    let from = setting("SOURCE_FROM", 2_000_000);
    let count = setting("SOURCE_COUNT", 5_000);
    let subscriptions = setting("PROBE_SUBSCRIPTIONS", 256) as usize;
    let batch_size = setting("RPC_BATCH_SIZE", 64) as usize;
    let batches_in_flight = setting("RPC_BATCHES_IN_FLIGHT", 16) as usize;
    let config = source::Config {
        chunk_size: NonZeroUsize::new(64).unwrap(),
        chunks_ahead: NonZeroUsize::new(8).unwrap(),
        rpc_batch_size: NonZeroUsize::new(batch_size).unwrap(),
        rpc_batches_in_flight: NonZeroUsize::new(batches_in_flight).unwrap(),
        recovery_timeout: Duration::from_secs(30),
        reconnect_policy: ReconnectPolicy {
            max_delay: Duration::from_secs(1),
            max_attempts: 10,
        },
    };
    let source = Source::connect(&url, config)
        .await
        .expect("node serves the required methods");
    let rpc = source.rpc();

    let item = |item, query_type| json!({ "key": hex(storage_key(item)), "type": query_type });
    let kinds = [
        Kind::Batch("archive_v1_header", |batch, hash| {
            batch.header(hash);
        }),
        Kind::Batch("archive_v1_body", |batch, hash| {
            batch.body(hash);
        }),
        Kind::Batch("get_zswap_state_root", |batch, hash| {
            batch.call(hash, "MidnightRuntimeApi_get_zswap_state_root", &[]);
        }),
        Kind::Batch("get_ledger_state_root", |batch, hash| {
            batch.call(hash, "MidnightRuntimeApi_get_ledger_state_root", &[]);
        }),
        Kind::Storage(
            "storage System.Events value",
            vec![item(SYSTEM_EVENTS_ITEM, "value")],
        ),
        Kind::Storage(
            "storage Aura.Authorities value",
            vec![item(AUTHORITY_SET_ITEMS[0], "value")],
        ),
        Kind::Storage(
            "storage Aura.Authorities hash",
            vec![item(AUTHORITY_SET_ITEMS[0], "hash")],
        ),
    ];

    let filter = env::var("PROBE_KINDS").unwrap_or_default();
    let kinds = kinds
        .into_iter()
        .filter(|kind| {
            filter.is_empty() || filter.split(',').any(|part| kind.name().contains(part))
        })
        .collect::<Vec<_>>();

    println!("| kind | heights | calls/s |\n|---|---|---:|");
    let started = Instant::now();
    let hashes = resolve(rpc, from..=from + count * kinds.len() as u64 - 1)
        .await
        .expect("hashes resolve")
        .into_iter()
        .map(|hash| hash.expect("one block per height"))
        .collect::<Vec<_>>();
    println!(
        "| archive_v1_hashByHeight | {from}.. | {:.0} |",
        hashes.len() as f64 / started.elapsed().as_secs_f64()
    );

    for (i, (kind, hashes)) in kinds.iter().zip(hashes.chunks(count as usize)).enumerate() {
        let started = Instant::now();
        match kind {
            Kind::Batch(_, add) => {
                stream::iter(hashes.chunks(batch_size))
                    .map(|hashes| {
                        let batch = hashes.iter().fold(Batch::default(), |mut batch, &hash| {
                            add(&mut batch, hash);
                            batch
                        });
                        rpc.batch(batch)
                    })
                    .buffer_unordered(batches_in_flight)
                    .try_for_each(|_| async { Ok(()) })
                    .await
                    .expect("calls succeed");
            }
            Kind::Storage(_, items) => {
                stream::iter(hashes.iter().copied())
                    .map(|hash| query(rpc, hash, items.clone()))
                    .buffer_unordered(subscriptions)
                    .collect::<Vec<_>>()
                    .await;
            }
        }
        let start = from + i as u64 * count;
        println!(
            "| {} | {start}..{} | {:.0} |",
            kind.name(),
            start + count,
            hashes.len() as f64 / started.elapsed().as_secs_f64()
        );
    }
}

enum Kind {
    Batch(&'static str, fn(&mut Batch, BlockHash)),
    Storage(&'static str, Vec<Value>),
}

impl Kind {
    fn name(&self) -> &'static str {
        match self {
            Self::Batch(name, _) | Self::Storage(name, _) => name,
        }
    }
}

/// One storage query, drained until `storageDone`.
async fn query(rpc: &NodeRpc<impl Transport>, hash: BlockHash, items: Vec<Value>) {
    let mut notifications = rpc
        .subscribe(
            method::ARCHIVE_STORAGE,
            vec![hex(hash.0), items.into(), Value::Null],
            method::ARCHIVE_STOP_STORAGE,
        )
        .await
        .expect("storage subscription")
        .notifications;
    while let Some(notification) = notifications.next().await {
        if notification.expect("storage event")["event"] == "storageDone" {
            break;
        }
    }
}
