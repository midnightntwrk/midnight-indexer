// This file is part of midnight-indexer.
// Copyright (C) 2025 Midnight Foundation
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

use crate::infra::pool::postgres::PostgresPool;
use indoc::indoc;
use sqlx::migrate::MigrateError;
use thiserror::Error;

/// Run the database migrations for Postgres.
pub async fn run(pool: &PostgresPool) -> Result<(), Error> {
    sqlx::migrate!("migrations/postgres").run(&**pool).await?;
    Ok(())
}

/// Seed mock system parameters if tables are empty.
pub async fn seed_mock_system_parameters(pool: &PostgresPool) -> Result<(), SeedError> {
    let query = "SELECT COUNT(*) FROM system_parameters_d";
    let (count,): (i64,) = sqlx::query_as(query).fetch_one(&**pool).await?;

    if count == 0 {
        // 2025-01-01 00:00:00 UTC in milliseconds
        let base_ts: i64 = 1735689600000;
        // 6 seconds per block
        let block_time_ms: i64 = 6000;

        let d_param_query = indoc! {"
            INSERT INTO system_parameters_d (
                block_height,
                block_hash,
                timestamp,
                num_permissioned_candidates,
                num_registered_candidates
            )
            VALUES ($1, $2, $3, $4, $5)
        "};

        let tc_query = indoc! {"
            INSERT INTO system_parameters_terms_and_conditions (
                block_height,
                block_hash,
                timestamp,
                hash,
                url
            )
            VALUES ($1, $2, $3, $4, $5)
        "};

        let d_params: &[(i64, &str, i32, i32)] = &[
            (
                0,
                "0000000000000000000000000000000000000000000000000000000000000000",
                10,
                0,
            ),
            (
                5,
                "0a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b",
                10,
                1,
            ),
            (
                12,
                "1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b",
                10,
                2,
            ),
            (
                18,
                "1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2c",
                9,
                3,
            ),
            (
                25,
                "3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d",
                8,
                4,
            ),
            (
                32,
                "4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d",
                7,
                5,
            ),
            (
                40,
                "5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f",
                6,
                6,
            ),
            (
                50,
                "6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a",
                5,
                7,
            ),
            (
                65,
                "7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b",
                4,
                8,
            ),
            (
                80,
                "8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c",
                3,
                9,
            ),
            (
                100,
                "9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d",
                2,
                10,
            ),
        ];

        for (block_height, block_hash_hex, num_perm, num_reg) in d_params {
            sqlx::query(d_param_query)
                .bind(*block_height)
                .bind(hex_to_bytes(block_hash_hex))
                .bind(base_ts + block_height * block_time_ms)
                .bind(*num_perm)
                .bind(*num_reg)
                .execute(&**pool)
                .await?;
        }

        let tc_params: &[(i64, &str, &str, &str)] = &[
            (
                0,
                "0000000000000000000000000000000000000000000000000000000000000000",
                "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2",
                "https://midnight.network/terms-and-conditions/v1.0.0",
            ),
            (
                8,
                "0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c",
                "a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3",
                "https://midnight.network/terms-and-conditions/v1.0.1",
            ),
            (
                20,
                "2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c",
                "b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2c3",
                "https://midnight.network/terms-and-conditions/v1.1.0",
            ),
            (
                35,
                "4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e",
                "c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2c3d4",
                "https://midnight.network/terms-and-conditions/v2.0.0",
            ),
            (
                55,
                "5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e",
                "d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5",
                "https://midnight.network/terms-and-conditions/v2.1.0",
            ),
            (
                70,
                "6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f",
                "e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6",
                "https://midnight.network/terms-and-conditions/v2.2.0",
            ),
            (
                90,
                "7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a",
                "f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7",
                "https://midnight.network/terms-and-conditions/v3.0.0",
            ),
        ];

        for (block_height, block_hash_hex, tc_hash_hex, url) in tc_params {
            sqlx::query(tc_query)
                .bind(*block_height)
                .bind(hex_to_bytes(block_hash_hex))
                .bind(base_ts + block_height * block_time_ms)
                .bind(hex_to_bytes(tc_hash_hex))
                .bind(*url)
                .execute(&**pool)
                .await?;
        }
    }

    Ok(())
}

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

/// Error possibly returned by [seed_mock_system_parameters].
#[derive(Debug, Error)]
#[error("cannot seed mock system parameters")]
pub struct SeedError(#[from] sqlx::Error);

/// Error possibly returned by [run].
#[derive(Debug, Error)]
#[error("cannot run migrations for postgres")]
pub struct Error(#[from] MigrateError);

#[cfg(test)]
mod tests {
    use crate::infra::{
        migrations::postgres::run,
        pool::{self, postgres::PostgresPool},
    };
    use anyhow::Context;
    use sqlx::{Row, postgres::PgSslMode};
    use std::{collections::HashSet, error::Error as StdError, time::Duration};
    use testcontainers::{ImageExt, runners::AsyncRunner};
    use testcontainers_modules::postgres::Postgres;

    #[tokio::test]
    async fn test_run() -> Result<(), Box<dyn StdError>> {
        let postgres_container = Postgres::default()
            .with_db_name("indexer")
            .with_user("indexer")
            .with_password(env!("APP__INFRA__STORAGE__PASSWORD"))
            .with_tag("17.1-alpine")
            .start()
            .await
            .context("start Postgres container")?;
        let postgres_port = postgres_container
            .get_host_port_ipv4(5432)
            .await
            .context("get Postgres port")?;

        let config = pool::postgres::Config {
            host: "localhost".to_string(),
            port: postgres_port,
            dbname: "indexer".to_string(),
            user: "indexer".to_string(),
            password: env!("APP__INFRA__STORAGE__PASSWORD").into(),
            sslmode: PgSslMode::Prefer,
            max_connections: 10,
            idle_timeout: Duration::from_secs(60),
            max_lifetime: Duration::from_secs(5 * 60),
        };
        let pool = PostgresPool::new(config).await?;

        let result = run(&pool).await;
        assert!(result.is_ok());

        let table_names = sqlx::query(
            "SELECT tablename
             FROM pg_catalog.pg_tables
             WHERE schemaname = 'public'",
        )
        .fetch_all(&*pool)
        .await?
        .into_iter()
        .map(|row| row.get::<String, _>(0))
        .collect::<HashSet<_>>();

        assert!(table_names.contains("_sqlx_migrations"));

        Ok(())
    }
}
