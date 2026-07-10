// This file is part of midnight-indexer.
// Copyright (C) Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Storage-layer integration tests for the block-hash dust generations sync
//! (issue #1283). These exercise the three storage methods the re-scoped
//! `dustGenerations` subscription is built on, against a real PostgreSQL
//! instance seeded with hand-crafted fixtures (no node required):
//!
//! * [`LedgerStateStorage::get_ledger_state_at`] — the by-block-hash
//!   ledger-state lookup that pins the whole snapshot to one block.
//! * [`DustGenerationsStorage::get_dust_generation_entries`] — the wallet's
//!   owned generation entries within a generation-tree index range.
//! * [`DustGenerationsStorage::get_dust_generation_dtime_updates`] — the
//!   owned-entry dtime delta, bounded to `(cutoff_block_id, upper_block_id]`;
//!   this bounding is the storage foundation of the subscription's
//!   determinism guarantee (issue #1283 acceptance criterion #2).
//!
//! The cryptographic root match against `dustGenerationMerkleTreeRoot`
//! (acceptance criterion #1) is covered separately — by the unit test in
//! `indexer-common` (`dust_generation_root_from_collapsed_update_matches_tree_root`)
//! and end-to-end in `indexer-tests`.
//!
//! NOTE: cloud (PostgreSQL) only. The `get_dust_generation_dtime_updates`
//! query has a separate SQLite implementation (`json_extract`/`unhex`/`iif`
//! instead of `->>`/`decode`/`regexp_replace`), so a standalone mirror of the
//! dtime tests is still required to close the "cloud + standalone" task item.

use crate::{
    domain::storage::{dust_generations::DustGenerationsStorage, ledger_state::LedgerStateStorage},
    infra::storage::Storage,
};
use futures::TryStreamExt;
use indexer_common::{
    cipher::make_cipher,
    domain::{
        ByteArray, ByteVec, LedgerEventAttributes, ProtocolVersion, dust::DustGenerationInfo,
    },
    infra::{
        migrations,
        pool::postgres::{Config, PostgresPool},
    },
};
use secrecy::SecretString;
use sqlx::postgres::PgSslMode;
use std::{error::Error as StdError, num::NonZeroU32, time::Duration};
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;

const PROTOCOL_VERSION: i64 = 2_000_000; // ProtocolVersion::V2_0
const BATCH: NonZeroU32 = NonZeroU32::new(16).unwrap();

/// Spin up PostgreSQL, run migrations, and return the container (kept alive by
/// the caller), the raw pool for seeding fixtures, and the [`Storage`] under
/// test.
async fn setup() -> Result<
    (
        testcontainers::ContainerAsync<Postgres>,
        PostgresPool,
        Storage,
    ),
    Box<dyn StdError>,
> {
    let container = Postgres::default()
        .with_db_name("indexer")
        .with_user("indexer")
        .with_password(env!("APP__INFRA__STORAGE__PASSWORD"))
        .with_tag("17.1-alpine")
        .start()
        .await?;
    let port = container.get_host_port_ipv4(5432).await?;

    let config = Config {
        host: "localhost".to_string(),
        port,
        dbname: "indexer".to_string(),
        user: "indexer".to_string(),
        password: env!("APP__INFRA__STORAGE__PASSWORD").into(),
        sslmode: PgSslMode::Prefer,
        max_connections: 10,
        idle_timeout: Duration::from_secs(60),
        max_lifetime: Duration::from_secs(5 * 60),
    };
    let pool = PostgresPool::new(config).await?;
    migrations::postgres::run(&pool).await?;

    // 64 hex chars = 32 bytes; the cipher is unused by the methods under test
    // but is required to construct `Storage`.
    let secret: SecretString = "00".repeat(32).into();
    let storage = Storage::new(make_cipher(secret)?, pool.clone());

    Ok((container, pool, storage))
}

/// Insert a block and return its internal id.
async fn insert_block(
    pool: &PostgresPool,
    height: i64,
    hash: &[u8],
    ledger_state_key: &[u8],
) -> Result<i64, Box<dyn StdError>> {
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO blocks (
             hash, height, protocol_version, parent_hash, timestamp,
             zswap_merkle_tree_root, ledger_parameters, ledger_state_key
         )
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING id",
    )
    .bind(hash)
    .bind(height)
    .bind(PROTOCOL_VERSION)
    .bind(&b"parent"[..])
    .bind(1_700_000_000_i64 + height)
    .bind(&b"zswap-root"[..])
    .bind(&b"params"[..])
    .bind(ledger_state_key)
    .fetch_one(&**pool)
    .await?;
    Ok(id)
}

/// Insert a regular transaction into a block and return its internal id.
async fn insert_transaction(
    pool: &PostgresPool,
    block_id: i64,
    hash: &[u8],
) -> Result<i64, Box<dyn StdError>> {
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO transactions (block_id, variant, hash, protocol_version, raw)
         VALUES ($1, 'Regular'::TRANSACTION_VARIANT, $2, $3, $4)
         RETURNING id",
    )
    .bind(block_id)
    .bind(hash)
    .bind(PROTOCOL_VERSION)
    .bind(&b"raw"[..])
    .fetch_one(&**pool)
    .await?;
    Ok(id)
}

/// Insert a dust generation info row (an owned generation entry).
#[allow(clippy::too_many_arguments)]
async fn insert_generation_info(
    pool: &PostgresPool,
    owner: &[u8],
    night_utxo_hash: &[u8],
    value: u128,
    generation_index: Option<i64>,
    dtime: Option<i64>,
    transaction_id: i64,
) -> Result<(), Box<dyn StdError>> {
    sqlx::query(
        "INSERT INTO dust_generation_info (
             night_utxo_hash, value, owner, nonce, ctime, merkle_index,
             dtime, transaction_id, generation_index, backing_night, initial_value
         )
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(night_utxo_hash)
    .bind(&value.to_be_bytes()[..])
    .bind(owner)
    .bind(&[9u8; 32][..])
    .bind(100_i64)
    // Give commitment index a distinct-ish value from generation index.
    .bind(generation_index.map(|i| i + 1000).unwrap_or(0))
    .bind(dtime)
    .bind(transaction_id)
    .bind(generation_index)
    .bind(&[7u8; 32][..]) // backing_night
    .bind(&value.to_be_bytes()[..]) // initial_value
    .execute(&**pool)
    .await?;
    Ok(())
}

/// Insert a `DustGenerationDtimeUpdate` ledger event whose embedded
/// `night_utxo_hash` matches an existing `dust_generation_info` row (the join
/// key), with the given `dtime` and `tree_insertion_path` surfaced verbatim.
async fn insert_dtime_event(
    pool: &PostgresPool,
    transaction_id: i64,
    night_utxo_hash: [u8; 32],
    dtime: u64,
    tree_insertion_path: &[u8],
) -> Result<(), Box<dyn StdError>> {
    let attributes = LedgerEventAttributes::DustGenerationDtimeUpdate {
        generation_info: DustGenerationInfo {
            night_utxo_hash: ByteArray(night_utxo_hash),
            value: 1_000,
            owner: ByteVec(vec![0xff; 32]),
            nonce: ByteArray([0u8; 32]),
            ctime: 100,
            dtime,
        },
        generation_index: 0, // ignored by the query; index comes from dgi
        tree_insertion_path: ByteVec(tree_insertion_path.to_vec()),
    };
    let attributes = serde_json::to_string(&attributes)?;

    sqlx::query(
        "INSERT INTO ledger_events (transaction_id, variant, grouping, raw, attributes)
         VALUES (
             $1,
             'DustGenerationDtimeUpdate'::LEDGER_EVENT_VARIANT,
             'Dust'::LEDGER_EVENT_GROUPING,
             $2,
             $3::jsonb
         )",
    )
    .bind(transaction_id)
    .bind(&b""[..])
    .bind(attributes)
    .execute(&**pool)
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// get_ledger_state_at
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_ledger_state_at_resolves_by_hash() -> Result<(), Box<dyn StdError>> {
    let (_container, pool, storage) = setup().await?;

    let hash_a = [0xa1u8; 32];
    let key_a = b"ledger-state-key-a";
    let block_a = insert_block(&pool, 1, &hash_a, key_a).await?;
    insert_block(&pool, 2, &[0xb2u8; 32], b"ledger-state-key-b").await?;

    // Known hash resolves to that block's row.
    let (block_id, height, protocol_version, ledger_state_key) = storage
        .get_ledger_state_at(ByteArray(hash_a))
        .await?
        .expect("known block hash resolves to a row");
    assert_eq!(block_id, block_a as u64);
    assert_eq!(height, 1);
    assert_eq!(protocol_version, ProtocolVersion::V2_0(2_000_000));
    assert_eq!(ledger_state_key.0, key_a);

    // Unknown hash resolves to None (drives the resolver's "unknown block hash" client error).
    let unknown = storage.get_ledger_state_at(ByteArray([0xffu8; 32])).await?;
    assert!(unknown.is_none(), "unknown block hash must resolve to None");

    Ok(())
}

// ---------------------------------------------------------------------------
// get_dust_generation_entries
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_dust_generation_entries_orders_filters_and_skips_legacy_rows()
-> Result<(), Box<dyn StdError>> {
    let (_container, pool, storage) = setup().await?;

    let owner_a = [1u8; 32];
    let owner_b = [2u8; 32];
    let block = insert_block(&pool, 1, &[0x01u8; 32], b"key").await?;
    let tx = insert_transaction(&pool, block, &[0x11u8; 32]).await?;

    // Owner A at generation indices 0, 2, 5, 7 (deliberately out of insert order).
    insert_generation_info(&pool, &owner_a, &[10u8; 32], 100, Some(5), None, tx).await?;
    insert_generation_info(&pool, &owner_a, &[11u8; 32], 100, Some(0), None, tx).await?;
    insert_generation_info(&pool, &owner_a, &[12u8; 32], 100, Some(2), None, tx).await?;
    insert_generation_info(&pool, &owner_a, &[14u8; 32], 100, Some(7), None, tx).await?;
    // Owner B — must be excluded.
    insert_generation_info(&pool, &owner_b, &[20u8; 32], 100, Some(1), None, tx).await?;
    // Legacy row for owner A with NULL generation_index — must be skipped.
    insert_generation_info(&pool, &owner_a, &[13u8; 32], 100, None, None, tx).await?;

    // Batch size 2 forces the internal cursor loop to page multiple times.
    let batch = NonZeroU32::new(2).unwrap();
    let entries: Vec<_> = storage
        .get_dust_generation_entries(&owner_a, 0, 7, batch)
        .await
        .try_collect()
        .await?;

    let indices: Vec<u64> = entries.iter().map(|e| e.generation_mt_index).collect();
    assert_eq!(
        indices,
        vec![0, 2, 5, 7],
        "owner A entries, ordered across batch boundaries, B excluded, legacy skipped"
    );
    assert!(entries.iter().all(|e| e.owner.0 == owner_a));

    // Sub-range [2, 4] selects only index 2.
    let sub: Vec<_> = storage
        .get_dust_generation_entries(&owner_a, 2, 4, batch)
        .await
        .try_collect()
        .await?;
    assert_eq!(
        sub.iter()
            .map(|e| e.generation_mt_index)
            .collect::<Vec<_>>(),
        vec![2]
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// get_dust_generation_dtime_updates
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_dust_generation_dtime_updates_bounds_by_cutoff_and_upper_block()
-> Result<(), Box<dyn StdError>> {
    let (_container, pool, storage) = setup().await?;

    let owner = [1u8; 32];
    let other_owner = [2u8; 32];
    // Three blocks, each with one dtime update for `owner`.
    let mut block_ids = Vec::new();
    for h in 1..=3i64 {
        let block = insert_block(&pool, h, &[h as u8; 32], b"key").await?;
        let tx = insert_transaction(&pool, block, &[(0x10 + h) as u8; 32]).await?;
        let night = [(0x30 + h) as u8; 32];
        // dgi row is the join target and supplies generation_index + owner.
        insert_generation_info(&pool, &owner, &night, 100, Some(h - 1), Some(h * 1000), tx).await?;
        insert_dtime_event(&pool, tx, night, (h * 1000) as u64, &[0xaa, h as u8]).await?;
        block_ids.push(block);
    }
    // A different owner's update in block 2 — must never be returned for `owner`.
    let other_night = [0x99u8; 32];
    let tx2 = insert_transaction(&pool, block_ids[1], &[0x22u8; 32]).await?;
    insert_generation_info(
        &pool,
        &other_owner,
        &other_night,
        100,
        Some(9),
        Some(2000),
        tx2,
    )
    .await?;
    insert_dtime_event(&pool, tx2, other_night, 2000, &[0xbb]).await?;

    // Bound to (block 1, block 2]: only block 2's own update survives (other owner excluded).
    let updates: Vec<_> = storage
        .get_dust_generation_dtime_updates(
            &owner,
            block_ids[0] as u64,
            block_ids[1] as u64,
            0,
            BATCH,
        )
        .await
        .try_collect()
        .await?;

    assert_eq!(
        updates.len(),
        1,
        "exactly one owned update in (block 1, block 2]"
    );
    assert_eq!(updates[0].owner.0, owner);
    assert_eq!(updates[0].generation_mt_index, 1);
    assert_eq!(
        updates[0].new_dtime, 2000,
        "dtime recovered from attributes"
    );
    assert_eq!(
        updates[0].tree_insertion_path.0,
        vec![0xaa, 2],
        "tree_insertion_path recovered from attributes"
    );

    // cutoff 0, upper = block 3 → all three owned updates, ordered by ledger event id.
    let all: Vec<_> = storage
        .get_dust_generation_dtime_updates(&owner, 0, block_ids[2] as u64, 0, BATCH)
        .await
        .try_collect()
        .await?;
    assert_eq!(
        all.iter()
            .map(|u| u.generation_mt_index)
            .collect::<Vec<_>>(),
        vec![0, 1, 2],
        "only the owner's updates, no other-owner leakage"
    );

    Ok(())
}
