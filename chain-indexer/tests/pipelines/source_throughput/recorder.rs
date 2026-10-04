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

//! A metrics recorder keeping the pipeline's stage times and block arrivals.

use chain_indexer::pipeline::metric;
use metrics::{
    Counter, CounterFn, Gauge, Histogram, HistogramFn, Key, KeyName, Metadata, Recorder,
    SharedString, Unit,
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

pub(super) static RECORDED: LazyLock<Recorded> = LazyLock::new(Recorded::default);

/// Histograms as runs and total seconds, and the peak of sourced but not yet consumed blocks.
#[derive(Default)]
pub(super) struct Recorded {
    histograms: Mutex<BTreeMap<String, Arc<Summary>>>,
    pub(super) sourced: AtomicU64,
    pub(super) consumed: AtomicU64,
    pub(super) peak: AtomicU64,
    /// Transactions in the decoded blocks.
    pub(super) transactions: AtomicU64,
}

impl Recorded {
    pub(super) fn summary(&self, name: &str) -> (u64, f64) {
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
    pub(super) runs: AtomicU64,
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
pub(super) struct HarnessRecorder;

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
