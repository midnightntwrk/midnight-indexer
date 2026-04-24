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

//! Benchmark the chain-indexer transaction application path with a real
//! transaction applied on top of its matching genesis ledger state.
//!
//! Both fixtures (`genesis_state.raw`, `tx_1_2_2.raw`) are produced together by
//! `generate_txs.sh` / `just generate-txs`, which spins up `CFG_PRESET=dev` and
//! captures the genesis `system_properties.genesis_state` RPC response
//! alongside the generated transactions. Regenerate all of them together after
//! any ledger/node version change.
//!
//! Run with `cargo bench --features standalone -p chain-indexer --bench apply_transaction`.

use chain_indexer::domain::{LedgerState, node};
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use indexer_common::{
    domain::{
        BlockHash, ByteVec, LedgerVersion, ProtocolVersion, TransactionHash,
        ledger::Transaction as LedgerTransaction,
    },
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

fn make_regular_tx(raw_bytes: Vec<u8>) -> node::RegularTransaction {
    let raw: ByteVec = raw_bytes.into();

    let deserialized = LedgerTransaction::deserialize(&raw, LedgerVersion::V8)
        .expect("deserialize fixture transaction");
    let hash: TransactionHash = deserialized.hash();
    let identifiers = deserialized.identifiers().expect("identifiers");

    node::RegularTransaction {
        hash,
        protocol_version: ProtocolVersion::V1_0(1_000_000),
        raw,
        identifiers,
        contract_actions: vec![],
    }
}

fn bench_apply_real_tx(c: &mut Criterion) {
    let rt = Runtime::new().expect("tokio runtime");
    let _temp_dir = init_ledger_db(&rt);

    let genesis_bytes = fs::read(format!(
        "{}/../indexer-common/tests/genesis_state.raw",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("read genesis_state.raw");

    let tx_bytes = fs::read(format!(
        "{}/../indexer-common/tests/tx_1_2_2.raw",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("read tx_1_2_2.raw");

    let parent_block_hash = BlockHash::from([0u8; 32]);
    // Chosen to sit inside the committed fixture's intent TTL window.
    // If you regenerate the fixtures, adjust these to match.
    let block_timestamp: u64 = 1_777_040_000_000;
    let parent_block_timestamp: u64 = block_timestamp - 6_000;

    c.bench_function(
        "LedgerState::apply_transactions (tx_1_2_2 on genesis)",
        |b| {
            b.iter_batched(
                || {
                    let state = LedgerState::from_genesis(&genesis_bytes, LedgerVersion::V8)
                        .expect("from_genesis");
                    let tx = make_regular_tx(tx_bytes.clone());
                    (state, tx)
                },
                |(mut state, tx)| {
                    state
                        .apply_transactions(
                            [node::Transaction::Regular(black_box(tx))],
                            black_box(parent_block_hash),
                            black_box(block_timestamp),
                            black_box(parent_block_timestamp),
                        )
                        .expect("apply")
                },
                criterion::BatchSize::SmallInput,
            )
        },
    );
}

criterion_group!(benches, bench_apply_real_tx);
criterion_main!(benches);
