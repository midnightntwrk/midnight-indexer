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

//! Benchmarks for chain-indexer ledger state operations.
//!
//! These cover the block-processing code paths that don't require
//! pre-existing transactions signed for a specific prior state:
//!
//! - `LedgerState::new` — bootstrap cost when the indexer starts from a network without genesis
//!   state
//! - `apply_transactions` on an empty batch — exercises `finalize_apply_transactions` (merkle tree
//!   updates, ledger parameter calculation) which runs once per block regardless of transaction
//!   count
//!
//! Applying real signed transactions requires matching ledger state; that is a
//! follow-up once we can capture a realistic state + tx fixture pair.
//!
//! Run with `just bench` or
//! `cargo bench --features standalone -p chain-indexer`.

use chain_indexer::domain::LedgerState;
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use indexer_common::{
    domain::{BlockHash, LedgerVersion, NetworkId},
    infra::ledger_db,
};
use tokio::runtime::Runtime;

fn init_ledger_db(rt: &Runtime) -> tempfile::TempDir {
    let temp_dir = tempfile::tempdir().expect("create tempdir");
    let sqlite_file = temp_dir
        .path()
        .join("ledger-db.sqlite")
        .display()
        .to_string();

    rt.block_on(async {
        ledger_db::init(ledger_db::Config {
            cache_size: 1_024,
            cnn_url: sqlite_file,
        })
        .await
        .expect("init ledger_db");
    });

    temp_dir
}

fn bench_ledger_state_new(c: &mut Criterion) {
    let rt = Runtime::new().expect("tokio runtime");
    let _temp_dir = init_ledger_db(&rt);

    let network_id: NetworkId = "undeployed".try_into().expect("network id");

    c.bench_function("LedgerState::new (undeployed, V8)", |b| {
        b.iter(|| LedgerState::new(black_box(network_id.clone()), LedgerVersion::V8).expect("new"))
    });
}

fn bench_apply_transactions_empty(c: &mut Criterion) {
    let rt = Runtime::new().expect("tokio runtime");
    let _temp_dir = init_ledger_db(&rt);

    let network_id: NetworkId = "undeployed".try_into().expect("network id");
    let parent_block_hash = BlockHash::from([0u8; 32]);
    let block_timestamp: u64 = 1_700_000_000_000;
    let parent_block_timestamp: u64 = block_timestamp - 6_000;

    c.bench_function("LedgerState::apply_transactions (empty batch)", |b| {
        b.iter_batched(
            || LedgerState::new(network_id.clone(), LedgerVersion::V8).expect("new"),
            |mut ledger_state| {
                ledger_state
                    .apply_transactions(
                        std::iter::empty(),
                        black_box(parent_block_hash),
                        black_box(block_timestamp),
                        black_box(parent_block_timestamp),
                    )
                    .expect("apply_transactions")
            },
            criterion::BatchSize::SmallInput,
        )
    });
}

criterion_group!(
    benches,
    bench_ledger_state_new,
    bench_apply_transactions_empty
);
criterion_main!(benches);
