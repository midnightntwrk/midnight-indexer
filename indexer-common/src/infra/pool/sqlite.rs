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

use derive_more::Into;
use log::debug;
use serde::Deserialize;
use sqlx::{
    ConnectOptions, Sqlite, Transaction,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::{ops::Deref, path::PathBuf, time::Duration};
use thiserror::Error;

/// New type for `sqlx::SqlitePool`, allowing for some custom extensions as well as security.
///
/// To use as `&sqlx::SqlitePool` in `Query::execute`, use its `Deref` implementation: `&*pool` or
/// `pool.deref()`. If an owned `sqlx::SqlitePool` is needed, use `Into::into`.
#[derive(Debug, Clone, Into)]
pub struct SqlitePool(sqlx::SqlitePool);

impl SqlitePool {
    /// Try to create a new [SqlitePool] with the given config.
    pub async fn new(config: Config) -> Result<Self, Error> {
        let max_connections = config.max_connections;
        let create_if_missing = config.create_if_missing;
        let cnn_url = config.cnn_url.clone();
        let connect_options = SqliteConnectOptions::try_from(config)
            .map_err(|source| Error::ConvertConfig { cnn_url, source })?;
        let path = connect_options.get_filename().to_path_buf();
        // Read SQLx's parsed mode rather than guessing from the filename. Its generated
        // in-memory filename is not URL-safe in SQLx 0.8, so replace only the filename
        // in this temporary copy before inspecting the mode.
        let in_memory = connect_options
            .clone()
            .filename("/")
            .to_url_lossy()
            .query_pairs()
            .any(|(key, value)| key == "mode" && value == "memory");
        if !create_if_missing && !in_memory {
            let metadata =
                tokio::fs::metadata(&path)
                    .await
                    .map_err(|source| Error::InspectFile {
                        path: path.clone(),
                        source,
                    })?;
            if metadata.len() == 0 {
                return Err(Error::EmptyFile { path });
            }
        }
        let inner = SqlitePoolOptions::new()
            .max_connections(max_connections)
            .connect_with(connect_options)
            .await
            .map_err(|source| Error::CreatePool { path, source })?;
        let pool = SqlitePool(inner);
        debug!(pool:?; "created pool");

        Ok(pool)
    }

    /// Begin a transaction with `BEGIN IMMEDIATE` semantics, claiming the
    /// writer lock up front.
    ///
    /// SQLite's default `BEGIN DEFERRED` transaction stays a reader until its
    /// first write statement. When multiple connections run in WAL mode, a
    /// deferred reader that later tries to upgrade to a writer (while another
    /// connection has committed in between) gets `SQLITE_BUSY_SNAPSHOT` (517),
    /// which is not retryable via `busy_timeout`. Every caller in this
    /// codebase that starts a transaction does so to write, so taking the
    /// write lock immediately is both correct and avoids the race. Shadowing
    /// `sqlx::Pool::begin` via inherent method makes the existing
    /// `self.pool.begin()` call sites pick up the new behavior transparently.
    ///
    /// `busy_timeout` bounds a single lock wait, but SQLite's busy handler is unfair polling,
    /// not a queue: under back-to-back writers (chain-indexer catching up) a waiter can lose
    /// every retry window and starve past the timeout. A begin failure is fatal to the whole
    /// standalone process (components propagate it and main exits), so retry a few times
    /// before giving up.
    pub async fn begin(&self) -> Result<Transaction<'static, Sqlite>, sqlx::Error> {
        const ATTEMPTS: u32 = 4;

        let mut attempt = 1;
        loop {
            match self.0.begin_with("BEGIN IMMEDIATE").await {
                Err(error) if is_busy_error(&error) && attempt < ATTEMPTS => {
                    log::warn!(attempt; "sqlite writer starved past busy_timeout, retrying begin");
                    attempt += 1;
                }

                other => return other,
            }
        }
    }
}

/// Whether the error is SQLite's `SQLITE_BUSY` family ("database is locked"), i.e. lock
/// contention that outlasted `busy_timeout` - transient by nature and safe to retry or skip
/// for housekeeping writes.
pub fn is_busy_error(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(db) if db.message().contains("database is locked"))
}

impl Deref for SqlitePool {
    type Target = sqlx::SqlitePool;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Error possibly returned by [SqlitePool::new].
#[derive(Debug, Error)]
pub enum Error {
    #[error("cannot parse SQLite connection URL {cnn_url:?}: {source}")]
    ConvertConfig {
        cnn_url: String,
        #[source]
        source: sqlx::Error,
    },

    #[error("cannot inspect existing SQLite database {path:?} (create_if_missing=false): {source}")]
    InspectFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error(
        "SQLite database {path:?} is empty (create_if_missing=false); refusing to initialize it"
    )]
    EmptyFile { path: PathBuf },

    #[error("cannot open SQLite database {path:?}: {source}")]
    CreatePool {
        path: PathBuf,
        #[source]
        source: sqlx::Error,
    },
}

/// Configuration for [SqlitePool].
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// SQLite connection URL or filesystem path.
    pub cnn_url: String,

    /// Maximum number of connections in the pool. Defaults to eight.
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,

    /// `synchronous=FULL` instead of `NORMAL`: every commit fsyncs the WAL. Not exposed to
    /// operators - set in code for DBs whose commits must hit disk in order relative to
    /// other files (the ledger DB; see `infra::ledger_db::init`).
    #[serde(skip)]
    pub synchronous_full: bool,

    /// Allow creating a missing database file. Defaults to true for fresh installations.
    /// When false, require an existing, non-empty file before opening it; this also
    /// rejects zero-byte files to avoid silently initializing a replacement database.
    /// In-memory databases are exempt. This does not validate schema or indexed data.
    #[serde(default = "create_if_missing_default")]
    pub create_if_missing: bool,
}

impl Config {
    /// Build a [Config] for the given connection URL using defaults for the
    /// remaining fields.
    pub fn with_url(cnn_url: impl Into<String>) -> Self {
        Self {
            cnn_url: cnn_url.into(),
            ..Default::default()
        }
    }
}

pub(crate) const fn create_if_missing_default() -> bool {
    true
}

fn default_max_connections() -> u32 {
    8
}

impl TryFrom<Config> for SqliteConnectOptions {
    type Error = sqlx::Error;

    fn try_from(config: Config) -> Result<Self, Self::Error> {
        // WAL lets readers run concurrent with a single writer; without it the
        // default `DELETE` journal mode serializes all access on a single file.
        // `busy_timeout` lets SQLite itself retry on lock contention instead of
        // immediately returning `SQLITE_BUSY`. It needs to cover the worst-case
        // writer hold time: on mainnet, chain-indexer's per-block write
        // transaction (many inserts across several tables) can exceed a few
        // seconds, so a short timeout causes concurrent writes (e.g. an API
        // `disconnect_wallet` UPDATE) to spuriously fail.
        //
        // `synchronous=NORMAL` fsyncs the WAL only at checkpoints: a power loss can drop the
        // last commits (never corrupt), all of which the indexer recomputes from the node -
        // except wallet sessions, which clients re-establish by reconnecting. DBs that cannot
        // accept tail loss opt into FULL via `synchronous_full`.
        let synchronous = if config.synchronous_full {
            SqliteSynchronous::Full
        } else {
            SqliteSynchronous::Normal
        };
        let options = config
            .cnn_url
            .parse::<SqliteConnectOptions>()?
            .create_if_missing(config.create_if_missing)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(synchronous)
            .busy_timeout(Duration::from_secs(30));
        Ok(options)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            cnn_url: "sqlite::memory:".to_string(),
            max_connections: default_max_connections(),
            synchronous_full: false,
            create_if_missing: create_if_missing_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::infra::pool::sqlite::{Config, Error, SqlitePool};
    use std::ops::Deref;
    use tokio::fs;

    #[tokio::test]
    async fn test_sqlite_pool_file_creation() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("test_indexer.sqlite");
        let pool = SqlitePool::new(Config::with_url(db_path.to_str().unwrap()))
            .await
            .unwrap();

        assert!(db_path.exists());
        pool.close().await;
    }

    #[tokio::test]
    async fn test_sqlite_pool_file_creation_disabled() {
        let temp_dir = tempfile::tempdir().expect("create temp directory");
        let db_path = temp_dir.path().join("missing.sqlite");

        let error = SqlitePool::new(Config {
            create_if_missing: false,
            ..Config::with_url(format!("sqlite://{}", db_path.display()))
        })
        .await
        .unwrap_err();

        assert!(error.to_string().contains(db_path.to_str().unwrap()));
        assert!(error.to_string().contains("create_if_missing=false"));
        assert!(matches!(error, Error::InspectFile { source, .. }
            if source.kind() == std::io::ErrorKind::NotFound));
        assert!(!db_path.exists());
    }

    #[tokio::test]
    async fn creation_disabled_rejects_empty_files_without_initializing_them() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("empty-memory.sqlite");
        fs::write(&db_path, []).await.unwrap();

        // The explicit flag must override mode=rwc, and "memory" in a filename
        // must not exempt a real file from the guard.
        let error = SqlitePool::new(Config {
            create_if_missing: false,
            ..Config::with_url(format!("sqlite://{}?mode=rwc", db_path.display()))
        })
        .await
        .unwrap_err();

        assert!(error.to_string().contains(db_path.to_str().unwrap()));
        assert!(error.to_string().contains("empty"));
        assert!(matches!(error, Error::EmptyFile { .. }));
        assert_eq!(fs::metadata(&db_path).await.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn creation_disabled_reopens_existing_database() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("existing.sqlite");
        let mut config = Config::with_url(db_path.to_str().unwrap());
        let pool = SqlitePool::new(config.clone()).await.unwrap();
        sqlx::query("CREATE TABLE sentinel (id INTEGER)")
            .execute(pool.deref())
            .await
            .unwrap();
        sqlx::query("INSERT INTO sentinel VALUES (42)")
            .execute(pool.deref())
            .await
            .unwrap();
        pool.close().await;

        config.create_if_missing = false;
        let pool = SqlitePool::new(config).await.unwrap();
        let id = sqlx::query_scalar::<_, i64>("SELECT id FROM sentinel")
            .fetch_one(pool.deref())
            .await
            .unwrap();
        assert_eq!(id, 42);
        pool.close().await;
    }

    #[tokio::test]
    async fn creation_disabled_allows_in_memory_databases() {
        for url in [
            "sqlite::memory:",
            "sqlite://?mode=memory",
            "sqlite://named?mode=memory",
        ] {
            let pool = SqlitePool::new(Config {
                create_if_missing: false,
                ..Config::with_url(url)
            })
            .await
            .unwrap();
            pool.close().await;
        }
    }

    #[tokio::test]
    async fn connection_errors_identify_the_database_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir
            .path()
            .join("missing-directory")
            .join("indexer.sqlite");
        let error = SqlitePool::new(Config::with_url(db_path.to_str().unwrap()))
            .await
            .unwrap_err();

        assert!(error.to_string().contains(db_path.to_str().unwrap()));
        assert!(matches!(error, Error::CreatePool { .. }));
        assert!(!db_path.exists());
    }

    #[tokio::test]
    async fn test_pool() {
        let pool = SqlitePool::new(Config::default()).await;
        assert!(pool.is_ok());
        let pool = pool.unwrap();

        let result = sqlx::query("CREATE TABLE test (id integer PRIMARY KEY)")
            .execute(pool.deref())
            .await;
        assert!(result.is_ok());
    }

    /// Regression test for the PM-15038 failure shape: many concurrent write transactions on
    /// one file-backed DB, each also reading inside its transaction like wallet-indexer's
    /// read-modify-write. With DEFERRED transactions this dies with `database is locked`
    /// (SQLITE_BUSY, 5) or `SQLITE_BUSY_SNAPSHOT` (517, not retried by busy_timeout);
    /// `BEGIN IMMEDIATE` + WAL + busy_timeout must let every writer through.
    #[tokio::test(flavor = "multi_thread")]
    async fn concurrent_write_transactions_never_hit_busy_errors() {
        const WRITERS: usize = 8;
        const WRITES_PER_WRITER: usize = 25;

        let temp_dir = tempfile::tempdir().expect("create tempdir");
        let db_path = temp_dir
            .path()
            .join("contention.sqlite")
            .display()
            .to_string();
        let pool = SqlitePool::new(Config::with_url(format!("sqlite://{db_path}")))
            .await
            .expect("create pool");

        sqlx::query("CREATE TABLE wallet_txs (id INTEGER PRIMARY KEY, wallet INTEGER, n INTEGER)")
            .execute(pool.deref())
            .await
            .expect("create table");

        let writers = (0..WRITERS)
            .map(|wallet| {
                let pool = pool.clone();
                tokio::spawn(async move {
                    for n in 0..WRITES_PER_WRITER {
                        let mut tx = pool.begin().await?;
                        // Read BEFORE writing: with a DEFERRED transaction this pins a WAL
                        // snapshot as a reader, and the later INSERT's lock upgrade fails
                        // with SQLITE_BUSY_SNAPSHOT once any other writer commits in
                        // between. This read-then-write order is what makes the test able
                        // to fail at all.
                        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM wallet_txs")
                            .fetch_one(&mut *tx)
                            .await?;
                        sqlx::query("INSERT INTO wallet_txs (wallet, n) VALUES ($1, $2)")
                            .bind(wallet as i64)
                            .bind(n as i64)
                            .execute(&mut *tx)
                            .await?;
                        tx.commit().await?;
                    }
                    Ok::<_, sqlx::Error>(())
                })
            })
            .collect::<Vec<_>>();

        for writer in writers {
            writer
                .await
                .expect("writer task panicked")
                .expect("writer hit an sqlite error");
        }

        let rows = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM wallet_txs")
            .fetch_one(pool.deref())
            .await
            .expect("count rows");
        assert_eq!(rows as usize, WRITERS * WRITES_PER_WRITER);
    }
}
