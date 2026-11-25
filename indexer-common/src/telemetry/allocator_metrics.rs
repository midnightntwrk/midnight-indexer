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

//! Publishes jemalloc's memory statistics as the `indexer_memory_allocator_bytes` gauge.

use metrics::{Gauge, Unit, describe_gauge, gauge};
use std::time::Duration;
use tikv_jemalloc_ctl::{Error, epoch, epoch_mib, stats};

/// How often the allocator statistics are refreshed.
pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(5);

/// One published jemalloc statistic.
struct Stat {
    gauge: Gauge,
    read: Box<dyn Fn() -> Result<usize, Error> + Send>,
}

/// Defines [STATS] and `stats` from one list of jemalloc `stats.*` names. Label and reader
/// of each statistic come from the same identifier, so a series cannot be published under another
/// statistic's name.
macro_rules! define_stats {
    ($($name:ident),+ $(,)?) => {
        /// The published jemalloc `stats.*` names, the values of the gauge's `stat` label.
        pub const STATS: &[&str] = &[$(stringify!($name)),+];

        /// Looks up the statistics and registers their gauges, in the order of [STATS].
        fn stats() -> Result<[Stat; STATS.len()], Error> {
            Ok([$({
                let mib = stats::$name::mib()?;
                Stat {
                    gauge: gauge!("indexer_memory_allocator_bytes", "stat" => stringify!($name)),
                    read: Box::new(move || mib.read()),
                }
            }),+])
        }
    };
}

define_stats![allocated, active, metadata, resident, mapped, retained];

/// Handles to jemalloc's statistics and the gauges they are published to.
pub struct AllocatorMetrics {
    epoch: epoch_mib,
    stats: [Stat; STATS.len()],
}

impl AllocatorMetrics {
    /// Looks up jemalloc's statistics and registers the gauge with the global recorder.
    pub fn new() -> Result<Self, Error> {
        describe_gauge!(
            "indexer_memory_allocator_bytes",
            Unit::Bytes,
            "jemalloc memory statistics in bytes, by jemalloc `stats.*` name"
        );

        Ok(Self {
            epoch: epoch::mib()?,
            stats: stats()?,
        })
    }

    /// Reads the current statistics, in the order of [STATS].
    fn read(&self) -> Result<Vec<usize>, Error> {
        // jemalloc caches its statistics until the epoch is advanced.
        self.epoch.advance()?;
        self.stats.iter().map(|stat| (stat.read)()).collect()
    }

    fn update(&self) -> Result<(), Error> {
        self.read()?
            .into_iter()
            .zip(&self.stats)
            .for_each(|(value, stat)| stat.gauge.set(value as f64));
        Ok(())
    }

    /// Refreshes the gauges every `interval`.
    ///
    /// While the statistics cannot be read the gauges keep their last values. The first failure
    /// is logged as a warning, repeated ones at debug level, and the recovery at info level.
    pub async fn run(self, interval: Duration) {
        let mut failing = false;

        loop {
            match self.update() {
                Ok(()) if failing => {
                    log::info!("reading allocator statistics again, gauges are current");
                    failing = false;
                }
                Ok(()) => {}
                Err(error) if failing => {
                    log::debug!(error:%; "cannot read allocator statistics");
                }
                Err(error) => {
                    log::warn!(error:%; "cannot read allocator statistics, gauges are stale");
                    failing = true;
                }
            }

            tokio::time::sleep(interval).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use metrics_exporter_prometheus::PrometheusBuilder;
    use std::{
        collections::{BTreeMap, HashMap},
        hint::black_box,
    };
    use tokio::time::{sleep, timeout};

    const MIB: usize = 1 << 20;

    #[test]
    fn test_read_every_stat() {
        let metrics = AllocatorMetrics::new().unwrap();
        // Keep a live allocation around so `allocated` cannot be zero. `black_box` stops an
        // optimised build from removing the allocation, as only its length is read.
        let live = black_box(vec![0u8; 1 << 20]);
        let values = STATS
            .iter()
            .copied()
            .zip(metrics.read().unwrap())
            .collect::<HashMap<_, _>>();

        assert_eq!(values.len(), STATS.len());
        assert!(values["allocated"] >= live.len());
        assert!(values["active"] >= values["allocated"]);
        assert!(values["resident"] >= values["active"]);
        assert!(values["mapped"] >= values["active"]);
        assert!(values.contains_key("metadata"));
        assert!(values.contains_key("retained"));
        metrics.update().unwrap();
    }

    #[test]
    fn test_publish_every_stat() {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        let metrics = metrics::with_local_recorder(&recorder, AllocatorMetrics::new).unwrap();
        let live = black_box(vec![0u8; MIB]);

        metrics.update().unwrap();
        let rendered = handle.render();
        let values = published(&rendered);

        assert!(rendered.contains("# TYPE indexer_memory_allocator_bytes gauge"));
        assert_eq!(
            values.keys().map(String::as_str).collect::<Vec<_>>(),
            [
                "active",
                "allocated",
                "mapped",
                "metadata",
                "resident",
                "retained"
            ]
        );
        // Each value is published under its own name: the relations between the statistics hold
        // for the published series as well.
        assert!(values["allocated"] >= live.len() as f64);
        assert!(values["active"] >= values["allocated"]);
        assert!(values["resident"] >= values["active"]);
        assert!(values["mapped"] >= values["active"]);
    }

    #[tokio::test]
    async fn test_run_refreshes_published_stats() {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        let metrics = metrics::with_local_recorder(&recorder, AllocatorMetrics::new).unwrap();
        let allocated = || {
            published(&handle.render())
                .get("allocated")
                .copied()
                .unwrap_or_default()
        };
        let allocated_reaches = async |bytes: f64| {
            let reached = async {
                while allocated() < bytes {
                    sleep(Duration::from_millis(5)).await;
                }
            };
            timeout(Duration::from_secs(10), reached).await
        };

        let run = tokio::spawn(metrics.run(Duration::from_millis(10)));

        // The first refresh publishes the current statistics.
        allocated_reaches(1.0).await.expect("stats are published");

        // A later refresh publishes a new allocation. Half of it is asked for, as other tests in
        // the same process may free memory meanwhile.
        let before = allocated();
        let live = black_box(vec![0u8; 64 * MIB]);
        allocated_reaches(before + (live.len() / 2) as f64)
            .await
            .expect("stats are refreshed");

        run.abort();
    }

    /// The values of the `indexer_memory_allocator_bytes` series in a rendered Prometheus scrape,
    /// by `stat` label.
    fn published(rendered: &str) -> BTreeMap<String, f64> {
        rendered
            .lines()
            .filter_map(|line| line.strip_prefix("indexer_memory_allocator_bytes{stat=\""))
            .filter_map(|line| line.split_once("\"} "))
            .map(|(stat, value)| (stat.to_owned(), value.parse().unwrap()))
            .collect()
    }
}
