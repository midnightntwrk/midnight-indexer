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

use crate::infra::pool::postgres::PostgresPool;
use indoc::indoc;
use sqlx::migrate::MigrateError;
use thiserror::Error;

/// Version of `009_contract_state_keys.sql`, which drops the contract-state blob columns.
const CONTRACT_STATE_KEYS_VERSION: i64 = 9;

/// Run the database migrations for Postgres.
pub async fn run(pool: &PostgresPool) -> Result<(), Error> {
    refuse_unconvertible_contract_states(pool).await?;
    sqlx::migrate!("migrations/postgres").run(&**pool).await?;
    Ok(())
}

/// Refuse to apply `009_contract_state_keys.sql` to a database that still stores contract states
/// as blobs.
///
/// That migration drops `state` and `zswap_state` without converting them - the arena nodes their
/// keys would point at were garbage collected, and no SQL can replay the chain to recreate them -
/// so applying it destroys data irrecoverably and leaves a database no version can read: the new
/// code refuses it for want of keys, the old code queries columns that no longer exist. Refusing
/// before the migration keeps the database readable by the version that wrote it, so an operator
/// who upgraded by mistake can still roll back.
///
/// In cloud deployments this is the only thing standing between a rolling deploy and silent data
/// loss: chain-indexer, wallet-indexer, indexer-api and spo-indexer all migrate on startup and
/// none of them depends on the others, so whichever process wins the race applies the migration.
/// Only chain-indexer checks for missing keys afterwards, and by then the blobs are gone.
///
/// The check necessarily runs *before* the migration, so it cannot ask about `state_key` - the
/// migration is what adds that column. It asks the equivalent question of the pre-migration
/// schema instead: are there contract actions whose blobs this migration would drop?
async fn refuse_unconvertible_contract_states(pool: &PostgresPool) -> Result<(), Error> {
    // No table means a fresh database, which has nothing to lose.
    let contract_actions_exists =
        sqlx::query_scalar::<_, i32>("SELECT 1 WHERE to_regclass('contract_actions') IS NOT NULL")
            .fetch_optional(&**pool)
            .await?
            .is_some();
    if !contract_actions_exists {
        return Ok(());
    }

    // Already applied: the blobs are gone either way, and chain-indexer's own startup check is
    // what reports the rows left without keys. An absent migrations table counts as not applied,
    // so an unrecognizable database errs towards refusing rather than dropping.
    let applied =
        sqlx::query_scalar::<_, i32>("SELECT 1 WHERE to_regclass('_sqlx_migrations') IS NOT NULL")
            .fetch_optional(&**pool)
            .await?
            .is_some()
            && sqlx::query_scalar::<_, i32>(indoc! {"
            SELECT 1
            FROM _sqlx_migrations
            WHERE version = $1
            LIMIT 1
        "})
            .bind(CONTRACT_STATE_KEYS_VERSION)
            .fetch_optional(&**pool)
            .await?
            .is_some();
    if applied {
        return Ok(());
    }

    let has_contract_actions =
        sqlx::query_scalar::<_, i32>("SELECT 1 FROM contract_actions LIMIT 1")
            .fetch_optional(&**pool)
            .await?
            .is_some();
    if has_contract_actions {
        return Err(Error::UnconvertibleContractStates);
    }

    Ok(())
}

/// Error possibly returned by [run].
#[derive(Debug, Error)]
pub enum Error {
    #[error("cannot run migrations for postgres")]
    Migrate(#[from] MigrateError),

    #[error("cannot inspect the database before running migrations for postgres")]
    Inspect(#[from] sqlx::Error),

    #[error(
        "refusing to migrate: this database stores contract states as blobs, which \
         009_contract_state_keys.sql drops without converting them; the blobs cannot be \
         recreated. Sync a new indexer from genesis and cut over, or - for a single instance - \
         wipe both the indexer database and the ledger DB and re-index from genesis. See \
         docs/re-indexing.md"
    )]
    UnconvertibleContractStates,
}

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
            min_connections: 0,
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
