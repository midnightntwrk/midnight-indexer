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

mod block;
mod contract_action;
pub mod dataloader;
mod dust;
mod ledger_events;
mod ledger_state;
mod spo;
mod system_parameters;
mod transaction;
mod unshielded;
mod wallet;

use crate::domain;
use chacha20poly1305::ChaCha20Poly1305;

#[cfg(feature = "cloud")]
use indexer_common::infra::pool::postgres::PostgresPool;
#[cfg(feature = "standalone")]
use indexer_common::infra::pool::sqlite::SqlitePool;

/// Unified storage implementation for PostgreSQL (cloud) and SQLite (standalone). Uses Cargo
/// features to select the appropriate database backend at build time.
#[derive(Clone)]
pub struct Storage {
    cipher: ChaCha20Poly1305,
    #[cfg(feature = "cloud")]
    pool: PostgresPool,
    #[cfg(feature = "standalone")]
    pool: SqlitePool,
}

impl Storage {
    #[cfg(feature = "cloud")]
    pub fn new(cipher: ChaCha20Poly1305, pool: PostgresPool) -> Self {
        Self { cipher, pool }
    }

    #[cfg(feature = "standalone")]
    pub fn new(cipher: ChaCha20Poly1305, pool: SqlitePool) -> Self {
        Self { cipher, pool }
    }
}

impl domain::storage::Storage for Storage {}
