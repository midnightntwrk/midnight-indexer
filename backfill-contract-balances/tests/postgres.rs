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

//! End-to-end test of the backfill binary against a real Postgres with the production
//! migrations: dry-run writes nothing, apply inserts byte-exact rows, a second apply is
//! a no-op (idempotency).

#![cfg(feature = "cloud")]

mod util;

use indexer_common::infra::{migrations, pool::postgres::PostgresPool};
use sqlx::{Row, postgres::PgSslMode};
use std::{process::Command, time::Duration};
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;
use util::{REAL_AMOUNT, REAL_STATE_HEX, REAL_TOKEN_TYPE, from_hex, synthetic_v4_state};

#[tokio::test(flavor = "multi_thread")]
async fn test_backfill_postgres() {
    let container = Postgres::default()
        .with_db_name("indexer")
        .with_user("indexer")
        .with_password("indexer")
        .with_tag("17.1-alpine")
        .start()
        .await
        .expect("Postgres container starts");
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("Postgres port is mapped");

    let config = indexer_common::infra::pool::postgres::Config {
        host: "localhost".to_string(),
        port,
        dbname: "indexer".to_string(),
        user: "indexer".to_string(),
        password: "indexer".into(),
        sslmode: PgSslMode::Prefer,
        max_connections: 5,
        idle_timeout: Duration::from_secs(60),
        max_lifetime: Duration::from_secs(5 * 60),
    };
    let pool = PostgresPool::new(config)
        .await
        .expect("pool can be created");
    migrations::postgres::run(&pool)
        .await
        .expect("migrations run");

    // Seed: one block, a V8 and a V9 transaction, then three contract actions: the real
    // preview state, a synthetic V9 state, and an empty state (failed-action convention).
    let real_state = from_hex(REAL_STATE_HEX);
    let synthetic_state = synthetic_v4_state();

    sqlx::query(
        "INSERT INTO blocks
         (hash, height, protocol_version, parent_hash, timestamp, zswap_merkle_tree_root,
          ledger_parameters, ledger_state_key)
         VALUES ($1, 0, 22000, $1, 0, $1, $1, $1)",
    )
    .bind([0u8; 32].as_slice())
    .execute(&*pool)
    .await
    .expect("block inserted");

    for (hash_marker, protocol_version) in [(1u8, 22_000i64), (2u8, 2_000_000i64)] {
        sqlx::query(
            "INSERT INTO transactions (block_id, variant, hash, protocol_version, raw)
             VALUES (1, 'Regular', $1, $2, $1)",
        )
        .bind([hash_marker; 32].as_slice())
        .bind(protocol_version)
        .execute(&*pool)
        .await
        .expect("transaction inserted");
    }

    for (transaction_id, address_marker, state) in [
        (1i64, 1u8, real_state.as_slice()),
        (2i64, 2u8, synthetic_state.as_slice()),
        (1i64, 3u8, [].as_slice()),
    ] {
        sqlx::query(
            "INSERT INTO contract_actions
             (transaction_id, variant, address, state, zswap_state, attributes)
             VALUES ($1, 'Call', $2, $3, $2, '{\"Call\": {\"entry_point\": \"test\"}}')",
        )
        .bind(transaction_id)
        .bind([address_marker; 32].as_slice())
        .bind(state)
        .execute(&*pool)
        .await
        .expect("contract action inserted");
    }

    // Dry-run: no writes, reports what it would insert.
    let output = run_backfill(port, false);
    assert!(output.status.success(), "dry-run succeeds: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("would insert"),
        "dry-run reports rows: {stdout}"
    );
    assert_eq!(count_balances(&pool).await, 0, "dry-run writes nothing");

    // Apply: exactly the two expected rows, byte-identical encodings.
    let output = run_backfill(port, true);
    assert!(output.status.success(), "apply succeeds: {output:?}");
    assert_eq!(count_balances(&pool).await, 2);

    let rows = sqlx::query(
        "SELECT contract_action_id, token_type, amount
         FROM contract_balances
         ORDER BY contract_action_id",
    )
    .fetch_all(&*pool)
    .await
    .expect("balances can be queried");

    assert_eq!(rows[0].get::<i64, _>("contract_action_id"), 1);
    assert_eq!(
        rows[0].get::<Vec<u8>, _>("token_type"),
        from_hex(REAL_TOKEN_TYPE)
    );
    assert_eq!(
        rows[0].get::<Vec<u8>, _>("amount"),
        REAL_AMOUNT.to_be_bytes()
    );

    assert_eq!(rows[1].get::<i64, _>("contract_action_id"), 2);
    assert_eq!(
        rows[1].get::<Vec<u8>, _>("token_type"),
        util::SYNTHETIC_TOKEN_TYPE
    );
    assert_eq!(
        rows[1].get::<Vec<u8>, _>("amount"),
        util::SYNTHETIC_AMOUNT.to_be_bytes()
    );

    // Idempotency: a second apply changes nothing.
    let output = run_backfill(port, true);
    assert!(output.status.success(), "second apply succeeds: {output:?}");
    assert_eq!(count_balances(&pool).await, 2, "second apply is a no-op");
}

fn run_backfill(port: u16, apply: bool) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_backfill-contract-balances"))
        .env("APP__INFRA__STORAGE__HOST", "127.0.0.1")
        .env("APP__INFRA__STORAGE__PORT", port.to_string())
        .env("APP__INFRA__STORAGE__DBNAME", "indexer")
        .env("APP__INFRA__STORAGE__USER", "indexer")
        .env("APP__INFRA__STORAGE__PASSWORD", "indexer")
        .env("APP__INFRA__STORAGE__SSLMODE", "prefer")
        .env("APPLY", if apply { "1" } else { "0" })
        .env("BATCH", "2") // smaller than the seeded action count to exercise paging
        .output()
        .expect("backfill binary runs")
}

async fn count_balances(pool: &PostgresPool) -> i64 {
    sqlx::query("SELECT count(*)::BIGINT AS n FROM contract_balances")
        .fetch_one(&**pool)
        .await
        .expect("count query runs")
        .get::<i64, _>("n")
}
