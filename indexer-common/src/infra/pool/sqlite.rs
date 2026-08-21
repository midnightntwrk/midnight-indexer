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

use log::debug;
use serde::Deserialize;
use sqlx::{
    Sqlite, Transaction,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::{ops::Deref, time::Duration};
use thiserror::Error;

/// SQLite pools split by role: one write connection, `max_connections` read connections.
///
/// SQLite allows a single writer per database file and its `busy_timeout` handler polls for
/// the lock without any queueing, so under sustained writes (chain-indexer catching up) a
/// waiting writer can starve past any timeout and fail with `SQLITE_BUSY`. Routing all
/// writes through a dedicated single-connection pool replaces that unfair polling with
/// sqlx's fair FIFO pool checkout: in-process writers queue on `acquire` and the SQLite
/// write lock itself is never contended from within the process.
///
/// `Deref` yields the read pool (`PRAGMA query_only=ON`, so a mis-routed write fails loudly
/// instead of contending silently). Transactions via [SqlitePool::begin] and bare write
/// statements via [SqlitePool::writer] use the write pool.
#[derive(Debug, Clone)]
pub struct SqlitePool {
    read: sqlx::SqlitePool,
    write: sqlx::SqlitePool,
}

impl SqlitePool {
    /// Try to create a new [SqlitePool] with the given config.
    ///
    /// With `max_connections <= 1` the read pool is the write pool: callers like the ledger
    /// DB require reads to observe the writer connection's own in-progress state (see
    /// `infra::ledger_db::init`). The same applies to in-memory databases, where every new
    /// connection would otherwise open its own empty database.
    pub async fn new(config: Config) -> Result<Self, Error> {
        let max_connections = config.max_connections;
        let single_connection = max_connections <= 1
            || config.cnn_url.contains(":memory:")
            || config.cnn_url.contains("mode=memory");
        let connect_options =
            SqliteConnectOptions::try_from(config).map_err(Error::ConvertConfig)?;

        // The write pool connects first: on a fresh database it creates the file and
        // switches it to WAL before any read-only connection touches it. The generous
        // acquire timeout replaces `busy_timeout` as the wait bound for writers; the wait
        // is fair, so it only expires if the writer ahead genuinely holds the connection
        // that long.
        let write = SqlitePoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(300))
            .connect_with(connect_options.clone())
            .await?;
        let read = if single_connection {
            write.clone()
        } else {
            SqlitePoolOptions::new()
                .max_connections(max_connections)
                .connect_with(connect_options.pragma("query_only", "ON"))
                .await?
        };
        let pool = SqlitePool { read, write };
        debug!(pool:?; "created pool");

        Ok(pool)
    }

    /// Begin a transaction on the write pool with `BEGIN IMMEDIATE` semantics, claiming the
    /// writer lock up front.
    ///
    /// In-process writers queue fairly on the pool's single connection, so the lock is
    /// uncontended here; `BEGIN IMMEDIATE` still guards against `SQLITE_BUSY_SNAPSHOT`
    /// (517) races with writers outside the process (e.g. the sqlite3 CLI).
    pub async fn begin(&self) -> Result<Transaction<'static, Sqlite>, sqlx::Error> {
        self.write.begin_with("BEGIN IMMEDIATE").await
    }

    /// The pool for write statements executed outside a transaction. Reads go through
    /// `Deref`.
    pub fn writer(&self) -> &sqlx::SqlitePool {
        &self.write
    }
}

impl Deref for SqlitePool {
    type Target = sqlx::SqlitePool;

    fn deref(&self) -> &Self::Target {
        &self.read
    }
}

/// Error possibly returned by [SqlitePool::new].
#[derive(Debug, Error)]
pub enum Error {
    #[error("cannot convert config into sqlite connect options")]
    ConvertConfig(#[source] sqlx::Error),

    #[error("cannot create sqlite connection pool")]
    CreatePool(#[from] sqlx::Error),
}

/// Configuration for [SqlitePool].
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub cnn_url: String,

    /// Size of the read pool; writes always use one dedicated connection. `1` collapses
    /// reads onto the write connection (see [SqlitePool::new]).
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,

    /// `synchronous=FULL` instead of `NORMAL`: every commit fsyncs the WAL. Not exposed to
    /// operators - set in code for DBs whose commits must hit disk in order relative to
    /// other files (the ledger DB; see `infra::ledger_db::init`).
    #[serde(skip)]
    pub synchronous_full: bool,
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
            .create_if_missing(true)
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
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::infra::pool::sqlite::{Config, SqlitePool};
    use std::{ops::Deref, path::Path};
    use tokio::fs;

    #[tokio::test]
    async fn test_sqlite_pool_file_creation() {
        let db_path = "test_indexer.sqlite";

        if Path::new(db_path).exists() {
            fs::remove_file(db_path)
                .await
                .expect("Failed to remove existing test database file");
        }
        assert!(!Path::new(db_path).exists());

        let pool = SqlitePool::new(Config::with_url(format!("sqlite://{db_path}"))).await;

        assert!(pool.is_ok());
        assert!(Path::new(db_path).exists());
        fs::remove_file(db_path)
            .await
            .expect("Failed to remove test database file");
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
            .execute(pool.writer())
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

    /// A write mis-routed to the read pool must fail loudly (`query_only=ON`) instead of
    /// silently contending with the write connection.
    #[tokio::test]
    async fn read_pool_rejects_writes() {
        let temp_dir = tempfile::tempdir().expect("create tempdir");
        let db_path = temp_dir.path().join("readonly.sqlite").display().to_string();
        let pool = SqlitePool::new(Config::with_url(format!("sqlite://{db_path}")))
            .await
            .expect("create pool");

        sqlx::query("CREATE TABLE test (id INTEGER PRIMARY KEY)")
            .execute(pool.writer())
            .await
            .expect("create table via write pool");

        let result = sqlx::query("INSERT INTO test (id) VALUES (1)")
            .execute(pool.deref())
            .await;
        assert!(result.is_err());
    }
}
