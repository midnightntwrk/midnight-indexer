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
//! - `DECODE_CPU_THREADS`: decode on that many threads, no decode if unset;
//! - `CONSUMER_SLEEP_MS`: the time the consumer sleeps per block.
//!
//! Every 10 s it prints the last interval's rates and per-block sizes, so the cause of a slowdown
//! shows at a glance; at the end it prints totals, requests and bytes per kind, and stage times.

use chain_indexer::{
    domain::BlockRef,
    infra::subxt_node::rpc::{Call, Count, Counters, ReconnectPolicy, Transport, method},
    pipeline::{
        decode::{self, CpuPool},
        metric,
        sourcing::{self, Block, Source, metadata_spec_version, resolve},
    },
};
use futures::{StreamExt, TryStreamExt};
use metrics::{
    Counter, CounterFn, Gauge, Histogram, HistogramFn, Key, KeyName, Metadata, Recorder,
    SharedString, Unit,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    env,
    fmt::Display,
    fs,
    num::NonZeroUsize,
    str::FromStr,
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{task, time::sleep};

/// The time resolution of the recorded arrivals.
const ARRIVAL_RESOLUTION: Duration = Duration::from_millis(10);
/// How often progress is printed, and every how many lines its column header is repeated.
const PROGRESS_INTERVAL: Duration = Duration::from_secs(10);
const PROGRESS_HEADER_EVERY: u64 = 20;
/// Clock ticks per second of `/proc/self/stat`'s CPU times, `USER_HZ`, fixed on Linux.
const USER_HZ: f64 = 100.0;

static RECORDED: LazyLock<Recorded> = LazyLock::new(Recorded::default);

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a node at NODE_URL"]
async fn source_throughput() {
    let url = env::var("NODE_URL").expect("NODE_URL is set");
    let from = setting("SOURCE_FROM", 0u64);
    let count = optional_setting::<u64>("SOURCE_COUNT");
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
    let decode_cpu_threads = optional_setting::<NonZeroUsize>("DECODE_CPU_THREADS");
    let consumer_sleep = Duration::from_millis(setting("CONSUMER_SLEEP_MS", 0));

    metrics::set_global_recorder(HarnessRecorder).expect("no other recorder is set");

    let source = Source::connect(&url, config)
        .await
        .expect("node serves the required methods");
    let end = match count {
        Some(count) => from + count - 1,
        None => source
            .rpc()
            .call::<u64>(Call {
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
                format!("{from}..={end} ({} blocks)", group(count)),
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
                height: from - 1,
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
    assert_eq!(consumer.blocks, count, "every block is received");
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

/// Receives blocks, sleeps per block, and records when blocks arrive.
struct Consumer {
    sleep: Duration,
    started: Instant,
    blocks: u64,
    errors: Vec<String>,
    /// The blocks received so far, at most one entry per [ARRIVAL_RESOLUTION].
    arrivals: Vec<(Duration, u64)>,
}

impl Consumer {
    fn new(sleep: Duration, started: Instant) -> Self {
        Self {
            sleep,
            started,
            blocks: 0,
            errors: vec![],
            arrivals: vec![],
        }
    }

    /// The blocks received in each of `n` equal slices of `wall`.
    fn slices(&self, wall: Duration, n: u32) -> Vec<u64> {
        let received_by = |at: Duration| {
            self.arrivals
                .iter()
                .take_while(|(arrival, _)| *arrival <= at)
                .last()
                .map_or(0, |(_, blocks)| *blocks)
        };
        (1..=n)
            .map(|i| received_by(wall * i / n) - received_by(wall * (i - 1) / n))
            .collect()
    }

    fn receive(&mut self, blocks: Result<u64, decode::Error>) -> impl Future<Output = ()> + use<> {
        let sleep_for = match blocks {
            Ok(blocks) => {
                self.blocks += blocks;
                RECORDED.consumed.fetch_add(blocks, Ordering::Relaxed);
                let now = self.started.elapsed();
                match self.arrivals.last_mut() {
                    Some((at, blocks)) if now - *at < ARRIVAL_RESOLUTION => *blocks = self.blocks,
                    _ => self.arrivals.push((now, self.blocks)),
                }
                self.sleep * blocks as u32
            }
            Err(error) => {
                let chain =
                    std::iter::successors(Some(&error as &dyn std::error::Error), |e| e.source())
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(": ");
                println!("error: {chain}");
                self.errors.push(chain);
                Duration::ZERO
            }
        };

        async move {
            if !sleep_for.is_zero() {
                sleep(sleep_for).await;
            }
        }
    }
}

/// What the sourced chunks carried: sizes keyed by extrinsic count (genesis, under `None`, belongs
/// to neither empty nor non-empty), blocks per runtime spec version, and the latest runtime.
#[derive(Default)]
struct Sourced {
    sizes: Mutex<BTreeMap<Option<usize>, Totals>>,
    runtimes: Mutex<(HashMap<usize, u32>, BTreeMap<u32, u64>)>,
    latest_runtime: AtomicU64,
}

impl Sourced {
    fn add(&self, chunk: &[Block]) {
        let mut sizes = self.sizes.lock().unwrap();
        let mut runtimes = self.runtimes.lock().unwrap();
        let (by_metadata, blocks) = &mut *runtimes;
        chunk.iter().for_each(|block| {
            let sizes_of = BlockSizes::of(block);
            let key = (block.height() > 0).then_some(sizes_of.extrinsic_count);
            sizes.entry(key).or_default().add(&sizes_of);

            let metadata = match block {
                Block::Genesis { metadata, .. } | Block::Block { metadata, .. } => metadata,
            };
            let version = *by_metadata
                .entry(Arc::as_ptr(metadata) as usize)
                .or_insert_with(|| metadata_spec_version(metadata).unwrap_or_default());
            *blocks.entry(version).or_default() += 1;
            self.latest_runtime.store(version as u64, Ordering::Relaxed);
        });
    }

    fn totals(&self) -> Totals {
        self.sizes
            .lock()
            .unwrap()
            .values()
            .fold(Totals::default(), |totals, other| totals.merge(other))
    }
}

/// The counters progress lines compare between intervals.
#[derive(Default)]
struct Snapshot {
    at: Duration,
    blocks: u64,
    requests: u64,
    wire_bytes: u64,
    transactions: u64,
    decode_busy: f64,
    sizes: Totals,
}

impl Snapshot {
    fn take(at: Duration, counters: &Counters, sourced: &Sourced) -> Self {
        let counts = counters.counts();
        // Storage items count decoded bytes; their wire bytes are under `archive_v1_storage`.
        let wire = counts
            .iter()
            .filter(|(key, _)| !key.starts_with(&format!("{} ", method::ARCHIVE_STORAGE)));
        Self {
            at,
            blocks: RECORDED.consumed.load(Ordering::Relaxed),
            requests: wire.clone().map(|(_, count)| count.requests).sum(),
            wire_bytes: wire
                .map(|(_, count)| count.request_bytes + count.response_bytes)
                .sum(),
            transactions: RECORDED.transactions.load(Ordering::Relaxed),
            decode_busy: RECORDED.summary(metric::DECODE_BLOCK_DURATION).1,
            sizes: sourced.totals(),
        }
    }
}

/// Every [PROGRESS_INTERVAL], a line of the last interval's rates and per-block sizes.
async fn progress(
    rpc: chain_indexer::infra::subxt_node::rpc::NodeRpc<impl Transport>,
    sourced: Arc<Sourced>,
    decode_cpu_threads: Option<NonZeroUsize>,
) {
    let started = Instant::now();
    let headers = [
        "time",
        "blocks",
        "blk/s",
        "calls/s",
        "wire/s",
        "ext/blk",
        "tx/blk",
        "header",
        "body",
        "events",
        "decode",
        "in flight",
        "runtime",
    ];
    let widths = [7, 11, 7, 8, 9, 7, 7, 7, 8, 7, 7, 9, 9];
    let line = |cells: &[String]| {
        cells
            .iter()
            .zip(widths)
            .map(|(cell, width)| format!("{cell:>width$}"))
            .collect::<Vec<_>>()
            .join("  ")
    };

    let mut previous = Snapshot::default();
    for n in 0u64.. {
        sleep(PROGRESS_INTERVAL).await;
        let now = Snapshot::take(started.elapsed(), rpc.counters(), &sourced);
        let seconds = (now.at - previous.at).as_secs_f64();
        let blocks = now.blocks - previous.blocks;
        let sourced_blocks = (now.sizes.blocks - previous.sizes.blocks).max(1) as f64;
        let per_block = |field: usize| {
            fmt_bytes(
                (now.sizes.fields[field] - previous.sizes.fields[field]) as f64 / sourced_blocks,
            )
        };
        let in_flight = RECORDED
            .sourced
            .load(Ordering::Relaxed)
            .saturating_sub(now.blocks);

        if n.is_multiple_of(PROGRESS_HEADER_EVERY) {
            println!("{}", line(&headers.map(ToOwned::to_owned)));
        }
        println!(
            "{}",
            line(&[
                format!("{:.0} s", now.at.as_secs_f64()),
                group(now.blocks),
                group((blocks as f64 / seconds) as u64),
                group(((now.requests - previous.requests) as f64 / seconds) as u64),
                fmt_bytes((now.wire_bytes - previous.wire_bytes) as f64 / seconds),
                format!(
                    "{:.1}",
                    (now.sizes.extrinsics - previous.sizes.extrinsics) as f64 / sourced_blocks
                ),
                match decode_cpu_threads {
                    Some(_) => format!(
                        "{:.2}",
                        (now.transactions - previous.transactions) as f64 / blocks.max(1) as f64
                    ),
                    None => "-".to_owned(),
                },
                per_block(0),
                per_block(1),
                per_block(2),
                match decode_cpu_threads {
                    Some(threads) => format!(
                        "{:.0}%",
                        100.0 * (now.decode_busy - previous.decode_busy)
                            / (threads.get() as f64 * seconds)
                    ),
                    None => "-".to_owned(),
                },
                group(in_flight),
                sourced.latest_runtime.load(Ordering::Relaxed).to_string(),
            ])
        );
        previous = now;
    }
}

/// Print a table with aligned columns: the first left-aligned, the others right-aligned. Without
/// headers it is a list of names and values, all left-aligned.
fn print_table(headers: &[&str], rows: Vec<Vec<String>>) {
    let widths = (0..headers.len())
        .map(|column| {
            rows.iter()
                .map(|row| row[column].chars().count())
                .chain([headers[column].chars().count()])
                .max()
                .unwrap_or(0)
        })
        .collect::<Vec<_>>();
    let headed = headers.iter().any(|header| !header.is_empty());
    let line = |cells: Vec<String>| {
        cells
            .iter()
            .zip(&widths)
            .enumerate()
            .map(|(column, (cell, &width))| match column {
                0 => format!("{cell:<width$}"),
                _ if headed => format!("{cell:>width$}"),
                _ => format!("{cell:<width$}"),
            })
            .collect::<Vec<_>>()
            .join("  ")
            .trim_end()
            .to_owned()
    };

    if headed {
        println!("{}", line(headers.iter().map(|h| h.to_string()).collect()));
        println!(
            "{}",
            widths
                .iter()
                .map(|&width| "-".repeat(width))
                .collect::<Vec<_>>()
                .join("  ")
        );
    }
    rows.into_iter().for_each(|row| println!("{}", line(row)));
}

/// An integer with thousands separators.
fn group(n: u64) -> String {
    let digits = n.to_string();
    digits
        .chars()
        .enumerate()
        .flat_map(|(i, digit)| {
            let separator = (i > 0 && (digits.len() - i).is_multiple_of(3)).then_some(',');
            separator.into_iter().chain([digit])
        })
        .collect()
}

/// A byte count in B, KB, MB or GB (powers of 1,000).
fn fmt_bytes(bytes: f64) -> String {
    match bytes {
        b if b < 1e3 => format!("{b:.0} B"),
        b if b < 1e6 => format!("{:.1} KB", b / 1e3),
        b if b < 1e9 => format!("{:.1} MB", b / 1e6),
        b => format!("{:.2} GB", b / 1e9),
    }
}

/// Decoded bytes of one sourced block, by data type.
#[derive(Debug, Default, Clone, Copy)]
struct BlockSizes {
    extrinsic_count: usize,
    header: usize,
    body: usize,
    events: usize,
    authorities: usize,
    zswap_state_root: usize,
    ledger_state_root: usize,
    system_parameters: usize,
    genesis: usize,
}

impl BlockSizes {
    fn of(block: &Block) -> Self {
        match block {
            Block::Genesis {
                header,
                zswap_state_root,
                ledger_state_root,
                system_parameters: (d_parameter, terms_and_conditions),
                ledger_state,
                cnight_mappings,
                extrinsics,
                events,
                ..
            } => Self {
                extrinsic_count: extrinsics.len(),
                header: header.len(),
                body: extrinsics.iter().map(|e| e.len()).sum(),
                events: events.len(),
                zswap_state_root: zswap_state_root.len(),
                ledger_state_root: ledger_state_root.len(),
                system_parameters: d_parameter.len() + terms_and_conditions.len(),
                genesis: ledger_state.len()
                    + cnight_mappings
                        .iter()
                        .map(|(k, v)| k.len() + v.len())
                        .sum::<usize>(),
                ..Default::default()
            },
            Block::Block {
                header,
                zswap_state_root,
                ledger_state_root,
                system_parameters,
                parent,
                extrinsics,
                events,
                ..
            } => Self {
                extrinsic_count: extrinsics.len(),
                header: header.len(),
                body: extrinsics.iter().map(|e| e.len()).sum(),
                events: events.len(),
                authorities: parent.authority_set.iter().map(|(_, v)| v.len()).sum(),
                zswap_state_root: zswap_state_root.len(),
                ledger_state_root: ledger_state_root.len(),
                system_parameters: system_parameters
                    .as_ref()
                    .map(|(d, t)| d.len() + t.len())
                    .unwrap_or_default(),
                genesis: 0,
            },
        }
    }

    fn fields(&self) -> [(&'static str, usize); 8] {
        [
            ("header", self.header),
            ("body", self.body),
            ("events", self.events),
            ("authorities (parent state)", self.authorities),
            ("zswap state root", self.zswap_state_root),
            ("ledger state root", self.ledger_state_root),
            ("system parameters", self.system_parameters),
            ("genesis ledger state, cNight mappings", self.genesis),
        ]
    }
}

/// Decoded bytes summed over blocks.
#[derive(Debug, Default, Clone, Copy)]
struct Totals {
    blocks: u64,
    extrinsics: usize,
    fields: [usize; 8],
}

impl Totals {
    fn add(&mut self, block: &BlockSizes) {
        self.blocks += 1;
        self.extrinsics += block.extrinsic_count;
        self.fields
            .iter_mut()
            .zip(block.fields())
            .for_each(|(total, (_, bytes))| *total += bytes);
    }

    fn merge(mut self, other: &Self) -> Self {
        self.blocks += other.blocks;
        self.extrinsics += other.extrinsics;
        self.fields
            .iter_mut()
            .zip(other.fields)
            .for_each(|(total, bytes)| *total += bytes);
        self
    }

    fn mean(&self, field: usize) -> f64 {
        self.fields[field] as f64 / self.blocks.max(1) as f64
    }
}

fn report(
    source: &Source<impl Transport>,
    consumer: &Consumer,
    sourced: &Sourced,
    wall: Duration,
    cpu: Option<f64>,
    decode_cpu_threads: Option<NonZeroUsize>,
) {
    let blocks = consumer.blocks.max(1) as f64;
    let wall_secs = wall.as_secs_f64();

    println!("\nTHROUGHPUT\n");
    print_table(
        &["", ""],
        vec![
            vec!["blocks".to_owned(), group(consumer.blocks)],
            vec!["time".to_owned(), format!("{wall_secs:.1} s")],
            vec![
                "blocks/s".to_owned(),
                group((consumer.blocks as f64 / wall_secs) as u64),
            ],
            vec![
                "blocks/s by tenth of the run".to_owned(),
                consumer
                    .slices(wall, 10)
                    .iter()
                    .map(|blocks| group((*blocks as f64 * 10.0 / wall_secs) as u64))
                    .collect::<Vec<_>>()
                    .join("  "),
            ],
            vec!["errors".to_owned(), consumer.errors.len().to_string()],
        ],
    );

    println!("\nRUNTIMES\n");
    print_table(
        &["spec version", "blocks"],
        sourced
            .runtimes
            .lock()
            .unwrap()
            .1
            .iter()
            .map(|(version, blocks)| vec![version.to_string(), group(*blocks)])
            .collect(),
    );

    println!("\nREQUESTS, by method, runtime function or storage item\n");
    println!(
        "Bytes are JSON wire bytes, except for storage items (decoded values); a storage\n\
         subscription's notifications are counted under archive_v1_storage.\n"
    );
    print_table(
        &[
            "key",
            "requests",
            "per block",
            "request bytes",
            "response bytes",
            "response per block",
        ],
        source
            .counters()
            .counts()
            .into_iter()
            .map(|(key, count)| {
                let Count {
                    requests,
                    request_bytes,
                    response_bytes,
                } = count;
                vec![
                    key,
                    group(requests),
                    format!("{:.3}", requests as f64 / blocks),
                    fmt_bytes(request_bytes as f64),
                    fmt_bytes(response_bytes as f64),
                    fmt_bytes(response_bytes as f64 / blocks),
                ]
            })
            .collect(),
    );
    let batches = source.counters().batches();
    println!(
        "\n{} batches, largest response {}",
        group(batches.batches),
        fmt_bytes(batches.largest_response_bytes as f64)
    );

    // Blocks with the run's fewest extrinsics carry nothing but inherents.
    let sizes = sourced.sizes.lock().unwrap();
    let fewest = sizes.keys().flatten().next().copied().unwrap_or(0);
    let empty = sizes.get(&Some(fewest)).copied().unwrap_or_default();
    let non_empty = sizes
        .range(Some(fewest + 1)..)
        .fold(Totals::default(), |totals, (_, other)| totals.merge(other));
    let all = sizes
        .values()
        .fold(Totals::default(), |totals, other| totals.merge(other));
    println!("\nDECODED BYTES PER BLOCK, by data type\n");
    println!(
        "Empty blocks have the run's fewest extrinsics ({fewest}): {} empty, {} non-empty.\n",
        group(empty.blocks),
        group(non_empty.blocks)
    );
    let mut rows = BlockSizes::default()
        .fields()
        .iter()
        .enumerate()
        .map(|(field, (name, _))| {
            vec![
                name.to_string(),
                fmt_bytes(all.mean(field)),
                fmt_bytes(empty.mean(field)),
                fmt_bytes(non_empty.mean(field)),
            ]
        })
        .collect::<Vec<_>>();
    rows.push(vec![
        "body per extrinsic".to_owned(),
        fmt_bytes(all.fields[1] as f64 / all.extrinsics.max(1) as f64),
        String::new(),
        String::new(),
    ]);
    rows.push(vec![
        "extrinsics".to_owned(),
        format!("{:.2}", all.extrinsics as f64 / all.blocks.max(1) as f64),
        format!(
            "{:.2}",
            empty.extrinsics as f64 / empty.blocks.max(1) as f64
        ),
        format!(
            "{:.2}",
            non_empty.extrinsics as f64 / non_empty.blocks.max(1) as f64
        ),
    ]);
    if decode_cpu_threads.is_some() {
        rows.push(vec![
            "transactions".to_owned(),
            format!(
                "{:.2}",
                RECORDED.transactions.load(Ordering::Relaxed) as f64 / blocks
            ),
            String::new(),
            String::new(),
        ]);
    }
    print_table(&["type", "all", "empty", "non-empty"], rows);

    println!("\nSTAGES\n");
    println!(
        "Busy share is the stage's total time over wall time; above 1 means it runs concurrently.\n"
    );
    print_table(
        &["stage", "runs", "total s", "mean ms", "busy share"],
        [
            ("resolve", metric::RESOLVE_DURATION),
            ("source", metric::SOURCE_DURATION),
            ("verify", metric::VERIFY_DURATION),
            ("emit (waiting for the consumer)", metric::EMIT_DURATION),
            ("decode, per chunk", metric::DECODE_CHUNK_DURATION),
            ("decode, per block", metric::DECODE_BLOCK_DURATION),
        ]
        .into_iter()
        .map(|(stage, name)| {
            let (runs, total) = RECORDED.summary(name);
            vec![
                stage.to_owned(),
                group(runs),
                format!("{total:.2}"),
                format!("{:.2}", total * 1000.0 / runs.max(1) as f64),
                format!("{:.2}", total / wall_secs),
            ]
        })
        .collect(),
    );

    println!("\nRESOURCES\n");
    let mut rows = vec![vec![
        "CPU".to_owned(),
        match cpu {
            Some(cpu) => format!(
                "{cpu:.1} s, {:.2} of {} cores busy",
                cpu / wall_secs,
                std::thread::available_parallelism().map_or(0, NonZeroUsize::get)
            ),
            None => "unknown".to_owned(),
        },
    ]];
    if let Some(threads) = decode_cpu_threads {
        let (_, busy) = RECORDED.summary(metric::DECODE_BLOCK_DURATION);
        rows.push(vec![
            "decode pool".to_owned(),
            format!(
                "{:.1}% of {threads} threads busy",
                100.0 * busy / (threads.get() as f64 * wall_secs)
            ),
        ]);
    }
    rows.push(vec![
        "peak blocks in flight".to_owned(),
        group(RECORDED.peak.load(Ordering::Relaxed)),
    ]);
    print_table(&["", ""], rows);
}

/// The process's user and system CPU time in seconds, from `/proc/self/stat`.
fn cpu_seconds() -> Option<f64> {
    let stat = fs::read_to_string("/proc/self/stat").ok()?;
    // Fields after the parenthesized command name; utime and stime are fields 14 and 15.
    let fields = stat
        .rsplit_once(')')?
        .1
        .split_whitespace()
        .collect::<Vec<_>>();
    let ticks = fields.get(11)?.parse::<f64>().ok()? + fields.get(12)?.parse::<f64>().ok()?;
    Some(ticks / USER_HZ)
}

/// Histograms as runs and total seconds, and the peak of sourced but not yet consumed blocks.
#[derive(Default)]
struct Recorded {
    histograms: Mutex<BTreeMap<String, Arc<Summary>>>,
    sourced: AtomicU64,
    consumed: AtomicU64,
    peak: AtomicU64,
    /// Transactions in the decoded blocks.
    transactions: AtomicU64,
}

impl Recorded {
    fn summary(&self, name: &str) -> (u64, f64) {
        self.histograms
            .lock()
            .unwrap()
            .get(name)
            .map(|summary| {
                (
                    summary.runs.load(Ordering::Relaxed),
                    summary.nanos.load(Ordering::Relaxed) as f64 / 1e9,
                )
            })
            .unwrap_or_default()
    }
}

#[derive(Default)]
struct Summary {
    runs: AtomicU64,
    nanos: AtomicU64,
}

impl HistogramFn for Summary {
    fn record(&self, seconds: f64) {
        self.runs.fetch_add(1, Ordering::Relaxed);
        self.nanos
            .fetch_add((seconds * 1e9) as u64, Ordering::Relaxed);
    }
}

/// Counts sourced blocks and tracks the peak in flight.
struct SourcedBlocks;

impl CounterFn for SourcedBlocks {
    fn increment(&self, blocks: u64) {
        let sourced = RECORDED.sourced.fetch_add(blocks, Ordering::Relaxed) + blocks;
        let in_flight = sourced.saturating_sub(RECORDED.consumed.load(Ordering::Relaxed));
        RECORDED.peak.fetch_max(in_flight, Ordering::Relaxed);
    }

    fn absolute(&self, _: u64) {}
}

/// Records into [RECORDED].
struct HarnessRecorder;

impl Recorder for HarnessRecorder {
    fn describe_counter(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}

    fn describe_gauge(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}

    fn describe_histogram(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}

    fn register_counter(&self, key: &Key, _: &Metadata<'_>) -> Counter {
        match key.name() {
            metric::SOURCED_BLOCK_COUNT => Counter::from_arc(Arc::new(SourcedBlocks)),
            _ => Counter::noop(),
        }
    }

    fn register_gauge(&self, _: &Key, _: &Metadata<'_>) -> Gauge {
        Gauge::noop()
    }

    fn register_histogram(&self, key: &Key, _: &Metadata<'_>) -> Histogram {
        let summary = RECORDED
            .histograms
            .lock()
            .unwrap()
            .entry(key.name().to_owned())
            .or_default()
            .clone();
        Histogram::from_arc(summary)
    }
}
