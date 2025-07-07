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

use crate::domain::{
    Block, BlockInfo, BlockTransactions, ContractAction, Transaction, storage::Storage,
};
use futures::{StreamExt, TryStreamExt};
use indexer_common::{
    domain::{
        ByteArray, ByteVec, ContractActionVariant, ContractBalance, DustCommitment, DustNullifier,
        DustOwner, RawTransaction, RawTransactionIdentifier, UnshieldedUtxo,
        dust::{DustEvent, DustEventDetails, DustGenerationInfo, DustRegistration, DustUtxo},
    },
    infra::{pool::sqlite::SqlitePool, sqlx::U128BeBytes},
};
use indoc::indoc;
use sqlx::{QueryBuilder, Row, Sqlite, sqlite::SqliteRow, types::Json};
use std::iter;

type Tx = sqlx::Transaction<'static, Sqlite>;

/// Sqlite based implementation of [Storage].
#[derive(Debug, Clone)]
pub struct SqliteStorage {
    pool: SqlitePool,
}

impl SqliteStorage {
    /// Create a new [SqliteStorage].
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

impl Storage for SqliteStorage {
    async fn get_highest_block(&self) -> Result<Option<BlockInfo>, sqlx::Error> {
        let query = indoc! {"
            SELECT hash, height
            FROM blocks
            ORDER BY height DESC
            LIMIT 1
        "};

        sqlx::query(query)
            .fetch_optional(&*self.pool)
            .await?
            .map(|row: SqliteRow| {
                let hash = row.try_get::<Vec<u8>, _>("hash")?.try_into().map_err(|_| {
                    sqlx::Error::Decode("cannot convert hash into 32-byte array".into())
                })?;

                let height = row.try_get::<i64, _>("height")? as u32;

                Ok(BlockInfo { hash, height })
            })
            .transpose()
    }

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

    async fn save_block(&self, block: &mut Block) -> Result<Option<u64>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let (max_transaction_id, transaction_ids) = save_block(block, &mut tx).await?;
        tx.commit().await?;

        for (transaction, id) in block.transactions.iter_mut().zip(transaction_ids.iter()) {
            transaction.id = *id as u64;
        }

        Ok(max_transaction_id)
    }

    async fn save_unshielded_utxos(
        &self,
        utxos: &[UnshieldedUtxo],
        transaction_id: &i64,
        spent: bool,
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        save_unshielded_utxos(utxos, transaction_id, spent, &mut tx).await?;
        tx.commit().await
    }

    async fn get_block_transactions(
        &self,
        block_height: u32,
    ) -> Result<BlockTransactions, sqlx::Error> {
        let sql = indoc! {"
            SELECT
                id,
                protocol_version,
                parent_hash,
                timestamp
            FROM blocks
            WHERE height = $1
        "};

        let (block_id, protocol_version, block_parent_hash, block_timestamp) =
            sqlx::query_as::<_, (i64, i64, Vec<u8>, i64)>(sql)
                .bind(block_height as i64)
                .fetch_one(&*self.pool)
                .await?;
        let block_parent_hash = ByteArray::<32>::try_from(block_parent_hash)
            .map_err(|error| sqlx::Error::Decode(error.into()))?;

        let sql = indoc! {"
            SELECT raw
            FROM transactions
            WHERE block_id = $1
        "};

        let transactions = sqlx::query_as::<_, (ByteVec,)>(sql)
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

    // DUST-specific storage methods.

    async fn save_dust_events(
        &self,
        events: &[DustEvent],
        transaction_id: u64,
    ) -> Result<(), sqlx::Error> {
        if events.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;

        for event in events {
            let event_type = match &event.event_details {
                DustEventDetails::DustInitialUtxo { .. } => "DustInitialUtxo",
                DustEventDetails::DustGenerationDtimeUpdate { .. } => "DustGenerationDtimeUpdate",
                DustEventDetails::DustSpendProcessed { .. } => "DustSpendProcessed",
                _ => "Unknown",
            };

            // Store event details as JSON for SQLite.

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

            sqlx::query(query)
                .bind(transaction_id as i64)
                .bind(event.transaction_hash.as_ref())
                .bind(event.logical_segment as i32)
                .bind(event.physical_segment as i32)
                .bind(event_type)
                .bind(Json(&event.event_details))
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await
    }

    async fn save_dust_utxos(&self, utxos: &[DustUtxo]) -> Result<(), sqlx::Error> {
        if utxos.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;

        // SQLite doesn't support batch inserts with QueryBuilder the same way.
        // So we insert one by one.
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
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await
    }

    async fn save_dust_generation_info(
        &self,
        generation_info: &[DustGenerationInfo],
    ) -> Result<(), sqlx::Error> {
        if generation_info.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;

        for info in generation_info {
            let query = indoc! {"
                INSERT INTO dust_generation_info (
                    value,
                    owner,
                    nonce,
                    ctime,
                    dtime,
                    merkle_index
                )
                VALUES ($1, $2, $3, $4, $5, $6)
            "};

            sqlx::query(query)
                .bind(U128BeBytes::from(info.value))
                .bind(info.owner.as_ref())
                .bind(info.nonce.as_ref())
                .bind(info.ctime as i64)
                .bind(if info.dtime == 0 {
                    None
                } else {
                    Some(info.dtime as i64)
                })
                .bind(0i64) // TODO: merkle_index should come from somewhere.
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await
    }

    async fn save_cnight_registrations(
        &self,
        registrations: &[DustRegistration],
    ) -> Result<(), sqlx::Error> {
        if registrations.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;

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
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await
    }

    async fn get_dust_generation_info_by_owner(
        &self,
        owner: DustOwner,
    ) -> Result<Vec<DustGenerationInfo>, sqlx::Error> {
        let query = indoc! {"
            SELECT value, owner, nonce, ctime, dtime
            FROM dust_generation_info
            WHERE owner = $1
            ORDER BY ctime DESC
        "};

        let rows = sqlx::query(query)
            .bind(owner.as_ref())
            .fetch_all(&*self.pool)
            .await?;

        let mut results = Vec::new();
        for row in rows {
            let value_bytes: Vec<u8> = row.try_get("value")?;
            let value = u128::from_be_bytes(
                value_bytes
                    .as_slice()
                    .try_into()
                    .map_err(|_| sqlx::Error::Decode("Invalid u128 value length".into()))?,
            );

            let owner_bytes: Vec<u8> = row.try_get("owner")?;
            let owner = ByteArray(
                owner_bytes
                    .try_into()
                    .map_err(|_| sqlx::Error::Decode("Invalid owner length".into()))?,
            );

            let nonce_bytes: Vec<u8> = row.try_get("nonce")?;
            let nonce = ByteArray(
                nonce_bytes
                    .try_into()
                    .map_err(|_| sqlx::Error::Decode("Invalid nonce length".into()))?,
            );

            results.push(DustGenerationInfo {
                value,
                owner,
                nonce,
                ctime: row.try_get::<i64, _>("ctime")? as u64,
                dtime: row
                    .try_get::<Option<i64>, _>("dtime")?
                    .map(|dt| dt as u64)
                    .unwrap_or(0),
            });
        }

        Ok(results)
    }

    async fn get_dust_utxos_by_owner(
        &self,
        owner: DustOwner,
    ) -> Result<Vec<DustUtxo>, sqlx::Error> {
        let query = indoc! {"
            SELECT commitment, nullifier, initial_value, owner, nonce, seq, ctime,
                   generation_info_id, spent_at_transaction_id
            FROM dust_utxos
            WHERE owner = $1
            ORDER BY ctime DESC
        "};

        let rows = sqlx::query(query)
            .bind(owner.as_ref())
            .fetch_all(&*self.pool)
            .await?;

        let mut results = Vec::new();
        for row in rows {
            let commitment_bytes: Vec<u8> = row.try_get("commitment")?;
            let commitment = ByteArray(
                commitment_bytes
                    .try_into()
                    .map_err(|_| sqlx::Error::Decode("Invalid commitment length".into()))?,
            );

            let nullifier = row
                .try_get::<Option<Vec<u8>>, _>("nullifier")?
                .map(|bytes| {
                    let arr: [u8; 32] = bytes
                        .try_into()
                        .map_err(|_| sqlx::Error::Decode("Invalid nullifier length".into()))?;
                    Ok::<_, sqlx::Error>(ByteArray(arr))
                })
                .transpose()?;

            let value_bytes: Vec<u8> = row.try_get("initial_value")?;
            let initial_value = u128::from_be_bytes(
                value_bytes
                    .as_slice()
                    .try_into()
                    .map_err(|_| sqlx::Error::Decode("Invalid u128 value length".into()))?,
            );

            let owner_bytes: Vec<u8> = row.try_get("owner")?;
            let owner = ByteArray(
                owner_bytes
                    .try_into()
                    .map_err(|_| sqlx::Error::Decode("Invalid owner length".into()))?,
            );

            let nonce_bytes: Vec<u8> = row.try_get("nonce")?;
            let nonce = ByteArray(
                nonce_bytes
                    .try_into()
                    .map_err(|_| sqlx::Error::Decode("Invalid nonce length".into()))?,
            );

            results.push(DustUtxo {
                commitment,
                nullifier,
                initial_value,
                owner,
                nonce,
                seq: row.try_get::<i32, _>("seq")? as u32,
                ctime: row.try_get::<i64, _>("ctime")? as u64,
                generation_info_id: row
                    .try_get::<Option<i64>, _>("generation_info_id")?
                    .map(|id| id as u64),
                spent_at_transaction_id: row
                    .try_get::<Option<i64>, _>("spent_at_transaction_id")?
                    .map(|id| id as u64),
            });
        }

        Ok(results)
    }

    async fn search_transactions_by_nullifier_prefix(
        &self,
        prefix: &str,
        after_block: Option<u32>,
    ) -> Result<Vec<(u64, RawTransaction)>, sqlx::Error> {
        // Use the prefix index for privacy-preserving search.
        let prefix_len = prefix.len().min(8); // Max prefix length supported by index.
        let truncated_prefix = &prefix[..prefix_len];

        let query = if let Some(_block_height) = after_block {
            indoc! {"
                SELECT DISTINCT dust_utxos.spent_at_transaction_id, transactions.raw
                FROM dust_utxos
                JOIN transactions ON dust_utxos.spent_at_transaction_id = transactions.id
                JOIN blocks ON transactions.block_id = blocks.id
                WHERE substr(hex(dust_utxos.nullifier), 1, $1) = $2
                  AND dust_utxos.nullifier IS NOT NULL
                  AND dust_utxos.spent_at_transaction_id IS NOT NULL
                  AND blocks.height > $3
                ORDER BY dust_utxos.spent_at_transaction_id
            "}
        } else {
            indoc! {"
                SELECT DISTINCT dust_utxos.spent_at_transaction_id, transactions.raw
                FROM dust_utxos
                JOIN transactions ON dust_utxos.spent_at_transaction_id = transactions.id
                WHERE substr(hex(dust_utxos.nullifier), 1, $1) = $2
                  AND dust_utxos.nullifier IS NOT NULL
                  AND dust_utxos.spent_at_transaction_id IS NOT NULL
                ORDER BY dust_utxos.spent_at_transaction_id
            "}
        };

        let mut query_builder = sqlx::query(query)
            .bind(prefix_len as i32)
            .bind(truncated_prefix);

        if let Some(block_height) = after_block {
            query_builder = query_builder.bind(block_height as i32);
        }

        let rows = query_builder.fetch_all(&*self.pool).await?;

        let results = rows
            .into_iter()
            .map(|row| {
                let tx_id: i64 = row.try_get(0)?;
                let raw_tx: RawTransaction = row.try_get(1)?;
                Ok((tx_id as u64, raw_tx))
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?;

        Ok(results)
    }

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

    async fn mark_dust_utxo_spent(
        &self,
        commitment: DustCommitment,
        nullifier: DustNullifier,
        transaction_id: u64,
    ) -> Result<(), sqlx::Error> {
        let query = indoc! {"
            UPDATE dust_utxos
            SET nullifier = $1,
                spent_at_transaction_id = $2
            WHERE commitment = $3
              AND spent_at_transaction_id IS NULL
        "};

        let result = sqlx::query(query)
            .bind(nullifier.as_ref())
            .bind(transaction_id as i64)
            .bind(commitment.as_ref())
            .execute(&*self.pool)
            .await?;

        if result.rows_affected() == 0 {
            // Either UTXO doesn't exist or is already spent.
            return Err(sqlx::Error::RowNotFound);
        }

        Ok(())
    }
}

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
        .push_values(iter::once(block), |mut q, block| {
            let Block {
                hash,
                height,
                protocol_version,
                parent_hash,
                author,
                timestamp,
                ..
            } = block;
            q.push_bind(hash.as_ref())
                .push_bind(*height as i64)
                .push_bind(protocol_version.0 as i64)
                .push_bind(parent_hash.as_ref())
                .push_bind(author.as_ref().map(|a| a.as_ref()))
                .push_bind(*timestamp as i64);
        })
        .push(" RETURNING id")
        .build()
        .fetch_one(&mut **tx)
        .await?
        .try_get::<i64, _>("id")?;

    save_transactions(&block.transactions, block_id, tx).await
}

async fn save_transactions(
    transactions: &[Transaction],
    block_id: i64,
    tx: &mut Tx,
) -> Result<(Option<u64>, Vec<i64>), sqlx::Error> {
    if transactions.is_empty() {
        return Ok((None, vec![]));
    }

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
            let Transaction {
                hash,
                protocol_version,
                transaction_result,
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
        })
        .push(" RETURNING id")
        .build()
        .fetch(&mut **tx)
        .map(|row| row.and_then(|row| row.try_get::<i64, _>("id")))
        .try_collect::<Vec<_>>()
        .await?;

    for (transaction, transaction_id) in transactions.iter().zip(transaction_ids.iter()) {
        save_identifiers(&transaction.identifiers, *transaction_id, tx).await?;
        let contract_action_ids =
            save_contract_actions(&transaction.contract_actions, *transaction_id, tx).await?;

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
    }

    let max_id = transaction_ids.iter().max().copied().map(|n| n as u64);
    Ok((max_id, transaction_ids))
}

async fn save_unshielded_utxos(
    utxos: &[UnshieldedUtxo],
    transaction_id: &i64,
    spent: bool,
    tx: &mut Tx,
) -> Result<(), sqlx::Error> {
    if utxos.is_empty() {
        return Ok(());
    }

    if spent {
        for utxo_info_for_spending in utxos {
            let query = indoc! {"
                INSERT INTO unshielded_utxos (
                    creating_transaction_id,
                    output_index,
                    owner_address,
                    token_type,
                    intent_hash,
                    value,
                    spending_transaction_id
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7)
                ON CONFLICT (intent_hash, output_index)
                DO UPDATE SET spending_transaction_id = $7
                WHERE unshielded_utxos.spending_transaction_id IS NULL
            "};

            sqlx::query(query)
                .bind(*transaction_id)
                .bind(utxo_info_for_spending.output_index as i32)
                .bind(utxo_info_for_spending.owner_address.as_ref())
                .bind(utxo_info_for_spending.token_type.as_ref())
                .bind(utxo_info_for_spending.intent_hash.as_ref())
                .bind(U128BeBytes::from(utxo_info_for_spending.value))
                .bind(transaction_id)
                .execute(&mut **tx)
                .await?;
        }
    } else {
        let query_base = indoc! {"
            INSERT INTO unshielded_utxos (
                creating_transaction_id,
                output_index,
                owner_address,
                token_type,
                intent_hash,
                value
            )
        "};

        QueryBuilder::new(query_base)
            .push_values(utxos.iter(), |mut q, utxo| {
                q.push_bind(transaction_id)
                    .push_bind(utxo.output_index as i32)
                    .push_bind(utxo.owner_address.as_ref())
                    .push_bind(utxo.token_type.as_ref())
                    .push_bind(utxo.intent_hash.as_ref())
                    .push_bind(U128BeBytes::from(utxo.value));
            })
            .build()
            .execute(&mut **tx)
            .await?;
    }

    Ok(())
}

async fn save_identifiers(
    identifiers: &[RawTransactionIdentifier],
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

async fn save_contract_actions(
    contract_actions: &[ContractAction],
    transaction_id: i64,
    tx: &mut Tx,
) -> Result<Vec<i64>, sqlx::Error> {
    if contract_actions.is_empty() {
        return Ok(Vec::new());
    }

    let mut contract_action_ids = Vec::new();

    // SQLite doesn't support RETURNING in bulk inserts, so we insert one by one
    for action in contract_actions {
        let query = indoc! {"
            INSERT INTO contract_actions (
                transaction_id,
                address,
                state,
                zswap_state,
                variant,
                attributes
            )
            VALUES (?, ?, ?, ?, ?, ?)
        "};

        let result = sqlx::query(query)
            .bind(transaction_id)
            .bind(&action.address)
            .bind(&action.state)
            .bind(&action.zswap_state)
            .bind(ContractActionVariant::from(&action.attributes))
            .bind(Json(&action.attributes))
            .execute(&mut **tx)
            .await?;

        contract_action_ids.push(result.last_insert_rowid());
    }

    Ok(contract_action_ids)
}

async fn save_contract_balances(
    balances: Vec<(i64, ContractBalance)>,
    tx: &mut Tx,
) -> Result<(), sqlx::Error> {
    // Insert balances one by one (SQLite limitation).
    for (action_id, balance) in balances {
        let query = indoc! {"
            INSERT INTO contract_balances (
                contract_action_id,
                token_type,
                amount
            )
            VALUES (?, ?, ?)
        "};

        sqlx::query(query)
            .bind(action_id)
            .bind(balance.token_type.as_ref())
            .bind(U128BeBytes::from(balance.amount))
            .execute(&mut **tx)
            .await?;
    }

    Ok(())
}
