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

use super::Storage;
use crate::domain::{Transaction, storage::transaction::TransactionStorage};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit};
use indexer_common::infra::{
    migrations,
    pool::sqlite::{self, SqlitePool},
};
use indoc::indoc;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

async fn storage() -> anyhow::Result<Storage> {
    // One connection keeps the in-memory database and progress handler on the same handle.
    let pool = SqlitePool::new(sqlite::Config {
        max_connections: 1,
        ..Default::default()
    })
    .await?;
    migrations::sqlite::run(&pool).await?;

    sqlx::raw_sql(indoc! {"
        INSERT INTO blocks (
            id, hash, height, protocol_version, parent_hash, timestamp,
            zswap_merkle_tree_root, ledger_parameters, ledger_state_key
        ) VALUES (1, zeroblob(32), 1, 2001000, zeroblob(32), 0, X'', X'', X'');

        WITH RECURSIVE ids(id) AS (
            SELECT 1
            UNION ALL
            SELECT id + 1 FROM ids WHERE id < 10000
        )
        INSERT INTO transactions (id, block_id, variant, hash, protocol_version, raw)
        SELECT id, 1, 'Regular', zeroblob(32), 2001000, X'' FROM ids;

        INSERT INTO regular_transactions (
            id, transaction_result, zswap_merkle_tree_root,
            zswap_start_index, zswap_end_index,
            dust_commitment_start_index, dust_commitment_end_index,
            dust_generation_start_index, dust_generation_end_index
        ) SELECT id, '\"Success\"', X'', 0, 0, 0, 0, 0, 0 FROM transactions;

        INSERT INTO transaction_identifiers (transaction_id, identifier)
        SELECT id, zeroblob(32) FROM transactions;
    "})
    .execute(&*pool)
    .await?;

    // Insertion order differs from transaction order; one mapping is duplicated.
    for id in [9000_i64, 2, 2] {
        sqlx::query(indoc! {"
            INSERT INTO transaction_identifiers (transaction_id, identifier)
            VALUES ($1, $2)
        "})
        .bind(id)
        .bind(vec![1_u8; 32])
        .execute(&*pool)
        .await?;
    }

    Ok(Storage::new(ChaCha20Poly1305::new(&[0; 32].into()), pool))
}

#[tokio::test]
async fn identifier_lookup_returns_each_match_once_in_transaction_order() -> anyhow::Result<()> {
    let storage = storage().await?;
    let transactions = storage
        .get_transactions_by_identifier(&vec![1; 32].into())
        .await?;

    assert_eq!(
        transactions.iter().map(Transaction::id).collect::<Vec<_>>(),
        [2, 9000]
    );
    Ok(())
}

#[tokio::test]
async fn identifier_lookup_returns_no_matches_for_unknown_identifier() -> anyhow::Result<()> {
    let storage = storage().await?;
    let transactions = storage
        .get_transactions_by_identifier(&vec![2; 32].into())
        .await?;

    assert!(transactions.is_empty());
    Ok(())
}

#[tokio::test]
async fn identifier_lookup_does_not_scan_unrelated_transactions() -> anyhow::Result<()> {
    let storage = storage().await?;
    let steps = Arc::new(AtomicUsize::new(0));
    let counter = steps.clone();
    let mut connection = storage.pool.acquire().await?;
    connection
        .lock_handle()
        .await?
        .set_progress_handler(1000, move || {
            counter.fetch_add(1000, Ordering::Relaxed);
            true
        });
    drop(connection);

    // Count actual SQLite work rather than elapsed time or version-specific plan text.
    // Indexed lookups need far fewer steps; scanning the 10,000-row table exceeds this.
    for identifier in [vec![1; 32], vec![2; 32]] {
        steps.store(0, Ordering::Relaxed);
        storage
            .get_transactions_by_identifier(&identifier.into())
            .await?;

        let executed = steps.load(Ordering::Relaxed);
        assert!(
            executed < 10_000,
            "identifier lookup scanned unrelated transactions ({executed} VM steps)"
        );
    }
    Ok(())
}
