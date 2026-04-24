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

//! SCALE-decodes the genesis ledger state blob (regression-sensitive path per #871).

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use indexer_common::{
    domain::{LedgerVersion, ledger::LedgerState},
    infra::ledger_db,
};
use std::fs;
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

fn bench_ledger_state_from_genesis(c: &mut Criterion) {
    let rt = Runtime::new().expect("tokio runtime");
    let _temp_dir = init_ledger_db(&rt);

    let genesis_bytes = fs::read(format!(
        "{}/tests/genesis_state.raw",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("read genesis_state.raw");

    c.bench_function("LedgerState::from_genesis (dev preset, V8)", |b| {
        b.iter(|| {
            LedgerState::from_genesis(black_box(&genesis_bytes), LedgerVersion::V8)
                .expect("from_genesis")
        })
    });
}

criterion_group!(benches, bench_ledger_state_from_genesis);
criterion_main!(benches);
