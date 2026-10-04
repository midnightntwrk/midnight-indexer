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

//! Throughput of the block sourcing pipeline, optionally with the decode stage, against the node at
//! `NODE_URL`. Run with `just source-throughput url from count`.
//!
//! Settings, from env vars:
//! - `SOURCE_FROM`, `SOURCE_COUNT`: the heights sourced; from genesis, and up to the finalized
//!   height at start, if unset or empty;
//! - `SOURCE_CHUNK_SIZE`, `SOURCE_CHUNKS_AHEAD`, `RPC_BATCH_SIZE`, `RPC_BATCHES_IN_FLIGHT`;
//! - `DECODE_CPU_THREADS`: decode on that many threads, by default `decode_cpu_threads`'s default
//!   (one less than the available cores); no decode if 0;
//! - `CONSUMER_SLEEP_MS`: the time the consumer sleeps per block.
//!
//! Every 10 s it prints the last interval's rates and per-block sizes, so the cause of a slowdown
//! shows at a glance; at the end it prints totals, requests and bytes per kind, and stage times.

use chain_indexer::{
    application::default_decode_cpu_threads,
    domain::BlockRef,
    infra::subxt_node::rpc::{Call, ReconnectPolicy, method},
    pipeline::{
        decode::{self, CpuPool},
        sourcing::{self, Source, resolve},
    },
};
use futures::{StreamExt, TryStreamExt};
use indexer_common::domain::BlockNumber;
use serde_json::Value;
use std::{
    env,
    fmt::Display,
    num::NonZeroUsize,
    str::FromStr,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};
use tokio::task;

mod consumer;
mod recorder;
mod report;

use self::{
    consumer::{Consumer, Sourced},
    recorder::{HarnessRecorder, RECORDED},
    report::{cpu_seconds, group, print_table, progress, report},
};

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a node at NODE_URL"]
async fn source_throughput() {
    let url = env::var("NODE_URL").expect("NODE_URL is set");
    let from = setting::<BlockNumber>("SOURCE_FROM", 0);
    let count = optional_setting::<BlockNumber>("SOURCE_COUNT");
    let config = sourcing::Config {
        chunk_size: setting("SOURCE_CHUNK_SIZE", NonZeroUsize::new(64).unwrap()),
        chunks_ahead: setting("SOURCE_CHUNKS_AHEAD", NonZeroUsize::new(8).unwrap()),
        rpc_batch_size: setting("RPC_BATCH_SIZE", NonZeroUsize::new(64).unwrap()),
        rpc_batches_in_flight: setting("RPC_BATCHES_IN_FLIGHT", NonZeroUsize::new(16).unwrap()),
        recovery_timeout: Duration::from_secs(30),
        reconnect_policy: ReconnectPolicy {
            max_delay: Duration::from_secs(1),
            max_attempts: 10,
        },
    };
    let decode_cpu_threads = match optional_setting::<usize>("DECODE_CPU_THREADS") {
        Some(threads) => NonZeroUsize::new(threads),
        None => Some(default_decode_cpu_threads()),
    };
    let consumer_sleep = Duration::from_millis(setting("CONSUMER_SLEEP_MS", 0));

    metrics::set_global_recorder(HarnessRecorder).expect("no other recorder is set");

    let source = Source::connect(&url, config)
        .await
        .expect("node serves the required methods");
    let end = match count {
        Some(count) => from + count - 1,
        None => source
            .rpc()
            .call::<BlockNumber>(Call {
                method: method::ARCHIVE_FINALIZED_HEIGHT,
                params: vec![],
            })
            .await
            .expect("finalized height"),
    };
    let count = end + 1 - from;
    let rpc = source.rpc();
    let node_call = |method| async move {
        rpc.call::<Value>(Call {
            method,
            params: vec![],
        })
        .await
        .map(|value| {
            value
                .as_str()
                .map(ToOwned::to_owned)
                .unwrap_or(value.to_string())
        })
        .unwrap_or_else(|error| format!("unknown ({error})"))
    };
    let chain = node_call("chainSpec_v1_chainName").await;
    let node_version = node_call("system_version").await;
    let finalized = node_call(method::ARCHIVE_FINALIZED_HEIGHT).await;
    print_table(
        &["", ""],
        vec![
            vec!["node".to_owned(), url.clone()],
            vec!["chain".to_owned(), chain],
            vec!["node version".to_owned(), node_version],
            vec!["finalized height".to_owned(), finalized],
            vec![
                "heights".to_owned(),
                format!("{from}..={end} ({} blocks)", group(count.into())),
            ],
            vec![
                "settings".to_owned(),
                format!(
                    "chunk size {}, chunks ahead {}, batch size {}, batches in flight {}",
                    config.chunk_size,
                    config.chunks_ahead,
                    config.rpc_batch_size,
                    config.rpc_batches_in_flight
                ),
            ],
            vec![
                "decode threads".to_owned(),
                decode_cpu_threads.map_or("none".to_owned(), |threads| threads.to_string()),
            ],
            vec![
                "consumer sleep".to_owned(),
                format!("{consumer_sleep:?} per block"),
            ],
        ],
    );
    println!();
    let start = match from {
        0 => None,
        from => {
            let hash = resolve(source.rpc(), from - 1..=from - 1)
                .await
                .expect("hash resolves")
                .pop()
                .flatten()
                .expect("one block at the height before SOURCE_FROM");
            Some(BlockRef {
                hash,
                height: (from - 1).into(),
            })
        }
    };

    let cpu_start = cpu_seconds();
    let started = Instant::now();
    let (chunks, _) = source.run(start, Some(end));
    let sourced = Arc::new(Sourced::default());
    let chunks = chunks.inspect({
        let sourced = sourced.clone();
        move |chunk| {
            if let Ok(chunk) = chunk {
                sourced.add(chunk)
            }
        }
    });
    let progress = task::spawn(progress(
        source.rpc().clone(),
        sourced.clone(),
        decode_cpu_threads,
    ));

    let mut consumer = Consumer::new(consumer_sleep, started);
    match decode_cpu_threads {
        Some(threads) => {
            let pool = Arc::new(CpuPool::new(threads).expect("pool builds"));
            decode::decode(chunks, pool, config.chunk_size)
                .for_each(|block| {
                    consumer.receive(block.map(|block| {
                        RECORDED
                            .transactions
                            .fetch_add(block.transactions.len() as u64, Ordering::Relaxed);
                        1
                    }))
                })
                .await
        }
        None => {
            chunks
                .map_ok(|chunk| chunk.len() as u64)
                .map_err(decode::Error::from)
                .for_each(|blocks| consumer.receive(blocks))
                .await
        }
    }
    let wall = started.elapsed();
    let cpu = cpu_seconds().zip(cpu_start).map(|(end, start)| end - start);
    progress.abort();

    report(&source, &consumer, &sourced, wall, cpu, decode_cpu_threads);
    assert_eq!(consumer.blocks, u64::from(count), "every block is received");
}

fn setting<T: FromStr<Err: Display>>(name: &str, default: T) -> T {
    optional_setting(name).unwrap_or(default)
}

/// The setting of the given name, `None` if unset or empty.
fn optional_setting<T: FromStr<Err: Display>>(name: &str) -> Option<T> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .parse()
                .unwrap_or_else(|error| panic!("{name}: {error}"))
        })
}
