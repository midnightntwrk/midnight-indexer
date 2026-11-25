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

//! Heap growth of the chain-indexer's ledger state persistence.
//!
//! This test is its own binary, so its counting global allocator measures only this test.

#![cfg(feature = "standalone")]

use anyhow::Context;
use indexer_common::{
    domain::{LedgerVersion, SerializedLedgerStateKey, ledger::LedgerState},
    infra::ledger_db,
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    collections::VecDeque,
    sync::atomic::{AtomicIsize, Ordering},
    time::Duration,
};

// Heap growth allowed per measured block, in bytes. A leaked `Sp` cache entry keeps at least the
// ledger state's `Arc` allocation of over 1 KB.
const ALLOWED_GROWTH_PER_BLOCK: isize = 512;
// Milliseconds between consecutive block timestamps.
const BLOCK_TIME: u64 = 6_000;
// Blocks between gc passes, like `gc_interval`.
const GC_INTERVAL: u64 = 50;
// Persisted ledger states kept before the oldest is unpersisted, like `ledger_state_retention`.
const RETENTION: usize = 20;
// Blocks persisted before measuring, so caches with a fixed bound reach it first.
const WARM_UP_BLOCKS: u64 = 250;
// Blocks per measured window, a multiple of `GC_INTERVAL` so every window ends on a gc pass.
const WINDOW_BLOCKS: u64 = 500;
// Measured windows. A leak grows the heap in every window, while a transient allocation on another
// thread lands in one, so the test checks the smallest growth.
const WINDOWS: usize = 3;

// Bytes currently allocated through `CountingAllocator`, on every thread.
static LIVE_BYTES: AtomicIsize = AtomicIsize::new(0);

// The system allocator, counting live bytes in `LIVE_BYTES`.
struct CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            LIVE_BYTES.fetch_add(layout.size() as isize, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        LIVE_BYTES.fetch_sub(layout.size() as isize, Ordering::Relaxed);
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_pointer = unsafe { System.realloc(pointer, layout, new_size) };
        if !new_pointer.is_null() {
            LIVE_BYTES.fetch_add(
                new_size as isize - layout.size() as isize,
                Ordering::Relaxed,
            );
        }
        new_pointer
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

// The chain-indexer persists every block's ledger state, unpersists the oldest beyond its
// retention window and runs gc on an interval. Anything this loop keeps alive grows with the
// chain, so after a warm-up, at least one measured window must grow the heap by less than a small
// allowance per block.
#[tokio::test(flavor = "multi_thread")]
async fn persisting_ledger_states_does_not_grow_the_heap() -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir().context("create tempdir")?;
    let cnn_url = temp_dir
        .path()
        .join("ledger-db.sqlite")
        .display()
        .to_string();
    ledger_db::init(ledger_db::Config {
        cache_max_nodes: 1_024,
        cnn_url,
    })
    .await
    .context("init ledger DB")?;

    for ledger_version in [LedgerVersion::V8, LedgerVersion::V9] {
        let mut ledger_state = LedgerState::new("undeployed".try_into()?, ledger_version)
            .context("create ledger state")?;
        let mut window = VecDeque::new();
        let mut block = 0;

        for _ in 0..WARM_UP_BLOCKS {
            ledger_state = index_next_block(ledger_state, &mut window, &mut block)?;
        }

        let mut growths = Vec::with_capacity(WINDOWS);
        for _ in 0..WINDOWS {
            let before = LIVE_BYTES.load(Ordering::Relaxed);
            for _ in 0..WINDOW_BLOCKS {
                ledger_state = index_next_block(ledger_state, &mut window, &mut block)?;
            }
            growths.push(LIVE_BYTES.load(Ordering::Relaxed) - before);
        }

        let allowed = ALLOWED_GROWTH_PER_BLOCK * WINDOW_BLOCKS as isize;
        assert!(
            growths.iter().any(|growth| *growth < allowed),
            "{ledger_version:?}: every window of {WINDOW_BLOCKS} blocks grew the heap by at least \
             {allowed} bytes: {growths:?}"
        );
    }

    Ok(())
}

// Applies the per-block update for the next block, which makes the state distinct, persists it
// into `window`, unpersists the oldest state beyond `RETENTION` and runs gc every `GC_INTERVAL`
// blocks.
fn index_next_block(
    mut ledger_state: LedgerState,
    window: &mut VecDeque<SerializedLedgerStateKey>,
    block: &mut u64,
) -> anyhow::Result<LedgerState> {
    *block += 1;
    ledger_state
        .finalize_apply_transactions(*block * BLOCK_TIME)
        .context("finalize block")?;
    let ledger_version = ledger_state.ledger_version();
    let (ledger_state, key) = ledger_state.persist().context("persist ledger state")?;

    window.push_back(key);
    if window.len() > RETENTION
        && let Some(key) = window.pop_front()
    {
        LedgerState::unpersist(&key, ledger_version).context("unpersist ledger state")?;
    }
    if block.is_multiple_of(GC_INTERVAL) {
        LedgerState::gc(Duration::from_secs(5));
    }

    Ok(ledger_state)
}
