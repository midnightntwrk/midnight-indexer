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

use crate::{
    domain::{Transaction, Wallet, storage::Storage},
    infra::storage::{self},
};
use chacha20poly1305::ChaCha20Poly1305;
use fastrace::trace;
use futures::TryStreamExt;
use indexer_common::{
    domain::{SessionId, ViewingKey},
    infra::{pool::postgres::PostgresPool, sqlx::postgres::map_deadlock_detected},
};
use indoc::indoc;
use sqlx::{
    Postgres, QueryBuilder, Row,
    types::{Uuid, time::OffsetDateTime},
};
use std::{num::NonZeroUsize, time::Duration};

type Tx = sqlx::Transaction<'static, Postgres>;

/// Postgres based implementation of [Storage].
#[derive(Clone)]
pub struct PostgresStorage {
    cipher: ChaCha20Poly1305,
    pool: PostgresPool,
}

impl PostgresStorage {
    /// Create a new [PostgresStorage].
    pub fn new(cipher: ChaCha20Poly1305, pool: PostgresPool) -> Self {
        Self { cipher, pool }
    }
}

impl Storage for PostgresStorage {
    type Database = sqlx::Postgres;

    #[trace(properties = { "session_id": "{session_id}" })]
    async fn acquire_lock(&mut self, session_id: SessionId) -> Result<Option<Tx>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        let lock_acquired =
            sqlx::query("SELECT pg_try_advisory_xact_lock(hashtextextended($1, 0))")
                .bind(const_hex::encode(session_id))
                .fetch_one(&mut *tx)
                .await
                .and_then(|row| row.try_get::<bool, _>(0))?;

        Ok(lock_acquired.then_some(tx))
    }

    #[trace(properties = { "session_id": "{session_id}" })]
    async fn get_wallet(
        &self,
        session_id: SessionId,
        tx: &mut Tx,
    ) -> Result<Option<Wallet>, sqlx::Error> {
        let query = indoc! {"
            SELECT id, viewing_key, last_indexed_transaction_id, last_active
            FROM wallets
            WHERE session_id = $1
        "};

        let wallet = sqlx::query_as::<_, storage::Wallet>(query)
            .bind(session_id)
            .fetch_optional(&mut **tx)
            .await?
            .map(|wallet| {
                Wallet::try_from((wallet, &self.cipher))
                    .map_err(|error| sqlx::Error::Decode(error.into()))
            })
            .transpose()?;

        Ok(wallet)
    }

    #[trace(properties = { "from": "{from}", "limit": "{limit}" })]
    async fn get_transactions(
        &self,
        from: u64,
        limit: NonZeroUsize,
        tx: &mut Tx,
    ) -> Result<Vec<Transaction>, sqlx::Error> {
        let query = indoc! {"
            SELECT id, raw
            FROM transactions
            WHERE id >= $1
            ORDER BY id ASC
            LIMIT $2
        "};

        sqlx::query_as(query)
            .bind(from as i64)
            .bind(limit.get() as i32)
            .fetch_all(&mut **tx)
            .await
    }

    #[trace]
    async fn save_relevant_transactions(
        &self,
        viewing_key: &ViewingKey,
        transactions: &[Transaction],
        last_indexed_transaction_id: u64,
        tx: &mut Tx,
    ) -> Result<(), sqlx::Error> {
        let id = Uuid::now_v7();
        let session_id = viewing_key.to_session_id();
        let viewing_key = viewing_key
            .encrypt(id, &self.cipher)
            .map_err(|error| sqlx::Error::Encode(error.into()))?;

        let query = indoc! {"
            INSERT INTO wallets (
                id,
                session_id,
                viewing_key,
                last_indexed_transaction_id,
                last_active
            )
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (session_id)
            DO UPDATE SET last_indexed_transaction_id = $4
            RETURNING id
        "};

        let wallet_id = sqlx::query(query)
            .bind(id)
            .bind(session_id)
            .bind(viewing_key)
            .bind(last_indexed_transaction_id as i64)
            .bind(OffsetDateTime::now_utc())
            .fetch_one(&mut **tx)
            .await?
            .try_get::<Uuid, _>("id")?;

        if !transactions.is_empty() {
            let query = indoc! {"
                INSERT INTO relevant_transactions (wallet_id, transaction_id)
            "};

            QueryBuilder::new(query)
                .push_values(transactions, |mut q, transaction| {
                    q.push_bind(wallet_id).push_bind(transaction.id as i64);
                })
                .build()
                .execute(&mut **tx)
                .await?;
        }

        Ok(())
    }

    #[trace]
    async fn active_wallets(&self, ttl: Duration) -> Result<Vec<ViewingKey>, sqlx::Error> {
        // Query wallets.
        let query = indoc! {"
            SELECT *
            FROM wallets
            WHERE active = TRUE
        "};

        let wallets = sqlx::query_as::<_, storage::Wallet>(query)
            .fetch(&*self.pool)
            .try_collect::<Vec<_>>()
            .await?;

        let now = OffsetDateTime::now_utc();

        // Mark inactive wallets.
        let outdated_ids = wallets
            .iter()
            .filter_map(|wallet| (now - wallet.last_active > ttl).then_some(wallet.id))
            .collect::<Vec<_>>();
        if !outdated_ids.is_empty() {
            let query = indoc! {"
                UPDATE wallets
                SET active = FALSE
                WHERE id = ANY($1)
            "};

            // This could cause a "deadlock_detected" error when the indexer-api sets a wallet
            // active at the same time. These errors can be ignored, because this operation will be
            // executed "very soon" again.
            sqlx::query(query)
                .bind(outdated_ids)
                .execute(&*self.pool)
                .await
                .map(|_| ())
                .or_else(|error| map_deadlock_detected(error, || ()))?;
        }

        // Return active viewing keys.
        let viewing_keys = wallets
            .into_iter()
            .filter_map(|wallet| {
                (now - wallet.last_active <= ttl).then_some(ViewingKey::decrypt(
                    &wallet.viewing_key,
                    wallet.id,
                    &self.cipher,
                ))
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| sqlx::Error::Decode(error.into()))?;

        Ok(viewing_keys)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        domain::{Wallet, storage::Storage},
        infra::storage::postgres::PostgresStorage,
    };
    use anyhow::Context;
    use assert_matches::assert_matches;
    use chacha20poly1305::{ChaCha20Poly1305, Key, KeyInit};
    use indexer_common::{
        domain::{ApplyStage, ViewingKey},
        infra::{
            migrations,
            pool::{self, postgres::PostgresPool},
        },
    };
    use indoc::indoc;
    use sqlx::{QueryBuilder, postgres::PgSslMode, types::time::OffsetDateTime};
    use std::{error::Error as StdError, iter};
    use testcontainers::{ImageExt, runners::AsyncRunner};
    use testcontainers_modules::postgres::Postgres;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_storage() -> Result<(), Box<dyn StdError>> {
        let postgres_container = Postgres::default()
            .with_db_name("indexer")
            .with_user("indexer")
            .with_password(env!("APP__INFRA__STORAGE__PASSWORD"))
            .with_tag("17.1-alpine")
            .start()
            .await
            .context("start Postgres container")?;
        let postgres_port = postgres_container
            .get_host_port_ipv4(5432)
            .await
            .context("get Postgres port")?;

        let config = pool::postgres::Config {
            host: "localhost".to_string(),
            port: postgres_port,
            dbname: "indexer".to_string(),
            user: "indexer".to_string(),
            password: env!("APP__INFRA__STORAGE__PASSWORD").into(),
            sslmode: PgSslMode::Prefer,
        };
        let pool = PostgresPool::new(config).await?;

        migrations::postgres::run(&pool).await?;

        // Fill DB with test data.

        let query = indoc! {"
            INSERT INTO blocks (
                hash,
                height,
                protocol_version,
                parent_hash,
                timestamp
            ) 
        "};
        QueryBuilder::new(query)
            .push_values(iter::once(1), |mut q, id| {
                q.push_bind(id.to_string().into_bytes())
                    .push_bind(id)
                    .push_bind(1_000)
                    .push_bind(0.to_string().into_bytes())
                    .push_bind(0);
            })
            .build()
            .execute(&*pool)
            .await?;

        let ids = 1..=100;
        let query = indoc! {"
            INSERT INTO transactions (
                block_id,
                hash,
                protocol_version,
                apply_stage,
                identifiers,
                raw,
                merkle_tree_root,
                start_index,
                end_index
            )
        "};
        QueryBuilder::new(query)
            .push_values(ids, |mut q, id| {
                q.push_bind(1)
                    .push_bind(id.to_string().into_bytes())
                    .push_bind(1_000)
                    .push_bind(ApplyStage::Success)
                    .push_bind([b"identifier"])
                    .push_bind(b"raw")
                    .push_bind(b"merkle_tree_root")
                    .push_bind(0)
                    .push_bind(1);
            })
            .build()
            .execute(&*pool)
            .await?;

        let cipher =
            ChaCha20Poly1305::new(&Key::clone_from_slice(b"01234567890123456789012345678901"));

        let viewing_key_a = ViewingKey::make_for_testing_yes_i_know_what_i_am_doing();
        let viewing_key_b = ViewingKey::make_for_testing_yes_i_know_what_i_am_doing();
        let session_id_a = viewing_key_a.to_session_id();
        let session_id_b = viewing_key_b.to_session_id();

        let uuid_a = Uuid::now_v7();
        let encrypted_viewing_key_a = viewing_key_a.encrypt(uuid_a, &cipher)?;
        let uuid_b = Uuid::now_v7();
        let encrypted_viewing_key_b = viewing_key_b.encrypt(uuid_b, &cipher)?;

        let wallets = [
            (uuid_a, encrypted_viewing_key_a, session_id_a, 1),
            (uuid_b, encrypted_viewing_key_b, session_id_b, 42),
        ];

        let query = indoc! {"
            INSERT INTO wallets (
                id,
                session_id,
                viewing_key,
                last_indexed_transaction_id,
                last_active
            )
        "};
        QueryBuilder::new(query)
            .push_values(
                wallets,
                |mut q, (id, viewing_key, session_id, last_indexed_transaction_id)| {
                    q.push_bind(id)
                        .push_bind(session_id)
                        .push_bind(viewing_key)
                        .push_bind(last_indexed_transaction_id)
                        .push_bind(OffsetDateTime::now_utc());
                },
            )
            .build()
            .execute(&*pool)
            .await?;

        // Start the actual testing.

        let mut storage = PostgresStorage::new(cipher, pool);

        let tx = storage.acquire_lock(session_id_b).await?;
        assert!(tx.is_some());
        let mut tx = tx.unwrap();

        let wallet = storage.get_wallet([0; 32].into(), &mut tx).await?;
        assert!(wallet.is_none());
        let wallet = storage.get_wallet(session_id_b, &mut tx).await?;
        assert_matches!(
            wallet,
            Some(Wallet {
                last_indexed_transaction_id: 42,
                ..
            })
        );

        let transactions = storage
            .get_transactions(42, 10.try_into()?, &mut tx)
            .await?;
        assert_eq!(
            transactions.iter().map(|t| t.id).collect::<Vec<_>>(),
            (42..52).collect::<Vec<_>>()
        );

        storage
            .save_relevant_transactions(&viewing_key_b, &transactions[0..5], 51, &mut tx)
            .await?;

        tx.commit().await?;

        let tx = storage.acquire_lock(session_id_b).await?;
        assert!(tx.is_some());
        let mut tx = tx.unwrap();

        let wallet = storage.get_wallet(session_id_b, &mut tx).await?;
        assert_matches!(
            wallet,
            Some(Wallet {
                last_indexed_transaction_id: 51,
                ..
            })
        );

        let relevant_transactions = sqlx::query_as::<_, (Uuid, i64)>(
            "SELECT wallet_id, transaction_id
             FROM relevant_transactions",
        )
        .fetch_all(&mut *tx)
        .await?;

        assert_eq!(
            relevant_transactions,
            (42..47).map(|tx_id| (uuid_b, tx_id)).collect::<Vec<_>>()
        );

        Ok(())
    }
}
