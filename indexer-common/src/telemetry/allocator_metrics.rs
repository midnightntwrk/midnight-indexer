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
    use std::collections::HashMap;

    #[test]
    fn test_read_every_stat() {
        let metrics = AllocatorMetrics::new().unwrap();
        // Keep a live allocation around so `allocated` cannot be zero.
        let live = vec![0u8; 1 << 20];
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
}
