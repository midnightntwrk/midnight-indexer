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

use crate::domain::{self, Block, BlockInfo, BlockTransactions, ContractAction, Transaction};
use async_stream::try_stream;
use fastrace::trace;
use futures::{Stream, TryFutureExt, TryStreamExt};
use indexer_common::{
    domain::{
        BlockHash, ByteArray, ByteVec, ContractActionVariant, ContractBalance, DustCommitment,
        DustNonce, DustNullifier, DustOwner, RawTransaction, UnshieldedUtxo,
        dust::{
            DustEvent, DustEventDetails, DustEventType, DustGenerationInfo, DustRegistration,
            DustUtxo, QualifiedDustOutput,
        },
    },
    infra::sqlx::U128BeBytes,
};
use indoc::indoc;
use sqlx::{QueryBuilder, types::Json};
use std::num::NonZeroU32;

#[cfg(feature = "cloud")]
type Tx = sqlx::Transaction<'static, sqlx::Postgres>;

#[cfg(feature = "standalone")]
type Tx = sqlx::Transaction<'static, sqlx::Sqlite>;

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
    #[cfg_attr(feature = "cloud", trace)]
    async fn get_highest_block_info(&self) -> Result<Option<BlockInfo>, sqlx::Error> {
        let query = indoc! {"
            SELECT hash, height
            FROM blocks
            ORDER BY height DESC
            LIMIT 1
        "};

        sqlx::query_as::<_, (ByteVec, i64)>(query)
            .fetch_optional(&*self.pool)
            .await?
            .map(|(hash, height)| {
                let hash = BlockHash::try_from(hash.as_ref())
                    .map_err(|error| sqlx::Error::Decode(error.into()))?;

                Ok(BlockInfo {
                    hash,
                    height: height as u32,
                })
            })
            .transpose()
    }

    #[cfg_attr(feature = "cloud", trace)]
    async fn get_transaction_count(&self) -> Result<u64, sqlx::Error> {
        let query = indoc! {"
            SELECT count(*) 
            FROM transactions
        "};

        let (count,) = sqlx::query_as::<_, (i64,)>(query)
            .fetch_one(&*self.pool)
            .await?;

        Ok(count as u64)
    }

    #[cfg_attr(feature = "cloud", trace)]
    async fn get_contract_action_count(&self) -> Result<(u64, u64, u64), sqlx::Error> {
        let query = indoc! {"
            SELECT count(*) 
            FROM contract_actions
            WHERE variant = $1
        "};

        let (deploy_count,) = sqlx::query_as::<_, (i64,)>(query)
            .bind(ContractActionVariant::Deploy)
            .fetch_one(&*self.pool)
            .await?;
        let (call_count,) = sqlx::query_as::<_, (i64,)>(query)
            .bind(ContractActionVariant::Call)
            .fetch_one(&*self.pool)
            .await?;
        let (update_count,) = sqlx::query_as::<_, (i64,)>(query)
            .bind(ContractActionVariant::Update)
            .fetch_one(&*self.pool)
            .await?;

        Ok((deploy_count as u64, call_count as u64, update_count as u64))
    }

    #[cfg_attr(feature = "cloud", trace(properties = { "block_height": "{block_height}" }))]
    async fn get_block_transactions(
        &self,
        block_height: u32,
    ) -> Result<BlockTransactions, sqlx::Error> {
        let query = indoc! {"
            SELECT
                id,
                protocol_version,
                parent_hash,
                timestamp
            FROM blocks
            WHERE height = $1
        "};

        let (block_id, protocol_version, block_parent_hash, block_timestamp) =
            sqlx::query_as::<_, (i64, i64, ByteVec, i64)>(query)
                .bind(block_height as i64)
                .fetch_one(&*self.pool)
                .await?;
        let block_parent_hash = ByteArray::<32>::try_from(block_parent_hash.as_ref())
            .map_err(|error| sqlx::Error::Decode(error.into()))?;

        let query = indoc! {"
            SELECT raw
            FROM transactions
            WHERE block_id = $1
        "};

        let transactions = sqlx::query_as::<_, (ByteVec,)>(query)
            .bind(block_id)
            .fetch(&*self.pool)
            .map_ok(|(t,)| t)
            .try_collect::<Vec<_>>()
            .await?;

        Ok(BlockTransactions {
            transactions,
            protocol_version: (protocol_version as u32).into(),
            block_parent_hash,
            block_timestamp: block_timestamp as u64,
        })
    }

    #[cfg_attr(feature = "cloud", trace)]
    async fn save_block(&self, block: &mut Block) -> Result<Option<u64>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let (max_transaction_id, transaction_ids) = save_block(block, &mut tx).await?;
        tx.commit().await?;

        // Update the block's transactions with their database IDs.
        for (transaction, id) in block.transactions.iter_mut().zip(transaction_ids.iter()) {
            transaction.id = *id as u64;
        }

        Ok(max_transaction_id)
    }

    // DUST-specific storage methods.
    #[trace(properties = { "transaction_id": "{transaction_id}" })]
    async fn save_dust_events(
        &self,
        events: impl AsRef<[DustEvent]> + Send,
        transaction_id: u64,
    ) -> Result<(), sqlx::Error> {
        let events = events.as_ref();
        if events.is_empty() {
            return Ok(());
        }

        let query = indoc! {"
            INSERT INTO dust_events (
                transaction_id,
                transaction_hash,
                logical_segment,
                physical_segment,
                event_type,
                event_data
            )
            VALUES ($1, $2, $3, $4, $5, $6)
        "};

        for event in events {
            let event_type = DustEventType::from(&event.event_details);

            #[cfg(feature = "cloud")]
            {
                sqlx::query(query)
                    .bind(transaction_id as i64)
                    .bind(event.transaction_hash)
                    .bind(event.logical_segment as i32)
                    .bind(event.physical_segment as i32)
                    .bind(event_type)
                    .bind(Json(&event.event_details))
                    .execute(&*self.pool)
                    .await?;
            }

            #[cfg(feature = "standalone")]
            {
                // SQLite doesn't support custom enum types like PostgreSQL does.
                // While PostgreSQL can use the DustEventType enum directly (via sqlx::Type),
                // SQLite requires us to manually convert the enum to a string representation.
                let event_type = match event_type {
                    DustEventType::DustInitialUtxo => "DustInitialUtxo",
                    DustEventType::DustGenerationDtimeUpdate => "DustGenerationDtimeUpdate",
                    DustEventType::DustSpendProcessed => "DustSpendProcessed",
                };

                sqlx::query(query)
                    .bind(transaction_id as i64)
                    .bind(event.transaction_hash.as_ref())
                    .bind(event.logical_segment as i32)
                    .bind(event.physical_segment as i32)
                    .bind(event_type)
                    .bind(Json(&event.event_details))
                    .execute(&*self.pool)
                    .await?;
            }
        }

        Ok(())
    }

    #[trace]
    async fn save_dust_utxos(
        &self,
        utxos: impl AsRef<[DustUtxo]> + Send,
    ) -> Result<(), sqlx::Error> {
        let utxos = utxos.as_ref();
        if utxos.is_empty() {
            return Ok(());
        }

        let query = indoc! {"
            INSERT INTO dust_utxos (
                commitment,
                nullifier,
                initial_value,
                owner,
                nonce,
                seq,
                ctime,
                generation_info_id,
                spent_at_transaction_id
            )
        "};

        #[cfg(feature = "cloud")]
        {
            QueryBuilder::new(query)
                .push_values(utxos.iter(), |mut q, utxo| {
                    q.push_bind(utxo.commitment)
                        .push_bind(utxo.nullifier)
                        .push_bind(U128BeBytes::from(utxo.initial_value))
                        .push_bind(utxo.owner)
                        .push_bind(utxo.nonce)
                        .push_bind(utxo.seq as i32)
                        .push_bind(utxo.ctime as i64)
                        .push_bind(utxo.generation_info_id.map(|id| id as i64))
                        .push_bind(utxo.spent_at_transaction_id.map(|id| id as i64));
                })
                .build()
                .execute(&*self.pool)
                .await?;
        }

        #[cfg(feature = "standalone")]
        {
            QueryBuilder::new(query)
                .push_values(utxos.iter(), |mut q, utxo| {
                    q.push_bind(utxo.commitment.as_ref())
                        .push_bind(utxo.nullifier.as_ref().map(|n| n.as_ref()))
                        .push_bind(U128BeBytes::from(utxo.initial_value))
                        .push_bind(utxo.owner.as_ref())
                        .push_bind(utxo.nonce.as_ref())
                        .push_bind(utxo.seq as i32)
                        .push_bind(utxo.ctime as i64)
                        .push_bind(utxo.generation_info_id.map(|id| id as i64))
                        .push_bind(utxo.spent_at_transaction_id.map(|id| id as i64));
                })
                .build()
                .execute(&*self.pool)
                .await?;
        }

        Ok(())
    }

    #[trace]
    async fn save_dust_generation_info(
        &self,
        generation_info: impl AsRef<[DustGenerationInfo]> + Send,
    ) -> Result<(), sqlx::Error> {
        let generation_info = generation_info.as_ref();
        if generation_info.is_empty() {
            return Ok(());
        }

        let query = indoc! {"
            INSERT INTO dust_generation_info (
                value,
                owner,
                nonce,
                merkle_index,
                ctime,
                dtime
            )
        "};

        #[cfg(feature = "cloud")]
        {
            QueryBuilder::new(query)
                .push_values(generation_info.iter(), |mut q, info| {
                    q.push_bind(U128BeBytes::from(info.value))
                        .push_bind(info.owner)
                        .push_bind(info.nonce)
                        .push_bind(0i64) // TODO: merkle_index should come from somewhere.
                        .push_bind(info.ctime as i64)
                        .push_bind(if info.dtime == 0 {
                            None
                        } else {
                            Some(info.dtime as i64)
                        });
                })
                .build()
                .execute(&*self.pool)
                .await?;
        }

        #[cfg(feature = "standalone")]
        {
            QueryBuilder::new(query)
                .push_values(generation_info.iter(), |mut q, info| {
                    q.push_bind(U128BeBytes::from(info.value))
                        .push_bind(info.owner.as_ref())
                        .push_bind(info.nonce.as_ref())
                        .push_bind(0i64) // TODO: merkle_index should come from somewhere.
                        .push_bind(info.ctime as i64)
                        .push_bind(if info.dtime == 0 {
                            None
                        } else {
                            Some(info.dtime as i64)
                        });
                })
                .build()
                .execute(&*self.pool)
                .await?;
        }

        Ok(())
    }

    #[trace]
    async fn save_cnight_registrations(
        &self,
        registrations: impl AsRef<[DustRegistration]> + Send,
    ) -> Result<(), sqlx::Error> {
        let registrations = registrations.as_ref();
        if registrations.is_empty() {
            return Ok(());
        }

        #[cfg(feature = "cloud")]
        {
            let query = indoc! {"
                INSERT INTO cnight_registrations (
                    cardano_address,
                    dust_address,
                    is_valid,
                    registered_at,
                    removed_at
                )
            "};

            QueryBuilder::new(query)
                .push_values(registrations.iter(), |mut q, reg| {
                    q.push_bind(reg.cardano_address.as_ref())
                        .push_bind(reg.dust_address.as_ref())
                        .push_bind(reg.is_valid)
                        .push_bind(reg.registered_at as i64)
                        .push_bind(reg.removed_at.map(|t| t as i64));
                })
                .push(" ON CONFLICT (cardano_address, dust_address) DO UPDATE SET ")
                .push("is_valid = EXCLUDED.is_valid, ")
                .push("removed_at = EXCLUDED.removed_at")
                .build()
                .execute(&*self.pool)
                .await?;
        }

        #[cfg(feature = "standalone")]
        {
            for reg in registrations {
                let query = indoc! {"
                    INSERT INTO cnight_registrations (
                        cardano_address,
                        dust_address,
                        is_valid,
                        registered_at,
                        removed_at
                    )
                    VALUES ($1, $2, $3, $4, $5)
                    ON CONFLICT (cardano_address, dust_address) DO UPDATE SET
                        is_valid = excluded.is_valid,
                        removed_at = excluded.removed_at
                "};

                sqlx::query(query)
                    .bind(reg.cardano_address.as_ref())
                    .bind(reg.dust_address.as_ref())
                    .bind(if reg.is_valid { 1i32 } else { 0i32 }) // SQLite boolean
                    .bind(reg.registered_at as i64)
                    .bind(reg.removed_at.map(|t| t as i64))
                    .execute(&*self.pool)
                    .await?;
            }
        }

        Ok(())
    }

    fn get_dust_generation_info_by_owner(
        &self,
        owner: DustOwner,
        mut generation_info_id: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<DustGenerationInfo, sqlx::Error>> + Send {
        try_stream! {
            loop {
                let query = indoc! {"
                    SELECT
                        id,
                        value,
                        owner,
                        nonce,
                        ctime,
                        dtime
                    FROM dust_generation_info
                    WHERE owner = $1
                    AND id >= $2
                    ORDER BY id
                    LIMIT $3
                "};

                #[cfg(feature = "cloud")]
                let rows = sqlx::query_as::<_, (i64, U128BeBytes, DustOwner, DustNonce, i64, Option<i64>)>(query)
                    .bind(owner.as_ref())
                    .bind(generation_info_id as i64)
                    .bind(batch_size.get() as i64)
                    .fetch_all(&*self.pool)
                    .await?;

                #[cfg(feature = "standalone")]
                let rows = sqlx::query_as::<_, (i64, U128BeBytes, ByteVec, ByteVec, i64, Option<i64>)>(query)
                    .bind(owner.as_ref())
                    .bind(generation_info_id as i64)
                    .bind(batch_size.get() as i64)
                    .fetch_all(&*self.pool)
                    .await?;

                if rows.is_empty() {
                    break;
                }

                #[cfg(feature = "cloud")]
                let items = rows
                    .iter()
                    .map(|(_, value, owner, nonce, ctime, dtime)| DustGenerationInfo {
                        value: (*value).into(),
                        owner: *owner,
                        nonce: *nonce,
                        ctime: *ctime as u64,
                        dtime: dtime.map(|dt| dt as u64).unwrap_or(0),
                    })
                    .collect::<Vec<DustGenerationInfo>>();

                #[cfg(feature = "standalone")]
                let items = rows
                    .iter()
                    .map(|(_, value, owner, nonce, ctime, dtime)| -> Result<DustGenerationInfo, sqlx::Error> {
                        Ok(DustGenerationInfo {
                            value: (*value).into(),
                            owner: DustOwner::try_from(owner.as_ref())
                                .map_err(|e| sqlx::Error::Decode(e.into()))?,
                            nonce: DustNonce::try_from(nonce.as_ref())
                                .map_err(|e| sqlx::Error::Decode(e.into()))?,
                            ctime: *ctime as u64,
                            dtime: dtime.map(|dt| dt as u64).unwrap_or(0),
                        })
                    })
                    .collect::<Result<Vec<DustGenerationInfo>, _>>()?;

                match rows.last() {
                    Some(row) => generation_info_id = row.0 as u64 + 1,
                    None => break,
                }

                for item in items {
                    yield item;
                }
            }
        }
    }

    fn get_dust_utxos_by_owner(
        &self,
        owner: DustOwner,
        mut utxo_id: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<DustUtxo, sqlx::Error>> + Send {
        try_stream! {
            loop {
                let query = indoc! {"
                    SELECT 
                        id,
                        commitment,
                        nullifier,
                        initial_value,
                        owner,
                        nonce,
                        seq,
                        ctime,
                        generation_info_id,
                        spent_at_transaction_id
                    FROM dust_utxos
                    WHERE owner = $1
                    AND id >= $2
                    ORDER BY id
                    LIMIT $3
                "};

                #[cfg(feature = "cloud")]
                let rows = sqlx::query_as::<_, (i64, DustCommitment, Option<DustNullifier>, U128BeBytes, DustOwner, DustNonce, i32, i64, Option<i64>, Option<i64>)>(query)
                    .bind(owner.as_ref())
                    .bind(utxo_id as i64)
                    .bind(batch_size.get() as i64)
                    .fetch_all(&*self.pool)
                    .await?;

                #[cfg(feature = "standalone")]
                let rows = sqlx::query_as::<_, (i64, ByteVec, Option<ByteVec>, U128BeBytes, ByteVec, ByteVec, i32, i64, Option<i64>, Option<i64>)>(query)
                    .bind(owner.as_ref())
                    .bind(utxo_id as i64)
                    .bind(batch_size.get() as i64)
                    .fetch_all(&*self.pool)
                    .await?;

                if rows.is_empty() {
                    break;
                }

                #[cfg(feature = "cloud")]
                let items = rows
                    .iter()
                    .map(|(_, commitment, nullifier, initial_value, owner, nonce, seq, ctime, generation_info_id, spent_at_transaction_id)| DustUtxo {
                        commitment: *commitment,
                        nullifier: *nullifier,
                        initial_value: (*initial_value).into(),
                        owner: *owner,
                        nonce: *nonce,
                        seq: *seq as u32,
                        ctime: *ctime as u64,
                        generation_info_id: generation_info_id.map(|id| id as u64),
                        spent_at_transaction_id: spent_at_transaction_id.map(|id| id as u64),
                    })
                    .collect::<Vec<DustUtxo>>();

                #[cfg(feature = "standalone")]
                let items = rows
                    .iter()
                    .map(|(_, commitment, nullifier, initial_value, owner, nonce, seq, ctime, generation_info_id, spent_at_transaction_id)| -> Result<DustUtxo, sqlx::Error> {
                        Ok(DustUtxo {
                            commitment: DustCommitment::try_from(commitment.as_ref())
                                .map_err(|e| sqlx::Error::Decode(e.into()))?,
                            nullifier: nullifier
                                .as_ref()
                                .map(|n| DustNullifier::try_from(n.as_ref())
                                    .map_err(|e| sqlx::Error::Decode(e.into())))
                                .transpose()?,
                            initial_value: (*initial_value).into(),
                            owner: DustOwner::try_from(owner.as_ref())
                                .map_err(|e| sqlx::Error::Decode(e.into()))?,
                            nonce: DustNonce::try_from(nonce.as_ref())
                                .map_err(|e| sqlx::Error::Decode(e.into()))?,
                            seq: *seq as u32,
                            ctime: *ctime as u64,
                            generation_info_id: generation_info_id.map(|id| id as u64),
                            spent_at_transaction_id: spent_at_transaction_id.map(|id| id as u64),
                        })
                    })
                    .collect::<Result<Vec<DustUtxo>, _>>()?;

                match rows.last() {
                    Some(row) => utxo_id = row.0 as u64 + 1,
                    None => break,
                }

                for item in items {
                    yield item;
                }
            }
        }
    }

    fn search_transactions_by_nullifier_prefix(
        &self,
        prefix: &str,
        after_block: Option<u32>,
        mut transaction_id: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<(u64, RawTransaction), sqlx::Error>> + Send {
        let nullifier_prefix = prefix.to_string();
        let prefix_len = nullifier_prefix.len();

        try_stream! {
            loop {
                #[cfg(feature = "cloud")]
                let query = if let Some(_block_height) = after_block {
                    indoc! {"
                        SELECT DISTINCT dust_utxos.spent_at_transaction_id, transactions.raw
                        FROM dust_utxos
                        JOIN transactions ON dust_utxos.spent_at_transaction_id = transactions.id
                        JOIN blocks ON transactions.block_id = blocks.id
                        WHERE substring(dust_utxos.nullifier::text, 1, $1) = $2
                        AND dust_utxos.nullifier IS NOT NULL
                        AND dust_utxos.spent_at_transaction_id IS NOT NULL
                        AND dust_utxos.spent_at_transaction_id >= $3
                        AND blocks.height > $4
                        ORDER BY dust_utxos.spent_at_transaction_id
                        LIMIT $5
                    "}
                } else {
                    indoc! {"
                        SELECT DISTINCT dust_utxos.spent_at_transaction_id, transactions.raw
                        FROM dust_utxos
                        JOIN transactions ON dust_utxos.spent_at_transaction_id = transactions.id
                        WHERE substring(dust_utxos.nullifier::text, 1, $1) = $2
                        AND dust_utxos.nullifier IS NOT NULL
                        AND dust_utxos.spent_at_transaction_id IS NOT NULL
                        AND dust_utxos.spent_at_transaction_id >= $3
                        ORDER BY dust_utxos.spent_at_transaction_id
                        LIMIT $4
                    "}
                };

                #[cfg(feature = "standalone")]
                let query = if let Some(_block_height) = after_block {
                    indoc! {"
                        SELECT DISTINCT dust_utxos.spent_at_transaction_id, transactions.raw
                        FROM dust_utxos
                        JOIN transactions ON dust_utxos.spent_at_transaction_id = transactions.id
                        JOIN blocks ON transactions.block_id = blocks.id
                        WHERE substr(hex(dust_utxos.nullifier), 1, $1) = $2
                        AND dust_utxos.nullifier IS NOT NULL
                        AND dust_utxos.spent_at_transaction_id IS NOT NULL
                        AND dust_utxos.spent_at_transaction_id >= $3
                        AND blocks.height > $4
                        ORDER BY dust_utxos.spent_at_transaction_id
                        LIMIT $5
                    "}
                } else {
                    indoc! {"
                        SELECT DISTINCT dust_utxos.spent_at_transaction_id, transactions.raw
                        FROM dust_utxos
                        JOIN transactions ON dust_utxos.spent_at_transaction_id = transactions.id
                        WHERE substr(hex(dust_utxos.nullifier), 1, $1) = $2
                        AND dust_utxos.nullifier IS NOT NULL
                        AND dust_utxos.spent_at_transaction_id IS NOT NULL
                        AND dust_utxos.spent_at_transaction_id >= $3
                        ORDER BY dust_utxos.spent_at_transaction_id
                        LIMIT $4
                    "}
                };

                let rows = if let Some(block_height) = after_block {
                    sqlx::query_as::<_, (i64, ByteVec)>(query)
                        .bind(prefix_len as i32)
                        .bind(&nullifier_prefix)
                        .bind(transaction_id as i64)
                        .bind(block_height as i64)
                        .bind(batch_size.get() as i64)
                        .fetch_all(&*self.pool)
                        .await?
                } else {
                    sqlx::query_as::<_, (i64, ByteVec)>(query)
                        .bind(prefix_len as i32)
                        .bind(&nullifier_prefix)
                        .bind(transaction_id as i64)
                        .bind(batch_size.get() as i64)
                        .fetch_all(&*self.pool)
                        .await?
                };

                if rows.is_empty() {
                    break;
                }

                match rows.last() {
                    Some(row) => transaction_id = row.0 as u64 + 1,
                    None => break,
                }

                for (tx_id, raw) in rows {
                    yield (tx_id as u64, raw);
                }
            }
        }
    }

    #[trace(properties = { "generation_index": "{generation_index}", "dtime": "{dtime}" })]
    async fn update_dust_generation_dtime(
        &self,
        generation_index: u64,
        dtime: u64,
    ) -> Result<(), sqlx::Error> {
        let query = indoc! {"
            UPDATE dust_generation_info
            SET dtime = $1
            WHERE merkle_index = $2
        "};

        sqlx::query(query)
            .bind(dtime as i64)
            .bind(generation_index as i64)
            .execute(&*self.pool)
            .await?;

        Ok(())
    }

    #[trace(properties = { "transaction_id": "{transaction_id}" })]
    async fn mark_dust_utxo_spent(
        &self,
        commitment: DustCommitment,
        nullifier: DustNullifier,
        transaction_id: u64,
    ) -> Result<(), sqlx::Error> {
        let query = indoc! {"
            UPDATE dust_utxos
            SET
                nullifier = $1,
                spent_at_transaction_id = $2
            WHERE commitment = $3
        "};

        #[cfg(feature = "cloud")]
        sqlx::query(query)
            .bind(nullifier)
            .bind(transaction_id as i64)
            .bind(commitment)
            .execute(&*self.pool)
            .await?;

        #[cfg(feature = "standalone")]
        sqlx::query(query)
            .bind(nullifier.as_ref())
            .bind(transaction_id as i64)
            .bind(commitment.as_ref())
            .execute(&*self.pool)
            .await?;

        Ok(())
    }
}

#[cfg_attr(feature = "cloud", trace)]
async fn save_block(block: &Block, tx: &mut Tx) -> Result<(Option<u64>, Vec<i64>), sqlx::Error> {
    let query = indoc! {"
        INSERT INTO blocks (
            hash,
            height,
            protocol_version,
            parent_hash,
            author,
            timestamp
        )
    "};

    let block_id = QueryBuilder::new(query)
        .push_values([block], |mut q, block| {
            let Block {
                hash,
                height,
                protocol_version,
                parent_hash,
                author,
                timestamp,
                ..
            } = block;

            #[cfg(feature = "standalone")]
            let author = author.as_ref().map(|a| a.as_ref());

            q.push_bind(hash.as_ref())
                .push_bind(*height as i64)
                .push_bind(protocol_version.0 as i64)
                .push_bind(parent_hash.as_ref())
                .push_bind(author)
                .push_bind(*timestamp as i64);
        })
        .push(" RETURNING id")
        .build_query_as::<(i64,)>()
        .fetch_one(&mut **tx)
        .map_ok(|(id,)| id)
        .await?;

    save_transactions(&block.transactions, block_id, tx).await
}

#[cfg_attr(feature = "cloud", trace(properties = { "block_id": "{block_id}" }))]
async fn save_transactions(
    transactions: &[Transaction],
    block_id: i64,
    tx: &mut Tx,
) -> Result<(Option<u64>, Vec<i64>), sqlx::Error> {
    if transactions.is_empty() {
        return Ok((None, vec![]));
    }

    #[cfg(feature = "cloud")]
    let query = indoc! {"
        INSERT INTO transactions (
            block_id,
            hash,
            protocol_version,
            transaction_result,
            raw,
            merkle_tree_root,
            start_index,
            end_index,
            paid_fees,
            estimated_fees,
            identifiers
        )
    "};

    #[cfg(feature = "standalone")]
    let query = indoc! {"
        INSERT INTO transactions (
            block_id,
            hash,
            protocol_version,
            transaction_result,
            raw,
            merkle_tree_root,
            start_index,
            end_index,
            paid_fees,
            estimated_fees
        )
    "};

    let transaction_ids = QueryBuilder::new(query)
        .push_values(transactions.iter(), |mut q, transaction| {
            #[cfg_attr(feature = "standalone", allow(unused_variables))]
            let Transaction {
                hash,
                protocol_version,
                transaction_result,
                identifiers,
                raw,
                merkle_tree_root,
                start_index,
                end_index,
                paid_fees,
                estimated_fees,
                ..
            } = transaction;

            q.push_bind(block_id)
                .push_bind(hash.as_ref())
                .push_bind(protocol_version.0 as i64)
                .push_bind(Json(transaction_result))
                .push_bind(raw)
                .push_bind(merkle_tree_root)
                .push_bind(*start_index as i64)
                .push_bind(*end_index as i64)
                .push_bind(U128BeBytes::from(*paid_fees))
                .push_bind(U128BeBytes::from(*estimated_fees));

            #[cfg(feature = "cloud")]
            q.push_bind(identifiers);
        })
        .push(" RETURNING id")
        .build_query_as::<(i64,)>()
        .fetch(&mut **tx)
        .map_ok(|(id,)| id)
        .try_collect::<Vec<_>>()
        .await?;

    for (transaction, &transaction_id) in transactions.iter().zip(transaction_ids.iter()) {
        #[cfg(feature = "standalone")]
        save_identifiers(&transaction.identifiers, transaction_id, tx).await?;

        let contract_action_ids =
            save_contract_actions(&transaction.contract_actions, transaction_id, tx).await?;

        let contract_balances = transaction
            .contract_actions
            .iter()
            .zip(contract_action_ids.iter())
            .flat_map(|(action, &action_id)| {
                action
                    .extracted_balances
                    .iter()
                    .map(move |&balance| (action_id, balance))
            })
            .collect::<Vec<_>>();
        save_contract_balances(contract_balances, tx).await?;

        save_unshielded_utxos(
            &transaction.created_unshielded_utxos,
            transaction_id,
            false,
            tx,
        )
        .await?;

        save_unshielded_utxos(
            &transaction.spent_unshielded_utxos,
            transaction_id,
            true,
            tx,
        )
        .await?;

        // Process DUST events within the same transaction.
        if !transaction.dust_events.is_empty() {
            process_dust_events_in_transaction(&transaction.dust_events, transaction_id as u64, tx)
                .await?;
            save_dust_events_tx(&transaction.dust_events, transaction_id as u64, tx).await?;
        }
    }

    let max_id = transaction_ids.last().map(|&n| n as u64);
    Ok((max_id, transaction_ids))
}

#[cfg_attr(feature = "cloud", trace(properties = { "transaction_id": "{transaction_id}" }))]
async fn save_unshielded_utxos(
    utxos: &[UnshieldedUtxo],
    transaction_id: i64,
    spent: bool,
    tx: &mut Tx,
) -> Result<(), sqlx::Error> {
    if utxos.is_empty() {
        return Ok(());
    }

    if spent {
        for &utxo in utxos {
            let query = indoc! {"
                INSERT INTO unshielded_utxos (
                    creating_transaction_id,
                    spending_transaction_id,
                    owner,
                    token_type,
                    value,
                    output_index,
                    intent_hash
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7)
                ON CONFLICT (intent_hash, output_index)
                DO UPDATE SET spending_transaction_id = $2
                WHERE unshielded_utxos.spending_transaction_id IS NULL
            "};

            let UnshieldedUtxo {
                owner,
                token_type,
                value,
                intent_hash,
                output_index,
            } = utxo;

            #[cfg(feature = "standalone")]
            let (owner, token_type, intent_hash) =
                { (owner.as_ref(), token_type.as_ref(), intent_hash.as_ref()) };

            sqlx::query(query)
                .bind(transaction_id)
                .bind(transaction_id)
                .bind(owner)
                .bind(token_type)
                .bind(U128BeBytes::from(value))
                .bind(output_index as i32)
                .bind(intent_hash)
                .execute(&mut **tx)
                .await?;
        }
    } else {
        let query_base = indoc! {"
            INSERT INTO unshielded_utxos (
                creating_transaction_id,
                owner,
                token_type,
                value,
                output_index,
                intent_hash
            )
        "};

        QueryBuilder::new(query_base)
            .push_values(utxos.iter(), |mut q, utxo| {
                let UnshieldedUtxo {
                    owner,
                    token_type,
                    value,
                    intent_hash,
                    output_index,
                } = utxo;

                #[cfg(feature = "standalone")]
                let (owner, token_type, intent_hash) =
                    { (owner.as_ref(), token_type.as_ref(), intent_hash.as_ref()) };

                q.push_bind(transaction_id)
                    .push_bind(owner)
                    .push_bind(token_type)
                    .push_bind(U128BeBytes::from(*value))
                    .push_bind(*output_index as i32)
                    .push_bind(intent_hash);
            })
            .build()
            .execute(&mut **tx)
            .await?;
    }

    Ok(())
}

#[cfg_attr(feature = "cloud", trace(properties = { "transaction_id": "{transaction_id}" }))]
async fn save_contract_actions(
    contract_actions: &[ContractAction],
    transaction_id: i64,
    tx: &mut Tx,
) -> Result<Vec<i64>, sqlx::Error> {
    if contract_actions.is_empty() {
        return Ok(Vec::new());
    }

    let query = indoc! {"
        INSERT INTO contract_actions (
            transaction_id,
            address,
            state,
            zswap_state,
            variant,
            attributes
        )
    "};

    let contract_action_ids = QueryBuilder::new(query)
        .push_values(contract_actions.iter(), |mut q, action| {
            q.push_bind(transaction_id)
                .push_bind(&action.address)
                .push_bind(&action.state)
                .push_bind(&action.zswap_state)
                .push_bind(ContractActionVariant::from(&action.attributes))
                .push_bind(Json(&action.attributes));
        })
        .push(" RETURNING id")
        .build_query_as::<(i64,)>()
        .fetch_all(&mut **tx)
        .await?
        .into_iter()
        .map(|(id,)| id)
        .collect();

    Ok(contract_action_ids)
}

#[cfg_attr(feature = "cloud", trace)]
async fn save_contract_balances(
    balances: Vec<(i64, ContractBalance)>,
    tx: &mut Tx,
) -> Result<(), sqlx::Error> {
    if !balances.is_empty() {
        let query = indoc! {"
            INSERT INTO contract_balances (
                contract_action_id,
                token_type,
                amount
            )
        "};

        QueryBuilder::new(query)
            .push_values(balances.iter(), |mut q, (action_id, balance)| {
                let ContractBalance { token_type, amount } = balance;

                #[cfg(feature = "standalone")]
                let token_type = token_type.as_ref();

                q.push_bind(*action_id)
                    .push_bind(token_type)
                    .push_bind(U128BeBytes::from(*amount));
            })
            .build()
            .execute(&mut **tx)
            .await?;
    }

    Ok(())
}

#[cfg(feature = "standalone")]
#[trace(properties = { "transaction_id": "{transaction_id}" })]
async fn save_identifiers(
    identifiers: &[indexer_common::domain::RawTransactionIdentifier],
    transaction_id: i64,
    tx: &mut Tx,
) -> Result<(), sqlx::Error> {
    if !identifiers.is_empty() {
        let query = indoc! {"
            INSERT INTO transaction_identifiers (
                transaction_id,
                identifier
            )
        "};

        QueryBuilder::new(query)
            .push_values(identifiers.iter(), |mut q, identifier| {
                q.push_bind(transaction_id).push_bind(identifier);
            })
            .build()
            .execute(&mut **tx)
            .await?;
    }

    Ok(())
}

// Transaction-aware DUST processing functions.
#[trace]
async fn save_dust_events_tx(
    events: impl AsRef<[DustEvent]>,
    transaction_id: u64,
    tx: &mut Tx,
) -> Result<(), sqlx::Error> {
    let events = events.as_ref();
    for event in events {
        let event_type = DustEventType::from(&event.event_details);

        let query = indoc! {"
            INSERT INTO dust_events (
                transaction_id,
                transaction_hash,
                logical_segment,
                physical_segment,
                event_type,
                event_data
            )
            VALUES ($1, $2, $3, $4, $5, $6)
        "};

        #[cfg(feature = "cloud")]
        {
            sqlx::query(query)
                .bind(transaction_id as i64)
                .bind(event.transaction_hash)
                .bind(event.logical_segment as i32)
                .bind(event.physical_segment as i32)
                .bind(event_type)
                .bind(Json(&event.event_details))
                .execute(&mut **tx)
                .await?;
        }

        #[cfg(feature = "standalone")]
        {
            // SQLite doesn't support custom enum types like PostgreSQL does.
            // While PostgreSQL can use the DustEventType enum directly (via sqlx::Type),
            // SQLite requires us to manually convert the enum to a string representation.
            let event_type = match event_type {
                DustEventType::DustInitialUtxo => "DustInitialUtxo",
                DustEventType::DustGenerationDtimeUpdate => "DustGenerationDtimeUpdate",
                DustEventType::DustSpendProcessed => "DustSpendProcessed",
            };

            sqlx::query(query)
                .bind(transaction_id as i64)
                .bind(event.transaction_hash.as_ref())
                .bind(event.logical_segment as i32)
                .bind(event.physical_segment as i32)
                .bind(event_type)
                .bind(Json(&event.event_details))
                .execute(&mut **tx)
                .await?;
        }
    }

    Ok(())
}

#[trace]
async fn process_dust_events_in_transaction(
    events: impl AsRef<[DustEvent]>,
    transaction_id: u64,
    tx: &mut Tx,
) -> Result<(), sqlx::Error> {
    // Group events by type for efficient processing.
    let (initial_utxos, generation_updates, spend_events) = group_dust_events_by_type(&events);

    // Process initial DUST UTXOs.
    if !initial_utxos.is_empty() {
        process_initial_utxos_tx(initial_utxos, tx).await?;
    }

    // Process generation time updates.
    for (generation, generation_index) in generation_updates {
        update_dust_generation_dtime_tx(generation_index, generation.dtime, tx).await?;
    }

    // Process DUST spends.
    for (commitment, nullifier, _v_fee) in spend_events {
        mark_dust_utxo_spent_tx(commitment, nullifier, transaction_id, tx).await?;
    }

    Ok(())
}

type InitialUtxoEvent<'a> = (&'a QualifiedDustOutput, &'a DustGenerationInfo, u64);
type GenerationUpdateEvent<'a> = (&'a DustGenerationInfo, u64);
type SpendEvent = (ByteArray<32>, ByteArray<32>, u128);

fn group_dust_events_by_type<'a>(
    events: &'a impl AsRef<[DustEvent]>,
) -> (
    Vec<InitialUtxoEvent<'a>>,
    Vec<GenerationUpdateEvent<'a>>,
    Vec<SpendEvent>,
) {
    events.as_ref().iter().fold(
        (Vec::new(), Vec::new(), Vec::new()),
        |(mut initial, mut updates, mut spends), event| {
            match &event.event_details {
                DustEventDetails::DustInitialUtxo {
                    output,
                    generation,
                    generation_index,
                } => {
                    initial.push((output, generation, *generation_index));
                }
                DustEventDetails::DustGenerationDtimeUpdate {
                    generation,
                    generation_index,
                } => {
                    updates.push((generation, *generation_index));
                }
                DustEventDetails::DustSpendProcessed {
                    commitment,
                    commitment_index: _,
                    nullifier,
                    v_fee,
                    time: _,
                    params: _,
                } => {
                    spends.push((*commitment, *nullifier, *v_fee));
                }
            }
            (initial, updates, spends)
        },
    )
}

async fn process_initial_utxos_tx(
    initial_utxos: Vec<InitialUtxoEvent<'_>>,
    tx: &mut Tx,
) -> Result<(), sqlx::Error> {
    let (generation_infos, dust_utxos) = initial_utxos
        .into_iter()
        .map(|(output, generation, generation_index)| {
            let generation_info = *generation;
            let dust_utxo = DustUtxo {
                // TODO: Calculate proper commitment from output fields once ledger API provides it.
                // For now using owner as placeholder which is incorrect.
                commitment: ByteArray(output.owner.0),
                nullifier: None,
                initial_value: output.initial_value,
                owner: output.owner,
                nonce: output.nonce,
                seq: output.seq,
                ctime: output.ctime,
                generation_info_id: Some(generation_index),
                spent_at_transaction_id: None,
            };
            (generation_info, dust_utxo)
        })
        .unzip::<_, _, Vec<_>, Vec<_>>();

    // Save generation info first.
    if !generation_infos.is_empty() {
        save_dust_generation_info_tx(&generation_infos, tx).await?;
    }

    // Then save DUST UTXOs.
    if !dust_utxos.is_empty() {
        save_dust_utxos_tx(&dust_utxos, tx).await?;
    }

    Ok(())
}

async fn save_dust_generation_info_tx(
    generation_info: &[DustGenerationInfo],
    tx: &mut Tx,
) -> Result<(), sqlx::Error> {
    #[cfg(feature = "cloud")]
    {
        let query = indoc! {"
            INSERT INTO dust_generation_info (
                value,
                owner,
                nonce,
                merkle_index,
                ctime,
                dtime
            )
        "};

        QueryBuilder::new(query)
            .push_values(generation_info.iter(), |mut q, info| {
                q.push_bind(U128BeBytes::from(info.value))
                    .push_bind(info.owner)
                    .push_bind(info.nonce)
                    .push_bind(0i64) // TODO: merkle_index should come from somewhere.
                    .push_bind(info.ctime as i64)
                    .push_bind(if info.dtime == 0 {
                        None
                    } else {
                        Some(info.dtime as i64)
                    });
            })
            .build()
            .execute(&mut **tx)
            .await?;
    }

    #[cfg(feature = "standalone")]
    {
        // SQLite doesn't support batch inserts the same way.
        for info in generation_info {
            let query = indoc! {"
                INSERT INTO dust_generation_info (
                    value,
                    owner,
                    nonce,
                    merkle_index,
                    ctime,
                    dtime
                )
                VALUES ($1, $2, $3, $4, $5, $6)
            "};

            sqlx::query(query)
                .bind(U128BeBytes::from(info.value))
                .bind(info.owner.as_ref())
                .bind(info.nonce.as_ref())
                .bind(0i64) // TODO: merkle_index should come from somewhere.
                .bind(info.ctime as i64)
                .bind(if info.dtime == 0 {
                    None
                } else {
                    Some(info.dtime as i64)
                })
                .execute(&mut **tx)
                .await?;
        }
    }

    Ok(())
}

async fn save_dust_utxos_tx(utxos: &[DustUtxo], tx: &mut Tx) -> Result<(), sqlx::Error> {
    #[cfg(feature = "cloud")]
    {
        let query = indoc! {"
            INSERT INTO dust_utxos (
                commitment,
                nullifier,
                initial_value,
                owner,
                nonce,
                seq,
                ctime,
                generation_info_id,
                spent_at_transaction_id
            )
        "};

        QueryBuilder::new(query)
            .push_values(utxos.iter(), |mut q, utxo| {
                q.push_bind(utxo.commitment)
                    .push_bind(utxo.nullifier)
                    .push_bind(U128BeBytes::from(utxo.initial_value))
                    .push_bind(utxo.owner)
                    .push_bind(utxo.nonce)
                    .push_bind(utxo.seq as i32)
                    .push_bind(utxo.ctime as i64)
                    .push_bind(utxo.generation_info_id.map(|id| id as i64))
                    .push_bind(utxo.spent_at_transaction_id.map(|id| id as i64));
            })
            .build()
            .execute(&mut **tx)
            .await?;
    }

    #[cfg(feature = "standalone")]
    {
        // SQLite doesn't support batch inserts the same way.
        for utxo in utxos {
            let query = indoc! {"
                INSERT INTO dust_utxos (
                    commitment,
                    nullifier,
                    initial_value,
                    owner,
                    nonce,
                    seq,
                    ctime,
                    generation_info_id,
                    spent_at_transaction_id
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "};

            sqlx::query(query)
                .bind(utxo.commitment.as_ref())
                .bind(utxo.nullifier.as_ref().map(|n| n.as_ref()))
                .bind(U128BeBytes::from(utxo.initial_value))
                .bind(utxo.owner.as_ref())
                .bind(utxo.nonce.as_ref())
                .bind(utxo.seq as i32)
                .bind(utxo.ctime as i64)
                .bind(utxo.generation_info_id.map(|id| id as i64))
                .bind(utxo.spent_at_transaction_id.map(|id| id as i64))
                .execute(&mut **tx)
                .await?;
        }
    }

    Ok(())
}

async fn update_dust_generation_dtime_tx(
    generation_index: u64,
    dtime: u64,
    tx: &mut Tx,
) -> Result<(), sqlx::Error> {
    let query = indoc! {"
        UPDATE dust_generation_info
        SET dtime = $1
        WHERE merkle_index = $2
    "};

    sqlx::query(query)
        .bind(dtime as i64)
        .bind(generation_index as i64)
        .execute(&mut **tx)
        .await?;

    Ok(())
}

async fn mark_dust_utxo_spent_tx(
    commitment: DustCommitment,
    nullifier: DustNullifier,
    transaction_id: u64,
    tx: &mut Tx,
) -> Result<(), sqlx::Error> {
    let query = indoc! {"
        UPDATE dust_utxos
        SET 
            nullifier = $1,
            spent_at_transaction_id = $2
        WHERE commitment = $3
    "};

    #[cfg(feature = "cloud")]
    sqlx::query(query)
        .bind(nullifier)
        .bind(transaction_id as i64)
        .bind(commitment)
        .execute(&mut **tx)
        .await?;

    #[cfg(feature = "standalone")]
    sqlx::query(query)
        .bind(nullifier.as_ref())
        .bind(transaction_id as i64)
        .bind(commitment.as_ref())
        .execute(&mut **tx)
        .await?;

    Ok(())
}
