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

#[cfg_attr(docsrs, doc(cfg(any(feature = "cloud", feature = "standalone"))))]
#[cfg(any(feature = "cloud", feature = "standalone"))]
pub mod v1_1;

use serde::Deserialize;

#[cfg(feature = "cloud")]
type Pool = crate::infra::pool::postgres::PostgresPool;

#[cfg(feature = "standalone")]
type Pool = crate::infra::pool::sqlite::SqlitePool;

/// The pool the ledger DB was initialized with. Storage-core keeps its copy private, and
/// maintenance SQL like [v1_1::LedgerDb::delete_roots] needs direct access.
#[cfg(any(feature = "cloud", feature = "standalone"))]
static POOL: std::sync::OnceLock<Pool> = std::sync::OnceLock::new();

#[cfg(any(feature = "cloud", feature = "standalone"))]
pub(crate) fn pool() -> Pool {
    POOL.get().expect("ledger DB is initialized").clone()
}

#[cfg(feature = "cloud")]
pub fn init(config: Config, pool: crate::infra::pool::postgres::PostgresPool) {
    let Config { cache_max_nodes } = config;

    let _ = POOL.set(pool.clone());
    let db = v1_1::LedgerDb::new(pool);
    let _ = midnight_storage_core_v1::storage::set_default_storage(|| {
        midnight_storage_core_v1::Storage::new(cache_max_nodes, db)
    });
}

#[cfg(feature = "standalone")]
pub async fn init(config: Config) -> Result<(), Error> {
    use crate::infra::{migrations, pool::sqlite};
    use log::warn;

    let Config {
        cache_max_nodes,
        cnn_url,
        vacuum_on_startup,
    } = config;

    // storage-core assumes a single writer: `flush_*` reads root counts and
    // then writes new ones, and that read-then-write must observe its own
    // in-progress state. With max_connections > 1, sqlx can route the read to
    // a different connection whose WAL snapshot predates the writer, breaking
    // the invariant and producing "roots counts can't be negative" panics.
    //
    // `synchronous_full`: chain-indexer commits the ledger state BEFORE the block row in the
    // main DB and the resume path relies on the on-disk ledger DB being at least as new as
    // the main DB. With NORMAL both files fsync independently at checkpoints, so a power
    // loss could keep block N in the main DB while dropping N's ledger state here - the
    // startup filter would then silently seed from an older state and diverge. FULL keeps
    // the cross-file ordering: a ledger state is durable before its block row can be.
    let mut pool = sqlite::SqlitePool::new(sqlite::Config {
        cnn_url: cnn_url.clone(),
        max_connections: 1,
        synchronous_full: true,
    })
    .await?;
    migrations::sqlite::run_for_ledger_db(&pool).await?;

    // Nodes culled by gc only land on SQLite's freelist; giving the pages back to the OS takes
    // a vacuum, run here while nothing else uses the pool. A failed copy (e.g. no disk
    // headroom) just postpones the shrink to a later startup; only a failure after the
    // compacted copy was swapped in panics.
    if vacuum_on_startup {
        match vacuum_if_mostly_free(pool, &cnn_url).await {
            Ok(vacuumed_pool) => pool = vacuumed_pool,
            Err((error, recovered_pool)) => {
                warn!(error:%; "cannot vacuum ledger DB");
                pool = recovered_pool;
            }
        }
    }

    let _ = POOL.set(pool.clone());
    let db = v1_1::LedgerDb::new(pool);
    let _ = midnight_storage_core_v1::storage::set_default_storage(|| {
        midnight_storage_core_v1::Storage::new(cache_max_nodes, db)
    });

    Ok(())
}

/// Compact the ledger DB when the freelist dominates the file: at least twice the live pages,
/// and worth at least 1 GiB. `VACUUM INTO` a sibling file (same volume - the system temp dir
/// may be a smaller filesystem) then swap it in, so live pages are copied once, not twice.
/// A copy left behind by a killed run is removed and rebuilt. Steady-state startups, whose
/// freelist stays small, never pay any of this.
///
/// Returns the pool to continue with; on error the original file stays in place and the
/// returned pool points at it.
#[cfg(feature = "standalone")]
async fn vacuum_if_mostly_free(
    pool: crate::infra::pool::sqlite::SqlitePool,
    cnn_url: &str,
) -> Result<
    crate::infra::pool::sqlite::SqlitePool,
    (sqlx::Error, crate::infra::pool::sqlite::SqlitePool),
> {
    use log::info;

    let stats = async {
        let page_size = sqlx::query_scalar::<_, i64>("PRAGMA page_size")
            .fetch_one(&*pool)
            .await?;
        let page_count = sqlx::query_scalar::<_, i64>("PRAGMA page_count")
            .fetch_one(&*pool)
            .await?;
        let freelist_count = sqlx::query_scalar::<_, i64>("PRAGMA freelist_count")
            .fetch_one(&*pool)
            .await?;
        let db_path = sqlx::query_scalar::<_, String>(
            "SELECT file FROM pragma_database_list WHERE name = 'main'",
        )
        .fetch_one(&*pool)
        .await?;
        let journal_mode = sqlx::query_scalar::<_, String>("PRAGMA journal_mode")
            .fetch_one(&*pool)
            .await?;

        Ok::<_, sqlx::Error>((page_size, page_count, freelist_count, db_path, journal_mode))
    }
    .await;
    let (page_size, page_count, freelist_count, db_path, journal_mode) = match stats {
        Ok(stats) => stats,
        Err(error) => return Err((error, pool)),
    };

    // A copy left behind by a killed earlier run; remove it also when the thresholds below
    // no longer fire.
    let _ = std::fs::remove_file(format!("{db_path}.vacuum"));

    if !should_vacuum(page_size, page_count, freelist_count) {
        return Ok(pool);
    }

    info!(
        page_count,
        freelist_count;
        "vacuuming ledger DB; this copies all live pages and can take a while"
    );
    let pool = vacuum_into_sibling(pool, cnn_url, &db_path, &journal_mode).await?;
    info!("vacuumed ledger DB");

    Ok(pool)
}

/// `VACUUM INTO` a sibling of the DB file and swap it in, returning a pool on the compacted
/// file. On copy failure the original file and pool stay in place; a failure after the swap
/// panics, since the half-finished state must not be built on silently.
#[cfg(feature = "standalone")]
async fn vacuum_into_sibling(
    pool: crate::infra::pool::sqlite::SqlitePool,
    cnn_url: &str,
    db_path: &str,
    journal_mode: &str,
) -> Result<
    crate::infra::pool::sqlite::SqlitePool,
    (sqlx::Error, crate::infra::pool::sqlite::SqlitePool),
> {
    use crate::infra::pool::sqlite;

    let compacted_path = format!("{db_path}.vacuum");

    if let Err(error) = sqlx::query("VACUUM INTO $1")
        .bind(&compacted_path)
        .execute(&*pool)
        .await
    {
        let _ = std::fs::remove_file(&compacted_path);
        return Err((error, pool));
    }

    // Swap in the compacted copy. Closing the pool checkpoints and removes the WAL; the
    // leftovers are removed before the rename anyway so the swapped-in file cannot see a
    // stale WAL and nothing lingers.
    pool.close().await;
    let _ = std::fs::remove_file(format!("{db_path}-wal"));
    let _ = std::fs::remove_file(format!("{db_path}-shm"));
    std::fs::rename(&compacted_path, db_path).expect("cannot swap compacted ledger DB into place");

    // Reopen with the same constraints as `init`: single writer and synchronous=FULL.
    let pool = sqlite::SqlitePool::new(sqlite::Config {
        cnn_url: cnn_url.to_owned(),
        max_connections: 1,
        synchronous_full: true,
    })
    .await
    .unwrap_or_else(|error| panic!("cannot reopen vacuumed ledger DB: {error}"));

    // VACUUM INTO always writes a rollback-journal file; restore the original mode (WAL in
    // production) so the swap does not silently downgrade write performance.
    sqlx::query_scalar::<_, String>(&format!("PRAGMA journal_mode = {journal_mode}"))
        .fetch_one(&*pool)
        .await
        .unwrap_or_else(|error| panic!("cannot restore ledger DB journal mode: {error}"));

    Ok(pool)
}

#[cfg(feature = "standalone")]
fn should_vacuum(page_size: i64, page_count: i64, freelist_count: i64) -> bool {
    const GIB: i64 = 1 << 30;
    freelist_count * page_size >= GIB && freelist_count >= 2 * (page_count - freelist_count)
}

#[cfg(all(test, feature = "standalone"))]
mod tests {
    use super::{should_vacuum, vacuum_into_sibling};
    use crate::infra::{migrations, pool::sqlite};

    const PAGE_SIZE: i64 = 4096;
    const GIB_PAGES: i64 = (1 << 30) / PAGE_SIZE;

    /// The copy-and-swap keeps the data, reopens on the compacted file, and leaves no
    /// temporary sibling behind.
    #[tokio::test]
    async fn vacuum_into_sibling_swaps_and_keeps_data() {
        let temp_dir = tempfile::tempdir().expect("create tempdir");
        let db_path = temp_dir
            .path()
            .join("ledger-db.sqlite")
            .display()
            .to_string();

        let pool = sqlite::SqlitePool::new(sqlite::Config {
            cnn_url: db_path.clone(),
            max_connections: 1,
            synchronous_full: true,
        })
        .await
        .expect("create pool");
        migrations::sqlite::run_for_ledger_db(&pool)
            .await
            .expect("run migrations");
        sqlx::query("INSERT INTO ledger_db_roots (key, count) VALUES ($1, 7)")
            .bind([0xab_u8; 32].as_slice())
            .execute(&*pool)
            .await
            .expect("insert root");

        let pool = vacuum_into_sibling(pool, &db_path, &db_path, "wal")
            .await
            .expect("vacuum into sibling");

        let count =
            sqlx::query_scalar::<_, i64>("SELECT count FROM ledger_db_roots WHERE key = $1")
                .bind([0xab_u8; 32].as_slice())
                .fetch_one(&*pool)
                .await
                .expect("read root back");
        assert_eq!(count, 7);
        assert!(!std::path::Path::new(&format!("{db_path}.vacuum")).exists());

        // The compacted copy is rollback-journal; the swap must restore the original mode.
        let journal_mode = sqlx::query_scalar::<_, String>("PRAGMA journal_mode")
            .fetch_one(&*pool)
            .await
            .expect("read journal mode");
        assert_eq!(journal_mode, "wal");
    }

    #[test]
    fn vacuums_only_when_mostly_free_and_worthwhile() {
        // The post-purge shape: freelist dwarfs the live pages.
        assert!(should_vacuum(PAGE_SIZE, 50 * GIB_PAGES, 49 * GIB_PAGES));

        // Steady state: file large, freelist small.
        assert!(!should_vacuum(PAGE_SIZE, 50 * GIB_PAGES, GIB_PAGES));

        // Mostly free but tiny: not worth a rewrite below 1 GiB reclaimed.
        assert!(!should_vacuum(PAGE_SIZE, 300, 299));

        // Exactly at both bounds: freelist worth 1 GiB and twice the live pages.
        assert!(should_vacuum(
            PAGE_SIZE,
            GIB_PAGES + GIB_PAGES / 2,
            GIB_PAGES
        ));
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Maximum number of arena nodes held in the storage-core caches. This is a node *count*, not
    /// a byte size: storage-core's read cache is strictly bounded by it and its write cache is
    /// truncated to it on flush (see `midnight_storage_core_v1::Storage::new`, whose own default
    /// is `DEFAULT_CACHE_SIZE = 10_000`). `0` means unbounded.
    pub cache_max_nodes: usize,

    #[cfg(feature = "standalone")]
    pub cnn_url: String,

    /// Compact the ledger DB at startup when gc has freed most of the file (see
    /// `vacuum_if_mostly_free`). The compaction can take a while on a large backlog and runs
    /// before anything else starts, so supervisors with tight startup timeouts may want this
    /// off.
    #[cfg(feature = "standalone")]
    #[serde(default = "default_vacuum_on_startup")]
    pub vacuum_on_startup: bool,
}

#[cfg(feature = "standalone")]
fn default_vacuum_on_startup() -> bool {
    true
}

#[cfg(feature = "standalone")]
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot create DB pool for SQLite")]
    CreatePool(#[from] crate::infra::pool::sqlite::Error),

    #[error("cannot run migrations for SQLite")]
    RunMigrations(#[from] crate::infra::migrations::sqlite::Error),
}
