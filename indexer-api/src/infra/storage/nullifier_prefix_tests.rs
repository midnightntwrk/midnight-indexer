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
use crate::domain::storage::{
    dust_generations::DustGenerationsStorage, shielded_nullifiers::ShieldedNullifiersStorage,
};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit};
use futures::TryStreamExt;
use indexer_common::infra::migrations;
use indoc::indoc;
use std::num::NonZeroU32;

#[cfg(feature = "standalone")]
async fn storage() -> (impl Sized, Storage) {
    use indexer_common::infra::pool::sqlite::{self, SqlitePool};

    let dir = tempfile::tempdir().expect("create tempdir");
    let url = dir.path().join("indexer.sqlite").display().to_string();
    let pool = SqlitePool::new(sqlite::Config::with_url(url))
        .await
        .expect("create pool");
    migrations::sqlite::run(&pool)
        .await
        .expect("run migrations");
    let storage = Storage::new(ChaCha20Poly1305::new(&[0; 32].into()), pool);
    (dir, storage)
}

#[cfg(feature = "cloud")]
async fn storage() -> (impl Sized, Storage) {
    use indexer_common::infra::pool::postgres::{self, PostgresPool};
    use sqlx::postgres::PgSslMode;
    use std::time::Duration;
    use testcontainers::{ImageExt, runners::AsyncRunner};
    use testcontainers_modules::postgres::Postgres;

    let container = Postgres::default()
        .with_tag("17.1-alpine")
        .start()
        .await
        .expect("start Postgres");
    let pool = PostgresPool::new(postgres::Config {
        host: container
            .get_host()
            .await
            .expect("Postgres host")
            .to_string(),
        port: container
            .get_host_port_ipv4(5432)
            .await
            .expect("Postgres port"),
        dbname: "postgres".into(),
        user: "postgres".into(),
        password: "postgres".into(),
        sslmode: PgSslMode::Prefer,
        max_connections: 2,
        idle_timeout: Duration::from_secs(60),
        max_lifetime: Duration::from_secs(300),
    })
    .await
    .expect("create pool");
    migrations::postgres::run(&pool)
        .await
        .expect("run migrations");
    let storage = Storage::new(ChaCha20Poly1305::new(&[0; 32].into()), pool);
    (container, storage)
}

// Nullifiers are deliberately not inserted in lexicographic order: pagination uses row IDs.
// The matching spends at heights 1 and 5 must be excluded by the inclusive 2..=4 window.
const SPENDS: &[(u32, &str)] = &[
    (1, "ff9d01"),
    (2, "12ff00"),
    (
        2,
        "ff9dd13db8d5c07a94967dd3a45d2e6ee66d1fe8f8c08438d8715b39750faf6e",
    ),
    (2, "12feff"),
    (
        3,
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    ),
    (3, "130000"),
    (3, "12ffff"),
    (4, "ff0000"),
    (5, "ff9d02"),
    (4, "ff9e00"),
    (4, "000000"),
];

fn nullifier(hex: &str) -> Vec<u8> {
    let mut bytes = const_hex::decode(hex).expect("fixture nullifier");
    bytes.resize(32, 0);
    bytes
}

async fn insert_spends(storage: &Storage) {
    let pool = &*storage.pool;
    for height in 1..=5i64 {
        sqlx::query(indoc! {"
            INSERT INTO blocks (
                id, hash, height, protocol_version, parent_hash, timestamp,
                zswap_merkle_tree_root, ledger_parameters, ledger_state_key
            ) VALUES ($1, $2, $1, 2001000, $2, 0, $2, $2, $2)
        "})
        .bind(height)
        .bind(vec![height as u8; 32])
        .execute(pool)
        .await
        .expect("insert block");
    }

    for (index, &(height, hex)) in SPENDS.iter().enumerate() {
        let id = index as i64 + 1;
        sqlx::query(indoc! {"
            INSERT INTO transactions (id, block_id, variant, hash, protocol_version, raw)
            VALUES ($1, $2, 'Regular', $3, 2001000, $3)
        "})
        .bind(id)
        .bind(height as i64)
        .bind(vec![id as u8; 32])
        .execute(pool)
        .await
        .expect("insert transaction");

        sqlx::query(indoc! {"
            INSERT INTO dust_nullifiers (id, nullifier, commitment, transaction_id, block_id)
            VALUES ($1, $2, $3, $1, $4)
        "})
        .bind(id)
        .bind(nullifier(hex))
        .bind(vec![id as u8; 32])
        .bind(height as i64)
        .execute(pool)
        .await
        .expect("insert DUST nullifier");

        sqlx::query(indoc! {"
            INSERT INTO zswap_nullifiers (id, nullifier, transaction_id, block_id)
            VALUES ($1, $2, $1, $3)
        "})
        .bind(id)
        .bind(nullifier(hex))
        .bind(height as i64)
        .execute(pool)
        .await
        .expect("insert shielded nullifier");
    }
}

async fn check_prefixes(dust: bool) {
    let (_database, storage) = storage().await;
    insert_spends(&storage).await;

    let cases: &[(&[&str], &[u64])] = &[
        (&["12"], &[2, 4, 7]),
        (&["12ff"], &[2, 7]),
        (&["12ffff"], &[7]),
        (&["ff"], &[3, 5, 8, 10]),
        (&["ffff"], &[5]),
        (&["ff9d"], &[3]),
        (
            &["ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"],
            &[5],
        ),
        (&["00"], &[11]),
        (&["ee"], &[]),
        (&["12ff", "ff"], &[2, 7, 3, 5, 8, 10]),
        (&[], &[]),
    ];
    for &(prefixes, expected_ids) in cases {
        let bytes = prefixes
            .iter()
            .map(|prefix| const_hex::decode(prefix).expect("fixture prefix"))
            .collect::<Vec<_>>();
        for batch_size in [1, 2, 64] {
            let batch_size = NonZeroU32::new(batch_size).unwrap();
            let rows: Vec<_> = if dust {
                storage
                    .get_dust_nullifier_transactions(&bytes, 2, 4, batch_size)
                    .await
                    .map_ok(|row| {
                        assert_eq!(
                            row.commitment_le_bytes.0,
                            vec![row.transaction_id as u8; 32]
                        );
                        (
                            row.transaction_id,
                            row.nullifier_le_bytes.0,
                            row.block_height,
                        )
                    })
                    .try_collect()
                    .await
                    .expect("query DUST nullifiers")
            } else {
                storage
                    .get_shielded_nullifier_transactions(&bytes, 2, 4, batch_size)
                    .await
                    .map_ok(|row| (row.transaction_id, row.nullifier.0, row.block_height))
                    .try_collect()
                    .await
                    .expect("query shielded nullifiers")
            };
            let expected: Vec<_> = expected_ids
                .iter()
                .map(|&id| {
                    let (height, hex) = SPENDS[id as usize - 1];
                    (id, nullifier(hex), height)
                })
                .collect();
            assert_eq!(
                rows, expected,
                "prefixes {prefixes:?}, batch size {batch_size}"
            );
        }
    }
}

#[tokio::test]
async fn dust_prefix_lookup_handles_trailing_ff_with_pagination_and_block_bounds() {
    check_prefixes(true).await;
}

#[tokio::test]
async fn shielded_prefix_lookup_handles_trailing_ff_with_pagination_and_block_bounds() {
    check_prefixes(false).await;
}
