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

use chain_indexer::{
    domain::BlockRef,
    infra::subxt_node::rpc::{Call, Count, ReconnectPolicy, Transport, method},
    pipeline::{
        decode::{self, CpuPool},
        metric,
        source::{self, Block, Source, resolve},
    },
};
use futures::{StreamExt, TryStreamExt};
use metrics::{
    Counter, CounterFn, Gauge, Histogram, HistogramFn, Key, KeyName, Metadata, Recorder,
    SharedString, Unit,
};
use std::{
    collections::BTreeMap,
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
use tokio::time::sleep;

/// The time resolution of the recorded arrivals.
const ARRIVAL_RESOLUTION: Duration = Duration::from_millis(10);
/// How often the consumer prints its progress.
const PROGRESS_INTERVAL: Duration = Duration::from_secs(10);
/// Clock ticks per second of `/proc/self/stat`'s CPU times, `USER_HZ`, fixed on Linux.
const USER_HZ: f64 = 100.0;

static RECORDED: LazyLock<Recorded> = LazyLock::new(Recorded::default);

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a node at NODE_URL"]
async fn source_throughput() {
    let url = env::var("NODE_URL").expect("NODE_URL is set");
    let from = setting("SOURCE_FROM", 0u64);
    let count = optional_setting::<u64>("SOURCE_COUNT");
    let config = source::Config {
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
    println!(
        "{url} heights {from}..={end}: {config:?}, decode threads {decode_cpu_threads:?}, \
         consumer sleep {consumer_sleep:?}"
    );
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
    // Keyed by extrinsic count; genesis, under `None`, belongs to neither empty nor non-empty.
    let sizes = Mutex::new(BTreeMap::<Option<usize>, Totals>::new());
    let chunks = chunks.inspect(|chunk| {
        if let Ok(chunk) = chunk {
            let mut sizes = sizes.lock().unwrap();
            chunk.iter().for_each(|block| {
                let sizes_of = BlockSizes::of(block);
                let key = (block.height() > 0).then_some(sizes_of.extrinsic_count);
                sizes.entry(key).or_default().add(&sizes_of)
            });
        }
    });

    let mut consumer = Consumer::new(consumer_sleep, started);
    match decode_cpu_threads {
        Some(threads) => {
            let pool = Arc::new(CpuPool::new(threads).expect("pool builds"));
            decode::decode(chunks, pool, config.chunk_size)
                .for_each(|block| consumer.receive(block.map(|_| 1)))
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

    report(
        &source,
        &consumer,
        &sizes.into_inner().unwrap(),
        wall,
        cpu,
        decode_cpu_threads,
    );
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
    /// The last progress line: when, and the blocks received by then.
    progress: (Duration, u64),
}

impl Consumer {
    fn new(sleep: Duration, started: Instant) -> Self {
        Self {
            sleep,
            started,
            blocks: 0,
            errors: vec![],
            arrivals: vec![],
            progress: (Duration::ZERO, 0),
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
                let (last_at, last_blocks) = self.progress;
                if now - last_at >= PROGRESS_INTERVAL {
                    println!(
                        "{:.0} s: {} blocks, {:.0} blocks/s",
                        now.as_secs_f64(),
                        self.blocks,
                        (self.blocks - last_blocks) as f64 / (now - last_at).as_secs_f64()
                    );
                    self.progress = (now, self.blocks);
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
    sizes: &BTreeMap<Option<usize>, Totals>,
    wall: Duration,
    cpu: Option<f64>,
    decode_cpu_threads: Option<NonZeroUsize>,
) {
    let blocks = consumer.blocks.max(1) as f64;
    let wall_secs = wall.as_secs_f64();

    println!("\n## Throughput\n");
    println!(
        "{} blocks in {wall_secs:.2} s: {:.1} blocks/s, {} errors",
        consumer.blocks,
        consumer.blocks as f64 / wall_secs,
        consumer.errors.len()
    );
    println!(
        "\nBlocks/s in tenths of the run: {}",
        consumer
            .slices(wall, 10)
            .iter()
            .map(|blocks| format!("{:.0}", *blocks as f64 * 10.0 / wall_secs))
            .collect::<Vec<_>>()
            .join(", ")
    );

    println!("\n## Requests, by method, runtime function or storage item\n");
    println!(
        "Bytes are JSON wire bytes, except for storage items (decoded values); a storage \
         subscription's notifications are counted under `archive_v1_storage`.\n"
    );
    println!(
        "| key | requests | per block | request bytes | response bytes | response bytes per block \
         |\n|---|---:|---:|---:|---:|---:|"
    );
    source.counters().counts().iter().for_each(|(key, count)| {
        let Count {
            requests,
            request_bytes,
            response_bytes,
        } = count;
        println!(
            "| {key} | {requests} | {:.3} | {request_bytes} | {response_bytes} | {:.0} |",
            *requests as f64 / blocks,
            *response_bytes as f64 / blocks
        );
    });
    let batches = source.counters().batches();
    println!(
        "\n{} batches, largest response {} bytes",
        batches.batches, batches.largest_response_bytes
    );

    // Blocks with the run's fewest extrinsics carry nothing but inherents.
    let fewest = sizes.keys().flatten().next().copied().unwrap_or(0);
    let empty = sizes.get(&Some(fewest)).copied().unwrap_or_default();
    let non_empty = sizes
        .range(Some(fewest + 1)..)
        .fold(Totals::default(), |totals, (_, other)| totals.merge(other));
    let all = sizes
        .values()
        .fold(Totals::default(), |totals, other| totals.merge(other));
    println!("\n## Decoded bytes per block, by data type\n");
    println!(
        "Empty blocks have the run's fewest extrinsics ({fewest}): {} empty, {} non-empty.\n",
        empty.blocks, non_empty.blocks
    );
    println!("| type | all | empty | non-empty |\n|---|---:|---:|---:|");
    BlockSizes::default()
        .fields()
        .iter()
        .enumerate()
        .for_each(|(field, (name, _))| {
            println!(
                "| {name} | {:.1} | {:.1} | {:.1} |",
                all.mean(field),
                empty.mean(field),
                non_empty.mean(field)
            );
        });
    println!(
        "| body per extrinsic | {:.1} | | |",
        all.fields[1] as f64 / all.extrinsics.max(1) as f64
    );

    println!("\n## Stages\n");
    println!(
        "Busy share is the stage's total time over wall time; above 1 means it runs concurrently.\n"
    );
    println!("| stage | runs | total s | mean ms | busy share |\n|---|---:|---:|---:|---:|");
    [
        ("resolve", metric::RESOLVE_DURATION),
        ("source", metric::SOURCE_DURATION),
        ("verify", metric::VERIFY_DURATION),
        ("emit (waiting for the consumer)", metric::EMIT_DURATION),
        ("decode, per chunk", metric::DECODE_CHUNK_DURATION),
        ("decode, per block", metric::DECODE_BLOCK_DURATION),
    ]
    .into_iter()
    .for_each(|(stage, name)| {
        let (runs, total) = RECORDED.summary(name);
        println!(
            "| {stage} | {runs} | {total:.2} | {:.2} | {:.2} |",
            total * 1000.0 / runs.max(1) as f64,
            total / wall_secs
        );
    });

    println!("\n## Resources\n");
    match cpu {
        Some(cpu) => println!(
            "CPU: {cpu:.2} s, {:.2} cores busy of {}",
            cpu / wall_secs,
            std::thread::available_parallelism().map_or(0, NonZeroUsize::get)
        ),
        None => println!("CPU: unknown"),
    }
    if let Some(threads) = decode_cpu_threads {
        let (_, busy) = RECORDED.summary(metric::DECODE_BLOCK_DURATION);
        println!(
            "Decode pool utilization: {:.1}% of {threads} threads",
            100.0 * busy / (threads.get() as f64 * wall_secs)
        );
    }
    println!(
        "Peak blocks in flight: {}",
        RECORDED.peak.load(Ordering::Relaxed)
    );
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
