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
use tikv_jemalloc_ctl::{epoch, epoch_mib, stats};

/// How often the allocator statistics are refreshed.
pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(5);

/// The published jemalloc `stats.*` names, the values of the gauge's `stat` label.
pub const STATS: [&str; 6] = [
    "allocated",
    "active",
    "metadata",
    "resident",
    "mapped",
    "retained",
];

/// Handles to jemalloc's statistics and the gauges they are published to.
pub struct AllocatorMetrics {
    epoch: epoch_mib,
    allocated: stats::allocated_mib,
    active: stats::active_mib,
    metadata: stats::metadata_mib,
    resident: stats::resident_mib,
    mapped: stats::mapped_mib,
    retained: stats::retained_mib,
    gauges: [Gauge; 6],
}

impl AllocatorMetrics {
    /// Looks up jemalloc's statistics and registers the gauge with the global recorder.
    pub fn new() -> Result<Self, tikv_jemalloc_ctl::Error> {
        describe_gauge!(
            "indexer_memory_allocator_bytes",
            Unit::Bytes,
            "jemalloc memory statistics in bytes, by jemalloc `stats.*` name"
        );

        Ok(Self {
            epoch: epoch::mib()?,
            allocated: stats::allocated::mib()?,
            active: stats::active::mib()?,
            metadata: stats::metadata::mib()?,
            resident: stats::resident::mib()?,
            mapped: stats::mapped::mib()?,
            retained: stats::retained::mib()?,
            gauges: STATS.map(|stat| gauge!("indexer_memory_allocator_bytes", "stat" => stat)),
        })
    }

    /// Reads the current statistics, in the order of [STATS].
    fn read(&self) -> Result<[usize; 6], tikv_jemalloc_ctl::Error> {
        // jemalloc caches its statistics until the epoch is advanced.
        self.epoch.advance()?;
        Ok([
            self.allocated.read()?,
            self.active.read()?,
            self.metadata.read()?,
            self.resident.read()?,
            self.mapped.read()?,
            self.retained.read()?,
        ])
    }

    fn update(&self) -> Result<(), tikv_jemalloc_ctl::Error> {
        self.read()?
            .into_iter()
            .zip(&self.gauges)
            .for_each(|(value, gauge)| gauge.set(value as f64));
        Ok(())
    }

    /// Refreshes the gauges every `interval`.
    pub async fn run(self, interval: Duration) {
        loop {
            if let Err(error) = self.update() {
                log::debug!(error:%; "cannot read allocator statistics");
            }
            tokio::time::sleep(interval).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_every_stat() {
        let metrics = AllocatorMetrics::new().unwrap();
        // Keep a live allocation around so `allocated` cannot be zero.
        let live = vec![0u8; 1 << 20];
        let [allocated, active, _, resident, mapped, _] = metrics.read().unwrap();

        assert!(allocated >= live.len());
        assert!(active >= allocated);
        assert!(resident >= active);
        assert!(mapped >= active);
        metrics.update().unwrap();
    }
}
