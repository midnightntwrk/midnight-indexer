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
    Sqlite, Transaction,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::{ops::Deref, time::Duration};
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
        let connect_options =
            SqliteConnectOptions::try_from(config).map_err(Error::ConvertConfig)?;
        let inner = SqlitePoolOptions::new()
            .max_connections(max_connections)
            .connect_with(connect_options)
            .await?;
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
    pub async fn begin(&self) -> Result<Transaction<'static, Sqlite>, sqlx::Error> {
        self.0.begin_with("BEGIN IMMEDIATE").await
    }
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
    #[error("cannot convert config into sqlite connect options")]
    ConvertConfig(#[source] sqlx::Error),

    #[error("cannot create sqlite connection pool")]
    CreatePool(#[from] sqlx::Error),
}

/// Configuration for [SqlitePool].
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub cnn_url: String,

    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
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
        let options = config
            .cnn_url
            .parse::<SqliteConnectOptions>()?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(30));
        Ok(options)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            cnn_url: "sqlite::memory:".to_string(),
            max_connections: default_max_connections(),
        }
    }
}

#[cfg(test)]
mod pool_concurrency {
    use crate::infra::pool::sqlite::{Config, SqlitePool};
    use std::{
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };
    use tokio::sync::Notify;

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    async fn fresh_pool() -> SqlitePool {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "indexer_pool_test_{}_{}_{}.sqlite",
            std::process::id(),
            nanos,
            seq
        ));
        let url = format!("sqlite://{}", path.display());
        SqlitePool::new(Config { cnn_url: url })
            .await
            .expect("create pool")
    }

    async fn create_t(pool: &SqlitePool) {
        sqlx::query("CREATE TABLE t (id INTEGER PRIMARY KEY AUTOINCREMENT, v INTEGER NOT NULL)")
            .execute(&**pool)
            .await
            .expect("create table");
    }

    /// A SELECT issued while a writer holds an in-progress transaction returns
    /// promptly rather than queueing behind the writer's commit.
    #[tokio::test]
    async fn reader_runs_concurrent_with_in_progress_writer() {
        let pool = fresh_pool().await;
        create_t(&pool).await;

        let writer_acquired = Arc::new(Notify::new());
        let writer_acquired_inner = writer_acquired.clone();
        let pool_writer = pool.clone();

        let writer = tokio::spawn(async move {
            let mut tx = pool_writer.begin().await.expect("begin");
            sqlx::query("INSERT INTO t (v) VALUES (1)")
                .execute(&mut *tx)
                .await
                .expect("insert");
            writer_acquired_inner.notify_one();
            tokio::time::sleep(Duration::from_millis(500)).await;
            tx.commit().await.expect("commit");
        });

        writer_acquired.notified().await;

        let start = Instant::now();
        let result = tokio::time::timeout(
            Duration::from_millis(200),
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM t").fetch_one(&*pool),
        )
        .await;
        let elapsed = start.elapsed();

        writer.await.expect("writer task");

        assert!(
            result.is_ok(),
            "SELECT did not return within 200ms while writer held a 500ms tx; elapsed = {:?}",
            elapsed
        );
    }

    /// Eight concurrent SELECTs against the pool while a writer holds a
    /// transaction complete in roughly the time of one SELECT.
    #[tokio::test]
    async fn many_readers_run_in_parallel_with_writer() {
        let pool = fresh_pool().await;
        create_t(&pool).await;
        sqlx::query("INSERT INTO t (v) VALUES (0)")
            .execute(&*pool)
            .await
            .expect("seed");

        let writer_acquired = Arc::new(Notify::new());
        let writer_acquired_inner = writer_acquired.clone();
        let pool_writer = pool.clone();

        let writer = tokio::spawn(async move {
            let mut tx = pool_writer.begin().await.expect("begin");
            sqlx::query("INSERT INTO t (v) VALUES (1)")
                .execute(&mut *tx)
                .await
                .expect("insert");
            writer_acquired_inner.notify_one();
            tokio::time::sleep(Duration::from_millis(500)).await;
            tx.commit().await.expect("commit");
        });

        writer_acquired.notified().await;

        let start = Instant::now();
        let readers = (0..8).map(|_| {
            let p = pool.clone();
            tokio::spawn(async move {
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM t")
                    .fetch_one(&*p)
                    .await
            })
        });
        let results = futures::future::join_all(readers).await;
        let elapsed = start.elapsed();

        writer.await.expect("writer task");
        for r in results {
            r.expect("reader task").expect("reader query");
        }

        assert!(
            elapsed < Duration::from_millis(200),
            "8 concurrent SELECTs took {:?} while writer held a 500ms tx",
            elapsed
        );
    }

    /// A second writer issued while a first writer holds a long-running
    /// transaction succeeds (the pool retries via busy_timeout) rather than
    /// failing with SQLITE_BUSY / "database is locked".
    #[tokio::test]
    async fn concurrent_writer_does_not_fail_with_database_is_locked() {
        let pool = fresh_pool().await;
        create_t(&pool).await;

        let long_writer_started = Arc::new(Notify::new());
        let long_writer_started_inner = long_writer_started.clone();
        let pool_long = pool.clone();

        let long_writer = tokio::spawn(async move {
            let mut tx = pool_long.begin().await.expect("begin");
            sqlx::query("INSERT INTO t (v) VALUES (1)")
                .execute(&mut *tx)
                .await
                .expect("insert");
            long_writer_started_inner.notify_one();
            for _ in 0..10 {
                tokio::time::sleep(Duration::from_millis(100)).await;
                sqlx::query("INSERT INTO t (v) VALUES (1)")
                    .execute(&mut *tx)
                    .await
                    .expect("insert in loop");
            }
            tx.commit().await.expect("commit");
        });

        long_writer_started.notified().await;

        let start = Instant::now();
        let result = sqlx::query("INSERT INTO t (v) VALUES (2)")
            .execute(&*pool)
            .await;
        let elapsed = start.elapsed();

        long_writer.await.expect("long writer task");

        assert!(
            result.is_ok(),
            "concurrent INSERT failed after {:?}: {:?}",
            elapsed,
            result.err()
        );
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
}
