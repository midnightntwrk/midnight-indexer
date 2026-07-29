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
pub fn init(config: Config, pool: crate::infra::pool::postgres::PostgresPool) {
    let Config { cache_max_nodes } = config;

    let _ = OBSERVER.set(v1_1::LedgerDb::new(pool.clone()));

    let db = v1_1::LedgerDb::new(pool);
    let _ = midnight_storage_core_v1::storage::set_default_storage(|| {
        midnight_storage_core_v1::Storage::new(cache_max_nodes, db)
    });
}

#[cfg(feature = "standalone")]
pub async fn init(config: Config) -> Result<(), Error> {
    use crate::infra::{migrations, pool::sqlite};

    let Config {
        cache_max_nodes,
        cnn_url,
    } = config;

    let pool = sqlite::SqlitePool::new(sqlite::Config { cnn_url }).await?;
    migrations::sqlite::run_for_ledger_db(&pool).await?;

    let _ = OBSERVER.set(v1_1::LedgerDb::new(pool.clone()));

    let db = v1_1::LedgerDb::new(pool);
    let _ = midnight_storage_core_v1::storage::set_default_storage(|| {
        midnight_storage_core_v1::Storage::new(cache_max_nodes, db)
    });

    Ok(())
}

/// A second handle on the ledger DB, held only for observability: storage-core keeps its own `DB`
/// private behind `StorageBackend`, so there is no way to ask the arena how many rows its database
/// has.
#[cfg_attr(docsrs, doc(cfg(any(feature = "cloud", feature = "standalone"))))]
#[cfg(any(feature = "cloud", feature = "standalone"))]
static OBSERVER: std::sync::OnceLock<v1_1::LedgerDb> = std::sync::OnceLock::new();

/// The number of rows in `ledger_db_nodes`, or `None` before [init] has run.
///
/// This is a full count, not an estimate, so it is proportional to the size of the arena and has no
/// business running per block on a large network. Sample it on an interval; see chain-indexer's
/// `arena_metrics_interval`.
#[cfg_attr(docsrs, doc(cfg(any(feature = "cloud", feature = "standalone"))))]
#[cfg(any(feature = "cloud", feature = "standalone"))]
pub fn node_count() -> Option<usize> {
    use midnight_storage_core_v1::db::DB;

    OBSERVER.get().map(|db| db.size())
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
}

#[cfg(feature = "standalone")]
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot create DB pool for SQLite")]
    CreatePool(#[from] crate::infra::pool::sqlite::Error),

    #[error("cannot run migrations for SQLite")]
    RunMigrations(#[from] crate::infra::migrations::sqlite::Error),
}
