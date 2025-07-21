// This file is part of midnight-indexer.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::domain::{self, Block, BlockInfo, BlockTransactions, ContractAction, Transaction};
use fastrace::trace;
use futures::{TryFutureExt, TryStreamExt};
use indexer_common::{
    domain::{
        BlockHash, ByteArray, ByteVec, ContractActionVariant, ContractBalance, UnshieldedUtxo,
        dust::{DustEvent, DustEventDetails},
    },
    infra::sqlx::U128BeBytes,
};
use indoc::indoc;
use sqlx::{QueryBuilder, types::Json};

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
    #[trace]
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

    #[trace]
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

    #[trace]
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

    #[trace(properties = { "block_height": "{block_height}" })]
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

    #[trace]
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
}

#[trace]
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

#[trace(properties = { "block_id": "{block_id}" })]
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

        process_dust_events(&transaction.dust_events, transaction_id, tx).await?;

        save_dust_events(&transaction.dust_events, transaction_id, tx).await?;
    }

    let max_id = transaction_ids.last().map(|&n| n as u64);
    Ok((max_id, transaction_ids))
}

#[trace(properties = { "transaction_id": "{transaction_id}" })]
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

#[trace(properties = { "transaction_id": "{transaction_id}" })]
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

#[trace]
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

#[cfg_attr(feature = "cloud", trace(properties = { "transaction_id": "{transaction_id}" }))]
async fn process_dust_events(
    dust_events: &[DustEvent],
    transaction_id: i64,
    tx: &mut Tx,
) -> Result<(), sqlx::Error> {
    let mut generation_dtime = None;

    for dust_event in dust_events {
        match dust_event.event_details {
            DustEventDetails::DustInitialUtxo {
                output, generation, ..
            } => {
                let generation_info_id = save_dust_generation_info(generation, tx).await?;
                save_dust_utxos(output, generation_info_id, tx).await?;
            }

            DustEventDetails::DustGenerationDtimeUpdate {
                generation,
                generation_index,
            } => generation_dtime = Some((generation.dtime, generation_index)),

            DustEventDetails::DustSpendProcessed {
                commitment,
                nullifier,
                ..
            } => {
                mark_dust_utxo_spent(commitment, nullifier, transaction_id, tx).await?;
            }
        }
    }

    if let Some((dtime, index)) = generation_dtime {
        update_dust_generation_dtime(dtime, index, tx).await?;
    }

    Ok(())
}

#[cfg_attr(feature = "cloud", trace(properties = { "transaction_id": "{transaction_id}" }))]
async fn save_dust_events(
    dust_events: &[DustEvent],
    transaction_id: i64,
    tx: &mut Tx,
) -> Result<(), sqlx::Error> {
    if dust_events.is_empty() {
        return Ok(());
    }

    let query = indoc! {"
        INSERT INTO dust_events (
            transaction_id,
            details
        )
    "};

    QueryBuilder::new(query)
        .push_values(dust_events.iter(), |mut q, event| {
            q.push_bind(transaction_id).push_bind(Json(event));
        })
        .build()
        .execute(&mut **tx)
        .await?;

    Ok(())
}

#[trace]
async fn save_dust_generation_info(
    generation: indexer_common::domain::dust::DustGenerationInfo,
    tx: &mut Tx,
) -> Result<u64, sqlx::Error> {
    let query = indoc! {"
        INSERT INTO dust_generation_info (
            value,
            owner,
            nonce,
            ctime,
            dtime
        )
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id
    "};

    let (id,) = sqlx::query_as::<_, (i64,)>(query)
        .bind(U128BeBytes::from(generation.value))
        .bind(generation.owner.as_ref())
        .bind(generation.nonce.as_ref())
        .bind(generation.ctime as i64)
        .bind(generation.dtime as i64)
        .fetch_one(&mut **tx)
        .await?;

    Ok(id as u64)
}

#[trace]
async fn save_dust_utxos(
    output: indexer_common::domain::dust::QualifiedDustOutput,
    generation_info_id: u64,
    tx: &mut Tx,
) -> Result<(), sqlx::Error> {
    let query = indoc! {"
        INSERT INTO dust_utxos (
            commitment,
            initial_value,
            owner,
            nonce,
            seq,
            ctime,
            generation_info_id
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
    "};

    // Calculate commitment (in real implementation, this would use proper crypto).
    let commitment = output.nonce; // Placeholder - should be properly calculated.

    sqlx::query(query)
        .bind(commitment.as_ref())
        .bind(U128BeBytes::from(output.initial_value))
        .bind(output.owner.as_ref())
        .bind(output.nonce.as_ref())
        .bind(output.seq as i64)
        .bind(output.ctime as i64)
        .bind(generation_info_id as i64)
        .execute(&mut **tx)
        .await?;

    Ok(())
}

#[trace(properties = { "transaction_id": "{transaction_id}" })]
async fn mark_dust_utxo_spent(
    commitment: ByteArray<32>,
    nullifier: ByteArray<32>,
    transaction_id: i64,
    tx: &mut Tx,
) -> Result<(), sqlx::Error> {
    let query = indoc! {"
                UPDATE dust_utxos
                SET nullifier = $1, spent_at_transaction_id = $2
                WHERE nullifier IS NULL
                AND commitment = $3
            "};

    sqlx::query(query)
        .bind(nullifier.as_ref())
        .bind(transaction_id)
        .bind(commitment.as_ref())
        .execute(&mut **tx)
        .await?;

    Ok(())
}

#[trace]
async fn update_dust_generation_dtime(
    dtime: u64,
    index: u64,
    tx: &mut Tx,
) -> Result<(), sqlx::Error> {
    let query = indoc! {"
            UPDATE dust_generation_info
            SET dtime = $1
            WHERE id = $2
        "};

    sqlx::query(query)
        .bind(dtime as i64)
        .bind(index as i64)
        .execute(&mut **tx)
        .await?;

    Ok(())
}
