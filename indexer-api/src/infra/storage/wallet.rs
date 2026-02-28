// This file is part of midnight-indexer.
// Copyright (C) 2025-2026 Midnight Foundation
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

use crate::{domain::storage::wallet::WalletStorage, infra::storage::Storage};
use chacha20poly1305::aead::{OsRng, rand_core::RngCore};
use fastrace::trace;
use futures::TryFutureExt;
use indexer_common::domain::{SessionId, ViewingKey};
use indoc::indoc;
use sqlx::types::{Uuid, time::OffsetDateTime};

impl WalletStorage for Storage {
    #[trace]
    async fn connect_wallet(&self, viewing_key: &ViewingKey) -> Result<SessionId, sqlx::Error> {
        let id = Uuid::now_v7();
        let viewing_key_hash = viewing_key.hash();
        let session_id = generate_session_id();
        let viewing_key = viewing_key
            .encrypt(id, &self.cipher)
            .map_err(|error| sqlx::Error::Encode(error.into()))?;

        let query = indoc! {"
            INSERT INTO wallets (
                id,
                viewing_key_hash,
                viewing_key,
                last_active,
                session_id
            )
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (viewing_key_hash)
            DO UPDATE SET last_active = $4, session_id = $5
        "};

        sqlx::query(query)
            .bind(id)
            .bind(viewing_key_hash.as_ref())
            .bind(&viewing_key)
            .bind(OffsetDateTime::now_utc())
            .bind(session_id.as_ref())
            .execute(&*self.pool)
            .await?;

        Ok(session_id)
    }

    #[trace]
    async fn disconnect_wallet(&self, session_id: SessionId) -> Result<(), sqlx::Error> {
        let query = indoc! {"
            UPDATE wallets
            SET session_id = NULL
            WHERE session_id = $1
        "};

        sqlx::query(query)
            .bind(session_id.as_ref())
            .execute(&*self.pool)
            .await?;

        Ok(())
    }

    #[trace]
    async fn resolve_session_id(&self, session_id: SessionId) -> Result<Option<Uuid>, sqlx::Error> {
        let query = indoc! {"
            SELECT id
            FROM wallets
            WHERE session_id = $1
        "};

        sqlx::query_scalar::<_, Uuid>(query)
            .bind(session_id.as_ref())
            .fetch_optional(&*self.pool)
            .await
    }

    #[trace(properties = { "wallet_id": "{wallet_id}" })]
    async fn keep_wallet_active(&self, wallet_id: Uuid) -> Result<(), sqlx::Error> {
        let query = indoc! {"
            UPDATE wallets
            SET last_active = $1
            WHERE id = $2
            AND session_id IS NOT NULL
        "};

        let result = sqlx::query(query)
            .bind(OffsetDateTime::now_utc())
            .bind(wallet_id)
            .execute(&*self.pool)
            .map_ok(|_| ())
            .await;

        #[cfg(feature = "cloud")]
        let result = result.or_else(|error| {
            indexer_common::infra::sqlx::postgres::ignore_deadlock_detected(error, || ())
        });

        result
    }
}

fn generate_session_id() -> SessionId {
    let mut session_id = [0u8; 32];
    OsRng.fill_bytes(&mut session_id);
    session_id.into()
}
