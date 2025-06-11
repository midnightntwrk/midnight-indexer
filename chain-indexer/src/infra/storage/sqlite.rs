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
    Block, BlockInfo, BlockTransactions, ContractAction, Transaction, UnshieldedUtxo,
    storage::Storage,
};
use futures::{StreamExt, TryStreamExt};
use indexer_common::{
    domain::{ByteArray, ByteVec, ContractActionVariant, Identifier},
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
                parent_hash,
                timestamp
            FROM blocks
            WHERE height = $1
        "};

        let (block_id, block_parent_hash, block_timestamp) =
            sqlx::query_as::<_, (i64, Vec<u8>, i64)>(sql)
                .bind(block_height as i64)
                .fetch_one(&*self.pool)
                .await?;
        let block_parent_hash = ByteArray::<32>::try_from(block_parent_hash)
            .map_err(|error| sqlx::Error::Decode(error.into()))?;

        let sql = indoc! {"
            SELECT transactions.raw
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
            block_parent_hash,
            block_timestamp: block_timestamp as u64,
        })
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
        save_contract_actions(&transaction.contract_actions, *transaction_id, tx).await?;

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
                INSERT INTO unshielded_utxos
                (creating_transaction_id, output_index, owner_address, token_type, intent_hash, value, spending_transaction_id)
                VALUES ($1, $2, $3, $4, $5, $6, $7)
                ON CONFLICT (intent_hash, output_index)
                DO UPDATE SET spending_transaction_id = $7
                WHERE unshielded_utxos.spending_transaction_id IS NULL
            "};

            sqlx::query(query)
                .bind(*transaction_id)
                .bind(utxo_info_for_spending.output_index as i32)
                .bind(&utxo_info_for_spending.owner_address)
                .bind(utxo_info_for_spending.token_type.as_ref())
                .bind(utxo_info_for_spending.intent_hash.as_ref())
                .bind(U128BeBytes::from(utxo_info_for_spending.value))
                .bind(transaction_id)
                .execute(&mut **tx)
                .await?;
        }
    } else {
        let query_base = indoc! {"
            INSERT INTO unshielded_utxos
             (creating_transaction_id, output_index, owner_address, token_type, intent_hash, value)
        "};
        let mut query_builder = QueryBuilder::new(query_base);
        query_builder.push_values(utxos.iter(), |mut q, utxo| {
            q.push_bind(transaction_id)
                .push_bind(utxo.output_index as i32)
                .push_bind(&utxo.owner_address)
                .push_bind(utxo.token_type.as_ref())
                .push_bind(utxo.intent_hash.as_ref())
                .push_bind(U128BeBytes::from(utxo.value));
        });

        let query = query_builder.build();
        query.execute(&mut **tx).await?;
    }

    Ok(())
}

async fn save_identifiers(
    identifiers: &[Identifier],
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
) -> Result<(), sqlx::Error> {
    if !contract_actions.is_empty() {
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

        QueryBuilder::new(query)
            .push_values(contract_actions.iter(), |mut q, action| {
                q.push_bind(transaction_id)
                    .push_bind(&action.address)
                    .push_bind(&action.state)
                    .push_bind(&action.zswap_state)
                    .push_bind(ContractActionVariant::from(&action.attributes))
                    .push_bind(Json(&action.attributes));
            })
            .build()
            .execute(&mut **tx)
            .await?;
    }

    Ok(())
}
