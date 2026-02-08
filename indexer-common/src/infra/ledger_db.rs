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

use serde::Deserialize;

#[cfg_attr(docsrs, doc(cfg(any(feature = "cloud", feature = "standalone"))))]
#[cfg(any(feature = "cloud", feature = "standalone"))]
pub mod v7;
#[cfg_attr(docsrs, doc(cfg(any(feature = "cloud", feature = "standalone"))))]
#[cfg(any(feature = "cloud", feature = "standalone"))]
pub mod v8;

#[cfg(feature = "cloud")]
pub fn init(config: Config, pool: crate::infra::pool::postgres::PostgresPool) {
    let Config { cache_size } = config;

    let db = v7::LedgerDb::new(pool);
    let _ = midnight_storage_core_v7::storage::set_default_storage(|| {
        midnight_storage_core_v7::Storage::new(cache_size, db)
    });
}

#[cfg(feature = "standalone")]
pub async fn init(config: Config) -> Result<(), Error> {
    use crate::infra::{migrations, pool::sqlite};

    let Config {
        cache_size,
        cnn_url,
    } = config;

    let pool = sqlite::SqlitePool::new(sqlite::Config { cnn_url }).await?;
    migrations::sqlite::run_for_ledger_db(&pool).await?;

    let db = v7::LedgerDb::new(pool);
    let _ = midnight_storage_core_v7::storage::set_default_storage(|| {
        midnight_storage_core_v7::Storage::new(cache_size, db)
    });

    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub cache_size: usize,

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
