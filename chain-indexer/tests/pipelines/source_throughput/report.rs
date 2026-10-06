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

//! Progress lines every interval, and the report at the end.

use super::{
    consumer::{BlockSizes, Consumer, Sourced, Totals},
    recorder::RECORDED,
};
use chain_indexer::{
    infra::subxt_node::rpc::{Count, Counters, Transport, method},
    pipeline::{metric, sourcing::Source},
};
use std::{
    fs,
    num::NonZeroUsize,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};
use tokio::time::sleep;

/// How often progress is printed, and every how many lines its column header is repeated.
const PROGRESS_INTERVAL: Duration = Duration::from_secs(10);

const PROGRESS_HEADER_EVERY: u64 = 20;

/// Clock ticks per second of `/proc/self/stat`'s CPU times, `USER_HZ`, fixed on Linux.
const USER_HZ: f64 = 100.0;

/// The counters progress lines compare between intervals.
#[derive(Default)]
struct Snapshot {
    pub(super) at: Duration,
    pub(super) blocks: u64,
    pub(super) requests: u64,
    wire_bytes: u64,
    pub(super) transactions: u64,
    decode_busy: f64,
    pub(super) sizes: Totals,
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
pub(super) async fn progress(
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
pub(super) fn print_table(headers: &[&str], rows: Vec<Vec<String>>) {
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
pub(super) fn group(n: u64) -> String {
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

pub(super) fn report(
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
pub(super) fn cpu_seconds() -> Option<f64> {
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
