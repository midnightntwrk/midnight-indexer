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

use crate::{
    domain::{SessionToken, storage::wallet::WalletStorage},
    infra::storage::Storage,
};
use chacha20poly1305::aead::{OsRng, rand_core::RngCore};
use fastrace::trace;
use futures::TryFutureExt;
use indexer_common::domain::{ByteVec, SessionId, ViewingKey};
use indoc::indoc;
use sqlx::types::{Uuid, time::OffsetDateTime};
use std::time::Duration;

impl WalletStorage for Storage {
    #[trace]
    async fn connect_wallet(
        &self,
        viewing_key: &ViewingKey,
        start_index: Option<u64>,
    ) -> Result<(Uuid, ByteVec), sqlx::Error> {
        let wallet_id = self.upsert_wallet(viewing_key, start_index).await?;

        let token = SessionToken {
            viewing_key: *viewing_key,
            start_index: start_index.unwrap_or_default(),
            issued_at: OffsetDateTime::now_utc().unix_timestamp(),
        }
        .seal(&self.cipher)
        .map_err(|error| sqlx::Error::Encode(error.into()))?;

        Ok((wallet_id, token))
    }

    #[trace]
    async fn disconnect_wallet(
        &self,
        session: &[u8],
        token_ttl: Duration,
    ) -> Result<bool, sqlx::Error> {
        match SessionToken::open(session, &self.cipher, token_ttl) {
            // Deliberate no-op: a self-contained token cannot be revoked on other instances, so
            // it is not revoked here either. Other sessions on the same viewing key stay active;
            // the wallet drops out of the active set once the inactivity TTL lapses.
            Ok(_) => Ok(true),

            // Legacy random session ID; remove this fallback after a deprecation period.
            Err(_) if session.len() == 32 => {
                let query = indoc! {"
                    UPDATE wallets
                    SET session_id = NULL
                    WHERE session_id = $1
                    RETURNING id
                "};

                sqlx::query_scalar::<_, Uuid>(query)
                    .bind(session)
                    .fetch_optional(&*self.pool)
                    .await
                    .map(|id| id.is_some())
            }

            Err(_) => Ok(false),
        }
    }

    #[trace]
    async fn resolve_session(
        &self,
        session: &[u8],
        token_ttl: Duration,
    ) -> Result<Option<Uuid>, sqlx::Error> {
        match SessionToken::open(session, &self.cipher, token_ttl) {
            Ok(token) => self
                .upsert_wallet(&token.viewing_key, Some(token.start_index))
                .await
                .map(Some),

            // Legacy random session ID; remove this fallback after a deprecation period.
            Err(_) if session.len() == 32 => {
                let query = indoc! {"
                    SELECT id
                    FROM wallets
                    WHERE session_id = $1
                "};

                sqlx::query_scalar::<_, Uuid>(query)
                    .bind(session)
                    .fetch_optional(&*self.pool)
                    .await
            }

            Err(_) => Ok(None),
        }
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

impl Storage {
    /// Insert the wallet or mark the existing one active, and return its ID. The stored random
    /// `session_id` only serves the wallet-indexer's activity check and the legacy session ID
    /// lookup; it is kept if already set so concurrent sessions do not invalidate each other.
    async fn upsert_wallet(
        &self,
        viewing_key: &ViewingKey,
        start_index: Option<u64>,
    ) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::now_v7();
        let viewing_key_hash = viewing_key.hash();
        let session_id = generate_session_id();
        let viewing_key = viewing_key
            .encrypt(id, &self.cipher)
            .map_err(|error| sqlx::Error::Encode(error.into()))?;
        let start_index: i64 = start_index
            .unwrap_or(0)
            .try_into()
            .map_err(|error| sqlx::Error::Encode(Box::new(error)))?;

        let query = indoc! {"
            INSERT INTO wallets (
                id,
                viewing_key_hash,
                viewing_key,
                wanted_start_index,
                first_indexed_transaction_id,
                last_indexed_transaction_id,
                last_active,
                session_id
            )
            VALUES ($1, $2, $3, $4, $4, $4, $5, $6)
            ON CONFLICT (viewing_key_hash)
            DO UPDATE SET
                last_active = $5,
                session_id = COALESCE(wallets.session_id, $6),
                wanted_start_index = CASE
                    WHEN wallets.wanted_start_index <= $4 THEN wallets.wanted_start_index
                    ELSE $4
                END
            RETURNING id
        "};

        sqlx::query_scalar::<_, Uuid>(query)
            .bind(id)
            .bind(viewing_key_hash.as_ref())
            .bind(&viewing_key)
            .bind(start_index)
            .bind(OffsetDateTime::now_utc())
            .bind(session_id.as_ref())
            .fetch_one(&*self.pool)
            .await
    }
}

fn generate_session_id() -> SessionId {
    let mut session_id = [0u8; 32];
    OsRng.fill_bytes(&mut session_id);
    session_id.into()
}

#[cfg(all(test, feature = "standalone"))]
mod tests {
    use super::*;
    use chacha20poly1305::{ChaCha20Poly1305, Key, KeyInit};
    use indexer_common::infra::{migrations, pool::sqlite::SqlitePool};
    use std::error::Error as StdError;

    const TTL: Duration = Duration::from_secs(60);

    async fn new_storage() -> Result<Storage, Box<dyn StdError>> {
        let pool = SqlitePool::new(Default::default()).await?;
        migrations::sqlite::run(&pool).await?;
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&[0u8; 32]));
        Ok(Storage::new(cipher, pool))
    }

    #[tokio::test]
    async fn token_works_across_instances() -> Result<(), Box<dyn StdError>> {
        let instance_a = new_storage().await?;
        // Separate database, same cipher key: simulates another indexer behind a load balancer.
        let instance_b = new_storage().await?;

        let viewing_key = ViewingKey::from([1; 32]);
        let (wallet_id, token) = instance_a.connect_wallet(&viewing_key, Some(7)).await?;

        // The issuing instance resolves the token to the same wallet.
        let resolved = instance_a.resolve_session(token.as_ref(), TTL).await?;
        assert_eq!(resolved, Some(wallet_id));

        // An instance that never saw the connect creates the wallet and resolves the token.
        let resolved = instance_b.resolve_session(token.as_ref(), TTL).await?;
        assert!(resolved.is_some());

        // Disconnect is accepted on that instance too.
        assert!(instance_b.disconnect_wallet(token.as_ref(), TTL).await?);

        Ok(())
    }

    #[tokio::test]
    async fn concurrent_sessions_share_one_wallet() -> Result<(), Box<dyn StdError>> {
        let storage = new_storage().await?;

        let viewing_key = ViewingKey::from([1; 32]);
        let (wallet_id_1, token_1) = storage.connect_wallet(&viewing_key, None).await?;
        let (wallet_id_2, token_2) = storage.connect_wallet(&viewing_key, None).await?;

        // One wallet row per viewing key, but both tokens resolve.
        assert_eq!(wallet_id_1, wallet_id_2);
        assert_ne!(token_1, token_2);
        assert_eq!(
            storage.resolve_session(token_1.as_ref(), TTL).await?,
            Some(wallet_id_1)
        );
        assert_eq!(
            storage.resolve_session(token_2.as_ref(), TTL).await?,
            Some(wallet_id_1)
        );

        // Disconnecting one session leaves the wallet active for the other.
        assert!(storage.disconnect_wallet(token_1.as_ref(), TTL).await?);
        let active: bool =
            sqlx::query_scalar("SELECT session_id IS NOT NULL FROM wallets WHERE id = $1")
                .bind(wallet_id_1)
                .fetch_one(&*storage.pool)
                .await?;
        assert!(active);
        assert_eq!(
            storage.resolve_session(token_2.as_ref(), TTL).await?,
            Some(wallet_id_1)
        );

        Ok(())
    }

    #[tokio::test]
    async fn invalid_sessions_do_not_resolve() -> Result<(), Box<dyn StdError>> {
        let storage = new_storage().await?;

        // Expired token.
        let expired_token = SessionToken {
            viewing_key: ViewingKey::from([1; 32]),
            start_index: 0,
            issued_at: OffsetDateTime::now_utc().unix_timestamp() - 100,
        }
        .seal(&storage.cipher)?;
        let expired = storage
            .resolve_session(expired_token.as_ref(), Duration::from_secs(10))
            .await?;
        assert_eq!(expired, None);
        assert!(
            !storage
                .disconnect_wallet(expired_token.as_ref(), Duration::from_secs(10))
                .await?
        );

        // Unknown legacy session ID.
        assert_eq!(storage.resolve_session(&[0; 32], TTL).await?, None);
        assert!(!storage.disconnect_wallet(&[0; 32], TTL).await?);

        // Garbage.
        assert_eq!(storage.resolve_session(&[42; 5], TTL).await?, None);

        Ok(())
    }

    #[tokio::test]
    async fn legacy_session_id_still_resolves() -> Result<(), Box<dyn StdError>> {
        let storage = new_storage().await?;

        let (wallet_id, _) = storage
            .connect_wallet(&ViewingKey::from([1; 32]), None)
            .await?;

        // Read back the stored random session ID like a pre-token client would hold it.
        let session_id: Vec<u8> =
            sqlx::query_scalar("SELECT session_id FROM wallets WHERE id = $1")
                .bind(wallet_id)
                .fetch_one(&*storage.pool)
                .await?;

        assert_eq!(
            storage.resolve_session(&session_id, TTL).await?,
            Some(wallet_id)
        );
        // Legacy disconnect removes the stored session ID.
        assert!(storage.disconnect_wallet(&session_id, TTL).await?);
        assert_eq!(storage.resolve_session(&session_id, TTL).await?, None);

        Ok(())
    }
}
