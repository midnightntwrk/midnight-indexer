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

mod block;
mod contract_action;
mod transaction;
mod unshielded_utxo;
mod wallet;

use crate::domain::storage::Storage;
use chacha20poly1305::ChaCha20Poly1305;
use derive_more::Debug;
use futures::stream::TryStreamExt;
use indexer_common::{domain::Identifier, infra::pool::sqlite::SqlitePool};
use indoc::indoc;
use sqlx::{Database, Row, Sqlite};

/// Sqlite based implementation of [Storage].
#[derive(Debug, Clone)]
pub struct SqliteStorage {
    #[debug(skip)]
    cipher: ChaCha20Poly1305,
    pool: SqlitePool,
}

impl SqliteStorage {
    /// Create a new [SqliteStorage].
    pub fn new(cipher: ChaCha20Poly1305, pool: SqlitePool) -> Self {
        Self { cipher, pool }
    }

    async fn get_identifiers_by_transaction_id(
        &self,
        id: u64,
    ) -> Result<Vec<Identifier>, sqlx::Error> {
        let query = indoc! {"
            SELECT identifier
            FROM transaction_identifiers
            WHERE transaction_id = $1
        "};

        let identifiers = sqlx::query(query)
            .bind(id as i64)
            .try_map(|row: <Sqlite as Database>::Row| Ok(row.try_get::<Vec<u8>, _>(0)?.into()))
            .fetch(&*self.pool)
            .try_collect::<Vec<_>>()
            .await?;

        Ok(identifiers)
    }
}

impl Storage for SqliteStorage {}
