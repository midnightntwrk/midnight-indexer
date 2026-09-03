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

use crate::infra::pool::sqlite::SqlitePool;
use indoc::indoc;
use sqlx::migrate::MigrateError;
use thiserror::Error;

/// Version of `011_contract_state_keys.sql`, which drops the contract-state blob columns.
const CONTRACT_STATE_KEYS_VERSION: i64 = 11;

/// Run the database migrations for SQLite.
pub async fn run(pool: &SqlitePool) -> Result<(), Error> {
    refuse_unconvertible_contract_states(pool).await?;
    sqlx::migrate!("migrations/sqlite").run(&**pool).await?;
    Ok(())
}

/// Run the database migrations for SQLite for the ledger DB.
///
/// Deliberately unguarded: the ledger DB is a separate schema of arena nodes and roots with no
/// contract actions in it.
pub async fn run_for_ledger_db(pool: &SqlitePool) -> Result<(), Error> {
    sqlx::migrate!("migrations/sqlite-ledger-db")
        .run(&**pool)
        .await?;
    Ok(())
}

/// Refuse to apply `011_contract_state_keys.sql` to a database that still stores contract states
/// as blobs.
///
/// That migration drops `state` and `zswap_state` without converting them - the arena nodes their
/// keys would point at were garbage collected, and no SQL can replay the chain to recreate them -
/// so applying it destroys data irrecoverably and leaves a database no version can read: the new
/// code refuses it for want of keys, the old code queries columns that no longer exist. Refusing
/// before the migration keeps the database readable by the version that wrote it, so an operator
/// who upgraded by mistake can still roll back.
///
/// The check necessarily runs *before* the migration, so it cannot ask about `state_key` - the
/// migration is what adds that column. It asks the equivalent question of the pre-migration
/// schema instead: are there contract actions whose blobs this migration would drop?
async fn refuse_unconvertible_contract_states(pool: &SqlitePool) -> Result<(), Error> {
    // No table means a fresh database, which has nothing to lose.
    let contract_actions_exists = sqlx::query_scalar::<_, i64>(indoc! {"
        SELECT 1
        FROM sqlite_master
        WHERE type = 'table' AND name = 'contract_actions'
        LIMIT 1
    "})
    .fetch_optional(&**pool)
    .await?
    .is_some();
    if !contract_actions_exists {
        return Ok(());
    }

    // Already applied: the blobs are gone either way, and chain-indexer's own startup check is
    // what reports the rows left without keys. An absent migrations table counts as not applied,
    // so an unrecognizable database errs towards refusing rather than dropping.
    let applied = sqlx::query_scalar::<_, i64>(indoc! {"
        SELECT 1
        FROM sqlite_master
        WHERE type = 'table' AND name = '_sqlx_migrations'
        LIMIT 1
    "})
    .fetch_optional(&**pool)
    .await?
    .is_some()
        && sqlx::query_scalar::<_, i64>(indoc! {"
            SELECT 1
            FROM _sqlx_migrations
            WHERE version = ?
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
        sqlx::query_scalar::<_, i64>("SELECT 1 FROM contract_actions LIMIT 1")
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
    #[error("cannot run migrations for sqlite")]
    Migrate(#[from] MigrateError),

    #[error("cannot inspect the database before running migrations for sqlite")]
    Inspect(#[from] sqlx::Error),

    #[error(
        "refusing to migrate: this database stores contract states as blobs, which \
         011_contract_state_keys.sql drops without converting them; the blobs cannot be \
         recreated. Sync a new indexer from genesis and cut over, or - for a single instance - \
         wipe both the indexer database and the ledger DB and re-index from genesis. See \
         docs/re-indexing.md"
    )]
    UnconvertibleContractStates,
}

#[cfg(test)]
mod tests {
    use crate::infra::{
        migrations::sqlite::{Error, refuse_unconvertible_contract_states, run},
        pool::{self, sqlite::SqlitePool},
    };
    use anyhow::Context;
    use std::error::Error as StdError;

    /// A fresh database has no contract actions to lose, so migrating is allowed and creates the
    /// key columns.
    #[tokio::test]
    async fn test_fresh_database_migrates() -> Result<(), Box<dyn StdError>> {
        let temp_dir = tempfile::tempdir().context("create tempdir")?;
        let pool = new_pool(&temp_dir).await?;

        run(&pool).await.context("run migrations")?;

        let columns = sqlx::query_scalar::<_, String>(
            "SELECT name FROM pragma_table_info('contract_actions')",
        )
        .fetch_all(&*pool)
        .await?;
        assert!(columns.contains(&"state_key".to_string()));
        assert!(!columns.contains(&"state".to_string()));

        Ok(())
    }

    /// A database whose contract actions still carry blobs must be refused, and refused *without*
    /// the migration having dropped anything - that is the whole point of checking first.
    #[tokio::test]
    async fn test_blob_era_database_is_refused() -> Result<(), Box<dyn StdError>> {
        let temp_dir = tempfile::tempdir().context("create tempdir")?;
        let pool = new_pool(&temp_dir).await?;

        // A pre-key `contract_actions` carrying one row, as a version that stored blobs left it.
        sqlx::query("CREATE TABLE contract_actions (id INTEGER PRIMARY KEY, state BLOB)")
            .execute(&*pool)
            .await?;
        sqlx::query("INSERT INTO contract_actions (id, state) VALUES (1, x'0102')")
            .execute(&*pool)
            .await?;

        let error = run(&pool).await.expect_err("migration is refused");
        assert!(matches!(error, Error::UnconvertibleContractStates));

        // The blob column, and the blob, survive the refusal.
        let state = sqlx::query_scalar::<_, Vec<u8>>("SELECT state FROM contract_actions")
            .fetch_one(&*pool)
            .await?;
        assert_eq!(state, vec![0x01, 0x02]);

        Ok(())
    }

    /// An empty pre-key `contract_actions` has no blobs to lose, so it is not refused.
    #[tokio::test]
    async fn test_empty_contract_actions_is_allowed() -> Result<(), Box<dyn StdError>> {
        let temp_dir = tempfile::tempdir().context("create tempdir")?;
        let pool = new_pool(&temp_dir).await?;

        sqlx::query("CREATE TABLE contract_actions (id INTEGER PRIMARY KEY, state BLOB)")
            .execute(&*pool)
            .await?;

        let result = refuse_unconvertible_contract_states(&pool).await;
        assert!(result.is_ok());

        Ok(())
    }

    /// Once the migration has been applied, the check steps aside: the blobs are already gone and
    /// chain-indexer's own startup check is what reports rows left without keys.
    #[tokio::test]
    async fn test_already_migrated_is_allowed() -> Result<(), Box<dyn StdError>> {
        let temp_dir = tempfile::tempdir().context("create tempdir")?;
        let pool = new_pool(&temp_dir).await?;

        run(&pool).await.context("run migrations")?;

        // Short-circuits on the applied migration without inspecting rows at all, which is what
        // lets an already-migrated database keep starting up.
        let result = refuse_unconvertible_contract_states(&pool).await;
        assert!(result.is_ok());

        // Idempotent: running again on an already-migrated database is still fine.
        run(&pool).await.context("re-run migrations")?;

        Ok(())
    }

    async fn new_pool(temp_dir: &tempfile::TempDir) -> anyhow::Result<SqlitePool> {
        let cnn_url = temp_dir.path().join("indexer.sqlite").display().to_string();
        SqlitePool::new(pool::sqlite::Config::with_url(cnn_url))
            .await
            .context("create pool")
    }
}
