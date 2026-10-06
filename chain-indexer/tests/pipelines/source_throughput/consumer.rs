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

//! The consumer of the pipeline's blocks, and its tallies of them.

use super::recorder::RECORDED;
use chain_indexer::pipeline::{
    decode,
    sourcing::{Block, metadata_spec_version},
};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::time::sleep;

/// The time resolution of the recorded arrivals.
const ARRIVAL_RESOLUTION: Duration = Duration::from_millis(10);

/// Receives blocks, sleeps per block, and records when blocks arrive.
pub(super) struct Consumer {
    pub(super) sleep: Duration,
    pub(super) started: Instant,
    pub(super) blocks: u64,
    pub(super) errors: Vec<String>,
    /// The blocks received so far, at most one entry per [ARRIVAL_RESOLUTION].
    pub(super) arrivals: Vec<(Duration, u64)>,
}

impl Consumer {
    pub(super) fn new(sleep: Duration, started: Instant) -> Self {
        Self {
            sleep,
            started,
            blocks: 0,
            errors: vec![],
            arrivals: vec![],
        }
    }

    /// The blocks received in each of `n` equal slices of `wall`.
    pub(super) fn slices(&self, wall: Duration, n: u32) -> Vec<u64> {
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

    pub(super) fn receive(
        &mut self,
        blocks: Result<u64, decode::Error>,
    ) -> impl Future<Output = ()> + use<> {
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
pub(super) struct Sourced {
    pub(super) sizes: Mutex<BTreeMap<Option<usize>, Totals>>,
    pub(super) runtimes: Mutex<(HashMap<usize, u32>, BTreeMap<u32, u64>)>,
    pub(super) latest_runtime: AtomicU64,
}

impl Sourced {
    pub(super) fn add(&self, chunk: &[Block]) {
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

    pub(super) fn totals(&self) -> Totals {
        self.sizes
            .lock()
            .unwrap()
            .values()
            .fold(Totals::default(), |totals, other| totals.merge(other))
    }
}

/// Decoded bytes of one sourced block, by data type.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct BlockSizes {
    extrinsic_count: usize,
    pub(super) header: usize,
    pub(super) body: usize,
    pub(super) events: usize,
    authorities: usize,
    zswap_state_root: usize,
    ledger_state_root: usize,
    system_parameters: usize,
    pub(super) genesis: usize,
}

impl BlockSizes {
    pub(super) fn of(block: &Block) -> Self {
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

    pub(super) fn fields(&self) -> [(&'static str, usize); 8] {
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
pub(super) struct Totals {
    pub(super) blocks: u64,
    pub(super) extrinsics: usize,
    pub(super) fields: [usize; 8],
}

impl Totals {
    pub(super) fn add(&mut self, block: &BlockSizes) {
        self.blocks += 1;
        self.extrinsics += block.extrinsic_count;
        self.fields
            .iter_mut()
            .zip(block.fields())
            .for_each(|(total, (_, bytes))| *total += bytes);
    }

    pub(super) fn merge(mut self, other: &Self) -> Self {
        self.blocks += other.blocks;
        self.extrinsics += other.extrinsics;
        self.fields
            .iter_mut()
            .zip(other.fields)
            .for_each(|(total, bytes)| *total += bytes);
        self
    }

    pub(super) fn mean(&self, field: usize) -> f64 {
        self.fields[field] as f64 / self.blocks.max(1) as f64
    }
}
