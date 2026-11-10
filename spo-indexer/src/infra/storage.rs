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

use crate::domain::{
    self, Epoch, PoolMetadata, SPO, SPOEpochPerformance, SPOHistory, ValidatorMembership,
};
use fastrace::trace;
use indoc::indoc;
use sqlx::types::chrono::{DateTime, Utc};

#[cfg(feature = "cloud")]
/// Sqlx transaction for Postgres.
type SqlxTransaction = sqlx::Transaction<'static, sqlx::Postgres>;

#[cfg(feature = "standalone")]
/// Sqlx transaction for Sqlite.
type SqlxTransaction = sqlx::Transaction<'static, sqlx::Sqlite>;

/// Unified storage implementation for PostgreSQL (cloud) and SQLite (standalone). Uses Cargo
/// features to select the appropriate database backend at build time.
#[derive(Debug, Clone)]
pub struct Storage {
    #[cfg(feature = "cloud")]
    pool: indexer_common::infra::pool::postgres::PostgresPool,

    #[cfg(feature = "standalone")]
    pool: indexer_common::infra::pool::sqlite::SqlitePool,
}

impl Storage {
    #[cfg(feature = "cloud")]
    pub fn new(pool: indexer_common::infra::pool::postgres::PostgresPool) -> Self {
        Self { pool }
    }

    #[cfg(feature = "standalone")]
    pub fn new(pool: indexer_common::infra::pool::sqlite::SqlitePool) -> Self {
        Self { pool }
    }
}

impl domain::storage::Storage for Storage {
    #[trace]
    async fn create_tx(&self) -> Result<SqlxTransaction, sqlx::Error> {
        Ok(self.pool.begin().await?)
    }

    async fn get_latest_epoch(&self) -> Result<Option<Epoch>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                epoch_no,
                starts_at,
                ends_at
            FROM epochs
            ORDER BY epoch_no
            DESC LIMIT 1
        "};

        sqlx::query_as::<_, (i64, DateTime<Utc>, DateTime<Utc>)>(query)
            .fetch_optional(&*self.pool)
            .await?
            .map(|(epoch_no, starts_at, ends_at)| {
                Ok(Epoch {
                    epoch_no: epoch_no as u32,
                    // Return millis to domain.
                    starts_at: starts_at.timestamp_millis(),
                    ends_at: ends_at.timestamp_millis(),
                })
            })
            .transpose()
    }

    #[trace]
    async fn save_epoch(&self, epoch: &Epoch, tx: &mut SqlxTransaction) -> Result<(), sqlx::Error> {
        sqlx::query(indoc! {
            "INSERT INTO epochs (epoch_no, starts_at, ends_at)
             VALUES ($1, $2, $3)"
        })
        .bind(epoch.epoch_no as i64)
        // Epoch starts_at/ends_at are in millis; store as timestamptz.
        .bind(
            DateTime::from_timestamp(
                epoch.starts_at / 1000,
                ((epoch.starts_at % 1000) * 1_000_000) as u32,
            )
            .unwrap_or_default(),
        )
        .bind(
            DateTime::from_timestamp(
                epoch.ends_at / 1000,
                ((epoch.ends_at % 1000) * 1_000_000) as u32,
            )
            .unwrap_or_default(),
        )
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    #[trace]
    async fn save_spo(&self, spo: &SPO, tx: &mut SqlxTransaction) -> Result<(), sqlx::Error> {
        sqlx::query(indoc! {
            "INSERT INTO spo_identity (
                spo_sk,
                sidechain_pubkey,
                pool_id,
                mainchain_pubkey,
                aura_pubkey
            )
            SELECT $1, $2, $3, $4, $5
            WHERE NOT EXISTS (
                SELECT 1 FROM spo_identity si
                WHERE si.spo_sk = $1
                   OR (si.mainchain_pubkey IS NOT DISTINCT FROM $4)
                   OR (si.aura_pubkey IS NOT DISTINCT FROM $5)
                   OR (si.sidechain_pubkey IS NOT DISTINCT FROM $2)
            )
            ON CONFLICT DO NOTHING"
        })
        .bind(&spo.spo_sk)
        .bind(&spo.sidechain_pubkey)
        .bind(&spo.pool_id)
        .bind(&spo.mainchain_pubkey)
        .bind(&spo.aura_pubkey)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    #[trace]
    async fn remove_unverified_spos(
        &self,
        spo: &SPO,
        tx: &mut SqlxTransaction,
    ) -> Result<Vec<String>, sqlx::Error> {
        // Matches every row that would make `save_spo` skip `spo`, except `spo` itself.
        let query = indoc! {"
            SELECT si.spo_sk FROM spo_identity si
            WHERE (
                si.spo_sk = $1
                OR si.sidechain_pubkey IS NOT DISTINCT FROM $2
                OR si.mainchain_pubkey IS NOT DISTINCT FROM $4
                OR si.aura_pubkey IS NOT DISTINCT FROM $5
            )
            AND NOT (
                si.spo_sk = $1
                AND si.sidechain_pubkey IS NOT DISTINCT FROM $2
                AND si.pool_id IS NOT DISTINCT FROM $3
                AND si.mainchain_pubkey IS NOT DISTINCT FROM $4
                AND si.aura_pubkey IS NOT DISTINCT FROM $5
            )
            AND NOT EXISTS (
                SELECT 1 FROM spo_history sh
                WHERE sh.spo_sk = si.spo_sk AND sh.status = 'VALID'
            )
            AND NOT EXISTS (
                SELECT 1 FROM spo_epoch_performance sep
                WHERE sep.spo_sk = si.spo_sk
            )
            ORDER BY si.spo_sk
        "};

        let removed = sqlx::query_scalar::<_, String>(query)
            .bind(&spo.spo_sk)
            .bind(&spo.sidechain_pubkey)
            .bind(&spo.pool_id)
            .bind(&spo.mainchain_pubkey)
            .bind(&spo.aura_pubkey)
            .fetch_all(&mut **tx)
            .await?;

        for spo_sk in &removed {
            // History rows reference the identity, so they go first.
            sqlx::query("DELETE FROM spo_history WHERE spo_sk = $1")
                .bind(spo_sk)
                .execute(&mut **tx)
                .await?;
            sqlx::query("DELETE FROM spo_identity WHERE spo_sk = $1")
                .bind(spo_sk)
                .execute(&mut **tx)
                .await?;
        }

        Ok(removed)
    }

    #[trace]
    async fn save_membership(
        &self,
        memberships: &[ValidatorMembership],
        tx: &mut SqlxTransaction,
    ) -> Result<(), sqlx::Error> {
        for member in memberships.iter() {
            sqlx::query(indoc! {
                "INSERT INTO committee_membership (
                    spo_sk,
                    sidechain_pubkey,
                    epoch_no,

                    position,
                    expected_slots
                )
                VALUES ($1, $2, $3, $4, $5)
                ON CONFLICT (epoch_no, position) DO NOTHING" // Prevents re-insertion errors.
            })
            .bind(&member.spo_sk)
            .bind(&member.sidechain_pubkey)
            .bind(member.epoch_no as i64)
            .bind(member.position as i32)
            .bind(member.expected_slots as i32)
            .execute(&mut **tx)
            .await?;
        }

        Ok(())
    }

    async fn save_spo_performance(
        &self,
        metadata: &SPOEpochPerformance,
        tx: &mut SqlxTransaction,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(indoc! {
            "INSERT INTO spo_epoch_performance (
                spo_sk,
                identity_label,
                epoch_no,
                expected_blocks,
                produced_blocks
            )
            SELECT $1, $2, $3, $4, $5
            WHERE EXISTS (SELECT 1 FROM spo_identity si WHERE si.spo_sk = $1)
            ON CONFLICT (epoch_no, spo_sk) DO NOTHING"
        })
        .bind(&metadata.spo_sk)
        .bind(&metadata.identity_label)
        .bind(metadata.epoch_no as i64)
        .bind(metadata.expected_blocks as i32)
        .bind(metadata.produced_blocks as i32)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    async fn save_pool_meta(
        &self,
        metadata: &PoolMetadata,
        tx: &mut SqlxTransaction,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(indoc! {
            "INSERT INTO pool_metadata_cache (
                pool_id, hex_id, name, ticker, homepage_url, url
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (pool_id) DO UPDATE SET
                name = CASE WHEN EXCLUDED.name IS NOT NULL AND EXCLUDED.name <> '' THEN EXCLUDED.name ELSE pool_metadata_cache.name END,
                ticker = CASE WHEN EXCLUDED.ticker IS NOT NULL AND EXCLUDED.ticker <> '' THEN EXCLUDED.ticker ELSE pool_metadata_cache.ticker END,
                homepage_url = CASE WHEN EXCLUDED.homepage_url IS NOT NULL AND EXCLUDED.homepage_url <> '' THEN EXCLUDED.homepage_url ELSE pool_metadata_cache.homepage_url END,
                url = CASE WHEN EXCLUDED.url IS NOT NULL AND EXCLUDED.url <> '' THEN EXCLUDED.url ELSE pool_metadata_cache.url END,
                updated_at = CURRENT_TIMESTAMP"
        })
        .bind(&metadata.pool_id)
        .bind(&metadata.hex_id)
        .bind(&metadata.name)
        .bind(&metadata.ticker)
        .bind(&metadata.homepage_url)
        .bind(&metadata.url)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    async fn save_spo_history(
        &self,
        history: &SPOHistory,
        tx: &mut SqlxTransaction,
    ) -> Result<(), sqlx::Error> {
        let epoch = history.epoch_no as i64;

        sqlx::query(indoc! {
            "INSERT INTO spo_history (
                spo_sk,
                epoch_no,
                status,
                valid_from,
                valid_to
            )
            SELECT $1, $2, $3, $4, $5
            WHERE EXISTS (SELECT 1 FROM spo_identity si WHERE si.spo_sk = $1)
            ON CONFLICT (spo_sk, epoch_no) DO UPDATE SET
                valid_to = EXCLUDED.epoch_no,
                status = EXCLUDED.status
            "
        })
        .bind(history.spo_sk.clone())
        .bind(epoch)
        .bind(history.status.to_string())
        .bind(epoch)
        .bind(epoch)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    async fn get_pool_ids(&self, limit: i64, offset: i64) -> Result<Vec<String>, sqlx::Error> {
        let query = indoc! {"
            SELECT pool_id
            FROM pool_metadata_cache
            ORDER BY updated_at DESC, pool_id ASC
            LIMIT $1 OFFSET $2
        "};

        let rows = sqlx::query_as::<_, (String,)>(query)
            .bind(limit)
            .bind(offset)
            .fetch_all(&*self.pool)
            .await?;

        Ok(rows.into_iter().map(|(pid,)| pid).collect())
    }

    async fn get_pool_ids_after(
        &self,
        after: &str,
        limit: i64,
    ) -> Result<Vec<String>, sqlx::Error> {
        let query = indoc! {"
            SELECT pool_id
            FROM pool_metadata_cache
            WHERE pool_id > $1
            ORDER BY pool_id ASC
            LIMIT $2
        "};

        let rows = sqlx::query_as::<_, (String,)>(query)
            .bind(after)
            .bind(limit)
            .fetch_all(&*self.pool)
            .await?;
        Ok(rows.into_iter().map(|(pid,)| pid).collect())
    }

    async fn save_stake_snapshot(
        &self,
        pool_id: &str,
        live_stake: Option<i64>,
        active_stake: Option<i64>,
        live_delegators: Option<i64>,
        live_saturation: Option<f64>,
        declared_pledge: Option<i64>,
        live_pledge: Option<i64>,
        tx: &mut SqlxTransaction,
    ) -> Result<(), sqlx::Error> {
        // Call the inherent implementation to avoid recursive call to the trait method.
        Storage::save_stake_snapshot(
            self,
            pool_id,
            live_stake,
            active_stake,
            live_delegators,
            live_saturation,
            declared_pledge,
            live_pledge,
            tx,
        )
        .await
    }

    async fn insert_stake_history(
        &self,
        pool_id: &str,
        mainchain_epoch: Option<i64>,
        live_stake: Option<i64>,
        active_stake: Option<i64>,
        live_delegators: Option<i64>,
        live_saturation: Option<f64>,
        declared_pledge: Option<i64>,
        live_pledge: Option<i64>,
        tx: &mut SqlxTransaction,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(indoc! {
            "INSERT INTO spo_stake_history (
                pool_id, recorded_at, mainchain_epoch,
                live_stake, active_stake, live_delegators, live_saturation, declared_pledge, live_pledge
            ) VALUES ($1, CURRENT_TIMESTAMP, $2, $3, $4, $5, $6, $7, $8)"
        })
        .bind(pool_id)
        .bind(mainchain_epoch)
        .bind(live_stake)
        .bind(active_stake)
        .bind(live_delegators)
        .bind(live_saturation)
        .bind(declared_pledge)
        .bind(live_pledge)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    async fn get_block_timestamp(&self, height: i64) -> Result<Option<i64>, sqlx::Error> {
        let query = indoc! {"
            SELECT timestamp
            FROM blocks
            WHERE height = $1
        "};

        sqlx::query_as::<_, (i64,)>(query)
            .bind(height)
            .fetch_optional(&*self.pool)
            .await
            .map(|row| row.map(|(t,)| t))
    }

    async fn get_stake_refresh_cursor(&self) -> Result<Option<String>, sqlx::Error> {
        let row = sqlx::query_as::<_, (Option<String>,)>(
            "SELECT last_pool_id FROM spo_stake_refresh_state WHERE id = TRUE",
        )
        .fetch_optional(&*self.pool)
        .await?;
        Ok(row.and_then(|(p,)| p))
    }

    async fn set_stake_refresh_cursor(&self, pool_id: Option<&str>) -> Result<(), sqlx::Error> {
        sqlx::query(indoc! {
            "INSERT INTO spo_stake_refresh_state (id, last_pool_id, updated_at)
             VALUES (TRUE, $1, CURRENT_TIMESTAMP)
             ON CONFLICT (id) DO UPDATE SET last_pool_id = EXCLUDED.last_pool_id, updated_at = CURRENT_TIMESTAMP"
        })
        .bind(pool_id)
        .execute(&*self.pool)
        .await?;
        Ok(())
    }
}

impl Storage {
    /// Optional upsert for stake snapshot (DB-first; can be wired to external data later).
    #[allow(clippy::too_many_arguments)]
    pub async fn save_stake_snapshot(
        &self,
        pool_id: &str,
        live_stake: Option<i64>,
        active_stake: Option<i64>,
        live_delegators: Option<i64>,
        live_saturation: Option<f64>,
        declared_pledge: Option<i64>,
        live_pledge: Option<i64>,
        tx: &mut SqlxTransaction,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(indoc! {
            "INSERT INTO spo_stake_snapshot (
                pool_id, live_stake, active_stake, live_delegators, live_saturation, declared_pledge, live_pledge
            ) VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (pool_id) DO UPDATE SET
                live_stake      = EXCLUDED.live_stake,
                active_stake    = EXCLUDED.active_stake,
                live_delegators = EXCLUDED.live_delegators,
                live_saturation = EXCLUDED.live_saturation,
                declared_pledge = EXCLUDED.declared_pledge,
                live_pledge     = EXCLUDED.live_pledge,
                updated_at      = CURRENT_TIMESTAMP"
        })
        .bind(pool_id)
        .bind(live_stake)
        .bind(active_stake)
        .bind(live_delegators)
        .bind(live_saturation)
        .bind(declared_pledge)
        .bind(live_pledge)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        domain::{PoolMetadata, SPO, SPOEpochPerformance, SPOHistory, SPOStatus, storage::Storage},
        infra::storage::{SqlxTransaction, Storage as StorageImpl},
    };
    use anyhow::Context;

    const POOL_ID: &str = "pool";
    const MAINCHAIN_PUBKEY: &str = "mainchain";

    #[tokio::test]
    async fn test_valid_registration_replaces_squatted_identity() -> anyhow::Result<()> {
        let (storage, _guard) = storage().await?;
        let mut tx = storage.create_tx().await?;

        // An unsigned registration claims the pool's mainchain key with the attacker's keys.
        let squatter = spo("squatter");
        storage.save_spo(&squatter, &mut tx).await?;
        save_history(&storage, &squatter, 1, SPOStatus::Invalid, &mut tx).await?;

        let genuine = spo("genuine");
        storage.remove_unverified_spos(&genuine, &mut tx).await?;
        storage.save_spo(&genuine, &mut tx).await?;
        save_history(&storage, &genuine, 2, SPOStatus::Valid, &mut tx).await?;

        assert_eq!(identities(&mut tx).await?, ["genuine"]);
        assert_eq!(
            history(&mut tx).await?,
            [("genuine".to_owned(), "VALID".to_owned())]
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_valid_registration_replaces_squat_on_any_key() -> anyhow::Result<()> {
        let genuine = spo("genuine");
        let attacker = SPO {
            mainchain_pubkey: "attacker".to_owned(),
            ..spo("attacker")
        };
        let squats = [
            ("mainchain key", spo("attacker")),
            (
                "sidechain key",
                SPO {
                    spo_sk: genuine.spo_sk.clone(),
                    sidechain_pubkey: genuine.sidechain_pubkey.clone(),
                    ..attacker.clone()
                },
            ),
            (
                "aura key",
                SPO {
                    aura_pubkey: genuine.aura_pubkey.clone(),
                    ..attacker.clone()
                },
            ),
        ];

        for (shared, squatter) in squats {
            let (storage, _guard) = storage().await?;
            let mut tx = storage.create_tx().await?;
            storage.save_spo(&squatter, &mut tx).await?;
            save_history(&storage, &squatter, 1, SPOStatus::Invalid, &mut tx).await?;

            let removed = storage.remove_unverified_spos(&genuine, &mut tx).await?;
            assert_eq!(removed, [squatter.spo_sk.as_str()], "{shared}");

            storage.save_spo(&genuine, &mut tx).await?;
            let identity = sqlx::query_as::<_, (String, String, String)>(
                "SELECT spo_sk, mainchain_pubkey, aura_pubkey FROM spo_identity",
            )
            .fetch_all(&mut *tx)
            .await?;
            assert_eq!(
                identity,
                [(
                    genuine.spo_sk.clone(),
                    genuine.mainchain_pubkey.clone(),
                    genuine.aura_pubkey.clone()
                )],
                "{shared}"
            );
            assert!(history(&mut tx).await?.is_empty(), "{shared}");
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_unrelated_identity_is_kept() -> anyhow::Result<()> {
        let (storage, _guard) = storage().await?;
        let mut tx = storage.create_tx().await?;

        let other = SPO {
            mainchain_pubkey: "other".to_owned(),
            ..spo("other")
        };
        storage.save_spo(&other, &mut tx).await?;

        let removed = storage.remove_unverified_spos(&spo("new"), &mut tx).await?;
        assert!(removed.is_empty());
        assert_eq!(identities(&mut tx).await?, ["other"]);

        Ok(())
    }

    #[tokio::test]
    async fn test_once_valid_identity_is_kept() -> anyhow::Result<()> {
        let (storage, _guard) = storage().await?;
        let mut tx = storage.create_tx().await?;

        // An identity that was valid once was registered by the pool itself.
        let valid = spo("valid");
        storage.save_spo(&valid, &mut tx).await?;
        save_history(&storage, &valid, 1, SPOStatus::Valid, &mut tx).await?;

        storage.remove_unverified_spos(&spo("new"), &mut tx).await?;
        assert_eq!(identities(&mut tx).await?, ["valid"]);

        Ok(())
    }

    #[tokio::test]
    async fn test_block_producing_identity_is_kept() -> anyhow::Result<()> {
        let (storage, _guard) = storage().await?;
        let mut tx = storage.create_tx().await?;

        let producer = spo("producer");
        storage.save_spo(&producer, &mut tx).await?;
        let performance = SPOEpochPerformance {
            spo_sk: producer.spo_sk.clone(),
            identity_label: POOL_ID.to_owned(),
            epoch_no: 1,
            expected_blocks: 1,
            produced_blocks: 1,
        };
        storage.save_spo_performance(&performance, &mut tx).await?;

        storage.remove_unverified_spos(&spo("new"), &mut tx).await?;
        assert_eq!(identities(&mut tx).await?, ["producer"]);

        Ok(())
    }

    #[tokio::test]
    async fn test_identity_being_saved_is_kept() -> anyhow::Result<()> {
        let (storage, _guard) = storage().await?;
        let mut tx = storage.create_tx().await?;

        let same = spo("same");
        storage.save_spo(&same, &mut tx).await?;
        storage.remove_unverified_spos(&same, &mut tx).await?;
        assert_eq!(identities(&mut tx).await?, ["same"]);

        Ok(())
    }

    /// An identity for the test pool whose sidechain and aura keys derive from `key`.
    fn spo(key: &str) -> SPO {
        SPO {
            spo_sk: key.to_owned(),
            pool_id: POOL_ID.to_owned(),
            mainchain_pubkey: MAINCHAIN_PUBKEY.to_owned(),
            sidechain_pubkey: key.to_owned(),
            aura_pubkey: format!("{key}-aura"),
        }
    }

    async fn save_history(
        storage: &StorageImpl,
        spo: &SPO,
        epoch_no: u64,
        status: SPOStatus,
        tx: &mut SqlxTransaction,
    ) -> anyhow::Result<()> {
        let history = SPOHistory {
            spo_sk: spo.spo_sk.clone(),
            epoch_no,
            status,
        };
        storage.save_spo_history(&history, tx).await?;
        Ok(())
    }

    async fn identities(tx: &mut SqlxTransaction) -> anyhow::Result<Vec<String>> {
        let identities = sqlx::query_scalar("SELECT spo_sk FROM spo_identity ORDER BY spo_sk")
            .fetch_all(&mut **tx)
            .await?;
        Ok(identities)
    }

    async fn history(tx: &mut SqlxTransaction) -> anyhow::Result<Vec<(String, String)>> {
        let history = sqlx::query_as("SELECT spo_sk, status FROM spo_history ORDER BY spo_sk")
            .fetch_all(&mut **tx)
            .await?;
        Ok(history)
    }

    /// Storage with migrations applied and the test pool's metadata saved. The guard keeps the
    /// database alive.
    #[cfg(feature = "cloud")]
    async fn storage() -> anyhow::Result<(
        StorageImpl,
        testcontainers::ContainerAsync<testcontainers_modules::postgres::Postgres>,
    )> {
        use indexer_common::infra::{
            migrations,
            pool::{self, postgres::PostgresPool},
        };
        use sqlx::postgres::PgSslMode;
        use std::time::Duration;
        use testcontainers::{ImageExt, runners::AsyncRunner};
        use testcontainers_modules::postgres::Postgres;

        let container = Postgres::default()
            .with_db_name("indexer")
            .with_user("indexer")
            .with_password(env!("APP__INFRA__STORAGE__PASSWORD"))
            .with_tag("17.1-alpine")
            .start()
            .await
            .context("start Postgres container")?;
        let port = container
            .get_host_port_ipv4(5432)
            .await
            .context("get Postgres port")?;

        let config = pool::postgres::Config {
            host: "localhost".to_string(),
            port,
            dbname: "indexer".to_string(),
            user: "indexer".to_string(),
            password: env!("APP__INFRA__STORAGE__PASSWORD").into(),
            sslmode: PgSslMode::Prefer,
            max_connections: 10,
            idle_timeout: Duration::from_secs(60),
            max_lifetime: Duration::from_secs(5 * 60),
        };
        let pool = PostgresPool::new(config).await.context("create pool")?;
        migrations::postgres::run(&pool)
            .await
            .context("run migrations")?;

        let storage = StorageImpl::new(pool);
        save_pool(&storage).await?;

        Ok((storage, container))
    }

    /// Storage with migrations applied and the test pool's metadata saved. The guard keeps the
    /// database alive.
    #[cfg(feature = "standalone")]
    async fn storage() -> anyhow::Result<(StorageImpl, tempfile::TempDir)> {
        use indexer_common::infra::{
            migrations,
            pool::{self, sqlite::SqlitePool},
        };

        let temp_dir = tempfile::tempdir().context("create temp dir")?;
        let cnn_url = temp_dir.path().join("indexer.sqlite").display().to_string();
        let pool = SqlitePool::new(pool::sqlite::Config::with_url(cnn_url))
            .await
            .context("create pool")?;
        migrations::sqlite::run(&pool)
            .await
            .context("run migrations")?;

        let storage = StorageImpl::new(pool);
        save_pool(&storage).await?;

        Ok((storage, temp_dir))
    }

    async fn save_pool(storage: &StorageImpl) -> anyhow::Result<()> {
        let mut tx = storage.create_tx().await?;
        storage
            .save_pool_meta(&PoolMetadata::placeholder(POOL_ID.to_owned()), &mut tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}
