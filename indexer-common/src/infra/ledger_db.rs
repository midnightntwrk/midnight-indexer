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
pub mod v7_0_0;

#[cfg(feature = "cloud")]
pub fn init(config: Config, pool: crate::infra::pool::postgres::PostgresPool) {
    let Config { cache_size } = config;

    let db = v7_0_0::LedgerDb::new(pool);
    let _ = midnight_storage_v7_0_0::storage::set_default_storage(|| {
        midnight_storage_v7_0_0::Storage::new(cache_size, db)
    });
}

#[cfg(feature = "standalone")]
pub fn init(config: Config, pool: crate::infra::pool::sqlite::SqlitePool) {
    let Config { cache_size } = config;

    let db = v7_0_0::LedgerDb::new(pool);
    let _ = midnight_storage_v7_0_0::storage::set_default_storage(|| {
        midnight_storage_v7_0_0::Storage::new(cache_size, db)
    });
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub cache_size: usize,
}
