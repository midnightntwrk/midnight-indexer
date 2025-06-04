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
    Block, BlockHash, ContractAction, ContractAttributes, Storage, Transaction, UnshieldedUtxo,
    UnshieldedUtxoFilter,
};
use async_stream::try_stream;
use chacha20poly1305::ChaCha20Poly1305;
use derive_more::Debug;
use futures::{Stream, stream::TryStreamExt};
use indexer_common::{
    domain::{
        ContractAddress, Identifier, SessionId, TransactionHash, UnshieldedAddress, ViewingKey,
    },
    infra::pool::sqlite::SqlitePool,
    stream::flatten_chunks,
};
use indoc::indoc;
use sqlx::{
    Database, Row, Sqlite,
    types::{Uuid, time::OffsetDateTime},
};
use std::num::NonZeroU32;

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
}

const TX_BY_ID_QUERY: &str = indoc! {"
            SELECT
                transactions.id,
                transactions.hash,
                blocks.hash AS block_hash,
                transactions.protocol_version,
                transactions.transaction_result,
                transactions.raw,
                transactions.merkle_tree_root,
                transactions.start_index,
                transactions.end_index
            FROM transactions
            INNER JOIN blocks ON blocks.id = transactions.block_id
            WHERE transactions.id = $1
        "};

impl Storage for SqliteStorage {
    async fn get_latest_block(&self) -> Result<Option<Block>, sqlx::Error> {
        let query = indoc! {"
            SELECT *
            FROM blocks
            ORDER BY height DESC
            LIMIT 1
        "};

        sqlx::query_as::<_, Block>(query)
            .fetch_optional(&*self.pool)
            .await
    }

    async fn get_block_by_hash(&self, hash: BlockHash) -> Result<Option<Block>, sqlx::Error> {
        let query = indoc! {"
            SELECT *
            FROM blocks
            WHERE hash = $1
            LIMIT 1
        "};

        sqlx::query_as::<_, Block>(query)
            .bind(hash.as_ref())
            .fetch_optional(&*self.pool)
            .await
    }

    async fn get_block_by_height(&self, height: u32) -> Result<Option<Block>, sqlx::Error> {
        let query = indoc! {"
            SELECT *
            FROM blocks
            WHERE height = $1
            LIMIT 1
        "};

        sqlx::query_as::<_, Block>(query)
            .bind(height as i64)
            .fetch_optional(&*self.pool)
            .await
    }

    fn get_blocks(
        &self,
        mut height: u32,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<Block, sqlx::Error>> {
        let chunks = try_stream! {
            loop {
                let query = indoc! {"
                    SELECT *
                    FROM blocks
                    WHERE height >= $1
                    ORDER BY id
                    LIMIT $2
                "};

                let blocks = sqlx::query_as::<_, Block>(query)
                    .bind(height as i64)
                    .bind(batch_size.get() as i64)
                    .fetch_all(&*self.pool)
                    .await?;

                if blocks.is_empty() {
                    break;
                } else {
                    height += blocks.len() as u32;
                }

                yield blocks;
            }
        };

        flatten_chunks(chunks)
    }

    async fn get_transaction_by_id(&self, id: u64) -> Result<Transaction, sqlx::Error> {
        let mut transaction = sqlx::query_as::<_, Transaction>(TX_BY_ID_QUERY)
            .bind(id as i64)
            .fetch_one(&*self.pool)
            .await?;

        let identifiers = self
            .get_identifiers_by_transaction_id(transaction.id)
            .await?;
        transaction.identifiers = identifiers;

        transaction.unshielded_created_outputs = self
            .get_unshielded_utxos(None, UnshieldedUtxoFilter::CreatedByTx(transaction.id))
            .await?;
        transaction.unshielded_spent_outputs = self
            .get_unshielded_utxos(None, UnshieldedUtxoFilter::SpentByTx(transaction.id))
            .await?;

        Ok(transaction)
    }

    async fn get_transactions_by_block_id(&self, id: u64) -> Result<Vec<Transaction>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                transactions.id,
                transactions.hash,
                blocks.hash AS block_hash,
                transactions.protocol_version,
                transactions.transaction_result,
                transactions.raw,
                transactions.merkle_tree_root,
                transactions.start_index,
                transactions.end_index
            FROM transactions
            INNER JOIN blocks ON blocks.id = transactions.block_id
            WHERE transactions.block_id = $1
        "};

        let mut transactions = sqlx::query_as::<_, Transaction>(query)
            .bind(id as i64)
            .fetch_all(&*self.pool)
            .await?;

        for transaction in transactions.iter_mut() {
            let identifiers = self
                .get_identifiers_by_transaction_id(transaction.id)
                .await?;
            transaction.identifiers = identifiers;
        }

        Ok(transactions)
    }

    async fn get_transactions_by_hash(
        &self,
        hash: TransactionHash,
    ) -> Result<Vec<Transaction>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                transactions.id,
                transactions.hash,
                blocks.hash AS block_hash,
                transactions.protocol_version,
                transactions.transaction_result,
                transactions.raw,
                transactions.merkle_tree_root,
                transactions.start_index,
                transactions.end_index
            FROM transactions
            INNER JOIN blocks ON blocks.id = transactions.block_id
            WHERE transactions.hash = $1
            ORDER BY transactions.id DESC
        "};

        let mut transactions = sqlx::query_as::<_, Transaction>(query)
            .bind(hash.as_ref())
            .fetch_all(&*self.pool)
            .await?;

        for transaction in transactions.iter_mut() {
            let identifiers = self
                .get_identifiers_by_transaction_id(transaction.id)
                .await?;
            transaction.identifiers = identifiers;
        }

        for transaction in transactions.iter_mut() {
            transaction.unshielded_created_outputs = self
                .get_unshielded_utxos(None, UnshieldedUtxoFilter::CreatedByTx(transaction.id))
                .await?;
            transaction.unshielded_spent_outputs = self
                .get_unshielded_utxos(None, UnshieldedUtxoFilter::SpentByTx(transaction.id))
                .await?;
        }

        Ok(transactions)
    }

    async fn get_transactions_by_identifier(
        &self,
        identifier: &Identifier,
    ) -> Result<Vec<Transaction>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                transactions.id,
                transactions.hash,
                blocks.hash AS block_hash,
                transactions.protocol_version,
                transactions.transaction_result,
                transactions.raw,
                transactions.merkle_tree_root,
                transactions.start_index,
                transactions.end_index
            FROM transactions
            INNER JOIN blocks ON blocks.id = transactions.block_id
            INNER JOIN transaction_identifiers ON transactions.id = transaction_identifiers.transaction_id
            WHERE transaction_identifiers.identifier = $1
        "};

        let mut transactions = sqlx::query_as::<_, Transaction>(query)
            .bind(identifier)
            .fetch_all(&*self.pool)
            .await?;

        for transaction in transactions.iter_mut() {
            let identifiers = self
                .get_identifiers_by_transaction_id(transaction.id)
                .await?;
            transaction.identifiers = identifiers;
        }

        for transaction in transactions.iter_mut() {
            transaction.unshielded_created_outputs = self
                .get_unshielded_utxos(None, UnshieldedUtxoFilter::CreatedByTx(transaction.id))
                .await?;
            transaction.unshielded_spent_outputs = self
                .get_unshielded_utxos(None, UnshieldedUtxoFilter::SpentByTx(transaction.id))
                .await?;
        }

        Ok(transactions)
    }

    async fn get_contract_deploy_by_address(
        &self,
        address: &ContractAddress,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                contract_actions.id AS id,
                contract_actions.address,
                contract_actions.state,
                contract_actions.attributes,
                contract_actions.zswap_state,
                contract_actions.transaction_id
            FROM contract_actions
            WHERE contract_actions.address = $1
            ORDER BY id ASC
            LIMIT 1
        "};

        let action = sqlx::query_as::<_, ContractAction>(query)
            .bind(address)
            .fetch_optional(&*self.pool)
            .await?;

        if let Some(action) = &action {
            assert_eq!(action.attributes, ContractAttributes::Deploy);
        }

        Ok(action)
    }

    async fn get_contract_action_by_address(
        &self,
        address: &ContractAddress,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                contract_actions.id AS id,
                contract_actions.address,
                contract_actions.state,
                contract_actions.attributes,
                contract_actions.zswap_state,
                contract_actions.transaction_id
            FROM contract_actions
            WHERE contract_actions.address = $1
            ORDER BY id DESC
            LIMIT 1
        "};

        sqlx::query_as::<_, ContractAction>(query)
            .bind(address)
            .fetch_optional(&*self.pool)
            .await
    }

    async fn get_contract_action_by_address_and_block_hash(
        &self,
        address: &ContractAddress,
        hash: BlockHash,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                contract_actions.id AS id,
                contract_actions.address,
                contract_actions.state,
                contract_actions.attributes,
                contract_actions.zswap_state,
                contract_actions.transaction_id
            FROM contract_actions
            INNER JOIN transactions ON transactions.id = contract_actions.transaction_id
            WHERE contract_actions.address = $1
            AND transactions.block_id = (SELECT id FROM blocks WHERE hash = $2)
            AND json_extract(transactions.transaction_result, '$') != 'Failure'
            ORDER BY id DESC
            LIMIT 1
        "};

        sqlx::query_as::<_, ContractAction>(query)
            .bind(address.as_ref())
            .bind(hash.as_ref())
            .fetch_optional(&*self.pool)
            .await
    }

    async fn get_contract_action_by_address_and_block_height(
        &self,
        address: &ContractAddress,
        height: u32,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                contract_actions.id AS id,
                contract_actions.address,
                contract_actions.state,
                contract_actions.attributes,
                contract_actions.zswap_state,
                contract_actions.transaction_id
            FROM contract_actions
            INNER JOIN transactions ON transactions.id = contract_actions.transaction_id
            INNER JOIN blocks ON blocks.id = transactions.block_id
            WHERE contract_actions.address = $1
            AND blocks.height = $2
            AND json_extract(transactions.transaction_result, '$') != 'Failure'
            ORDER BY id DESC
            LIMIT 1
        "};

        sqlx::query_as::<_, ContractAction>(query)
            .bind(address)
            .bind(height as i64)
            .fetch_optional(&*self.pool)
            .await
    }

    async fn get_contract_action_by_address_and_transaction_hash(
        &self,
        address: &ContractAddress,
        hash: TransactionHash,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                contract_actions.id AS id,
                contract_actions.address,
                contract_actions.state,
                contract_actions.attributes,
                contract_actions.zswap_state,
                contract_actions.transaction_id
            FROM contract_actions
            WHERE contract_actions.address = $1
            AND contract_actions.transaction_id = (
                SELECT id FROM transactions
                WHERE hash = $2
                AND json_extract(transaction_result, '$') != 'Failure'
                ORDER BY id DESC
                LIMIT 1
            )
            ORDER BY id DESC
            LIMIT 1
        "};

        sqlx::query_as::<_, ContractAction>(query)
            .bind(address.as_ref())
            .bind(hash.as_ref())
            .fetch_optional(&*self.pool)
            .await
    }

    async fn get_contract_action_by_address_and_transaction_identifier(
        &self,
        address: &ContractAddress,
        identifier: &Identifier,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                contract_actions.id AS id,
                contract_actions.address,
                contract_actions.state,
                contract_actions.attributes,
                contract_actions.zswap_state,
                contract_actions.transaction_id
            FROM contract_actions
            INNER JOIN transactions ON transactions.id = contract_actions.transaction_id
            INNER JOIN transaction_identifiers ON transactions.id = transaction_identifiers.transaction_id
            WHERE contract_actions.address = $1
            AND transaction_identifiers.identifier = $2
            AND json_extract(transactions.transaction_result, '$') != 'Failure'
            ORDER BY id DESC
            LIMIT 1
        "};

        sqlx::query_as::<_, ContractAction>(query)
            .bind(address)
            .bind(identifier)
            .fetch_optional(&*self.pool)
            .await
    }

    async fn get_contract_actions_by_transaction_id(
        &self,
        id: u64,
    ) -> Result<Vec<ContractAction>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                contract_actions.id AS id,
                contract_actions.address,
                contract_actions.state,
                contract_actions.attributes,
                contract_actions.zswap_state,
                contract_actions.transaction_id
            FROM contract_actions
            WHERE contract_actions.transaction_id = $1
        "};

        sqlx::query_as::<_, ContractAction>(query)
            .bind(id as i64)
            .fetch_all(&*self.pool)
            .await
    }

    fn get_contract_actions_by_address(
        &self,
        address: &ContractAddress,
        height: u32,
        mut contract_action_id: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<ContractAction, sqlx::Error>> + Send {
        let chunks = try_stream! {
            loop {
                let query = indoc! {"
                    SELECT
                        contract_actions.id AS id,
                        contract_actions.address,
                        contract_actions.state,
                        contract_actions.attributes,
                        contract_actions.zswap_state,
                        contract_actions.transaction_id
                    FROM contract_actions
                    INNER JOIN transactions ON transactions.id = contract_actions.transaction_id
                    INNER JOIN blocks ON blocks.id = transactions.block_id
                    WHERE contract_actions.address = $1
                    AND blocks.height >= $2
                    AND contract_actions.id >= $3
                    AND json_extract(transactions.transaction_result, '$') != 'Failure'
                    ORDER BY id ASC
                    LIMIT $4
                "};

                let actions = sqlx::query_as::<_, ContractAction>(query)
                    .bind(address)
                    .bind(height as i64)
                    .bind(contract_action_id as i64)
                    .bind(batch_size.get() as i64)
                    .fetch(&*self.pool)
                    .map_ok(ContractAction::from)
                    .try_collect::<Vec<_>>()
                    .await?;

                let max_id = actions.iter().map(|action| action.id).max();
                match max_id {
                    Some(max_id) => contract_action_id = max_id + 1,
                    None => break,
                }

                yield actions;
            }
        };

        flatten_chunks(chunks)
    }

    async fn get_highest_indices(
        &self,
        session_id: SessionId,
    ) -> Result<(Option<u64>, Option<u64>, Option<u64>), sqlx::Error> {
        let query = indoc! {"
            SELECT (
                SELECT MAX(end_index) FROM transactions
            ) AS highest_end_index,
            (
                SELECT MAX(end_index)
                FROM transactions
                INNER JOIN relevant_transactions ON transactions.id = relevant_transactions.transaction_id
            ) AS highest_relevant_end_index,
            (
                SELECT end_index
                FROM transactions
                INNER JOIN relevant_transactions ON transactions.id = relevant_transactions.transaction_id
                INNER JOIN wallets ON wallets.id = relevant_transactions.wallet_id
                WHERE wallets.session_id = $1
                ORDER BY end_index DESC
                LIMIT 1
            ) AS max_end_index_for_session
        "};

        let (highest_index, highest_relevant_index, highest_relevant_wallet_index) =
            sqlx::query_as::<_, (Option<i64>, Option<i64>, Option<i64>)>(query)
                .bind(session_id.as_ref())
                .fetch_one(&*self.pool)
                .await?;

        Ok((
            highest_index.map(|n| n as u64),
            highest_relevant_index.map(|n| n as u64),
            highest_relevant_wallet_index.map(|n| n as u64),
        ))
    }

    fn get_relevant_transactions(
        &self,
        session_id: SessionId,
        mut index: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<Transaction, sqlx::Error>> + Send {
        let chunks = try_stream! {
            loop {
                let query = indoc! {"
                    SELECT
                        transactions.id,
                        transactions.hash,
                        blocks.hash AS block_hash,
                        transactions.protocol_version,
                        transactions.transaction_result,
                        transactions.raw,
                        transactions.merkle_tree_root,
                        transactions.start_index,
                        transactions.end_index
                    FROM transactions
                    INNER JOIN blocks ON blocks.id = transactions.block_id
                    INNER JOIN relevant_transactions ON transactions.id = relevant_transactions.transaction_id
                    INNER JOIN wallets ON wallets.id = relevant_transactions.wallet_id
                    WHERE wallets.session_id = $1
                    AND transactions.start_index >= $2
                    ORDER BY transactions.id
                    LIMIT $3
                "};

                let mut transactions = sqlx::query_as::<_, Transaction>(query)
                    .bind(session_id.as_ref())
                    .bind(index as i64)
                    .bind(batch_size.get() as i64)
                    .fetch_all(&*self.pool)
                    .await?;

                index = match transactions.iter().map(|t| t.end_index).max() {
                    Some(end_index) => end_index + 1,
                    None => break,
                };

                for transaction in transactions.iter_mut() {
                    let identifiers = self.
                        get_identifiers_by_transaction_id(transaction.id).await?;
                    transaction.identifiers = identifiers;

                    transaction.unshielded_created_outputs =
                        self.get_unshielded_utxos(None, UnshieldedUtxoFilter::CreatedByTx(transaction.id)).await?;
                    transaction.unshielded_spent_outputs =
                        self.get_unshielded_utxos(None, UnshieldedUtxoFilter::SpentByTx(transaction.id)).await?;
                }

                yield transactions;
            }
        };

        flatten_chunks(chunks)
    }

    async fn connect_wallet(&self, viewing_key: &ViewingKey) -> Result<(), sqlx::Error> {
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
                last_active
            )
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (session_id)
            DO UPDATE SET active = TRUE, last_active = $4
        "};

        sqlx::query(query)
            .bind(id)
            .bind(session_id.as_ref())
            .bind(viewing_key)
            .bind(OffsetDateTime::now_utc())
            .execute(&*self.pool)
            .await?;

        Ok(())
    }

    async fn disconnect_wallet(&self, session_id: SessionId) -> Result<(), sqlx::Error> {
        let query = indoc! {"
            UPDATE wallets
            SET active = FALSE
            WHERE session_id = $1
        "};

        sqlx::query(query)
            .bind(session_id.as_ref())
            .execute(&*self.pool)
            .await?;

        Ok(())
    }

    async fn set_wallet_active(&self, session_id: SessionId) -> Result<(), sqlx::Error> {
        let query = indoc! {"
            UPDATE wallets
            SET active = TRUE, last_active = $2
            WHERE session_id = $1
        "};

        sqlx::query(query)
            .bind(session_id.as_ref())
            .bind(OffsetDateTime::now_utc())
            .execute(&*self.pool)
            .await?;

        Ok(())
    }

    async fn get_unshielded_utxos(
        &self,
        address: Option<&UnshieldedAddress>,
        filter: UnshieldedUtxoFilter<'_>,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        // Build the appropriate SQL based on filter type
        let sql = match (&address, &filter) {
            (Some(_), UnshieldedUtxoFilter::All) => {
                indoc! {"
                SELECT
                    id, owner_address, token_type, value, output_index, intent_hash,
                    creating_transaction_id, spending_transaction_id
                FROM unshielded_utxos
                WHERE owner_address = ?
                ORDER BY id ASC
            "}
            }
            (None, UnshieldedUtxoFilter::CreatedByTx(_)) => {
                indoc! {"
                SELECT *
                FROM unshielded_utxos
                WHERE creating_transaction_id = ?
            "}
            }
            (None, UnshieldedUtxoFilter::SpentByTx(_)) => {
                indoc! {"
                SELECT *
                FROM unshielded_utxos
                WHERE spending_transaction_id = ?
            "}
            }
            (Some(_), UnshieldedUtxoFilter::CreatedInTxForAddress(_)) => {
                indoc! {"
                SELECT *
                FROM unshielded_utxos
                WHERE creating_transaction_id = ?
                AND owner_address = ?
            "}
            }
            (Some(_), UnshieldedUtxoFilter::SpentInTxForAddress(_)) => {
                indoc! {"
                SELECT *
                FROM unshielded_utxos
                WHERE spending_transaction_id = ?
                AND owner_address = ?
            "}
            }
            (Some(_), UnshieldedUtxoFilter::FromHeight(_)) => {
                indoc! {"
                SELECT unshielded_utxos.*
                FROM   unshielded_utxos
                JOIN   transactions  ON   transactions.id = unshielded_utxos.creating_transaction_id
                JOIN   blocks        ON   blocks.id = transactions.block_id
                WHERE  unshielded_utxos.owner_address = ?
                  AND  blocks.height >= ?
                ORDER  BY unshielded_utxos.id ASC
            "}
            }
            (Some(_), UnshieldedUtxoFilter::FromBlockHash(_)) => {
                indoc! {"
                SELECT unshielded_utxos.*
                FROM   unshielded_utxos
                JOIN   transactions  ON   transactions.id = unshielded_utxos.creating_transaction_id
                JOIN   blocks        ON   blocks.id = transactions.block_id
                WHERE  unshielded_utxos.owner_address = ?
                  AND  blocks.hash = ?
                ORDER  BY unshielded_utxos.id ASC
            "}
            }
            (Some(_), UnshieldedUtxoFilter::FromTxHash(_)) => {
                indoc! {"
                SELECT unshielded_utxos.*
                FROM   unshielded_utxos
                JOIN   transactions   ON  transactions.id = unshielded_utxos.creating_transaction_id
                WHERE  unshielded_utxos.owner_address = ?
                  AND  transactions.hash = ?
                ORDER  BY unshielded_utxos.id ASC
            "}
            }
            (Some(_), UnshieldedUtxoFilter::FromTxIdentifier(_)) => {
                indoc! {"
                SELECT unshielded_utxos.*
                FROM   unshielded_utxos
                JOIN   transaction_identifiers
                    ON transaction_identifiers.transaction_id = unshielded_utxos.creating_transaction_id
                WHERE  unshielded_utxos.owner_address = ?
                  AND  transaction_identifiers.identifier = ?
                ORDER  BY unshielded_utxos.id ASC
            "}
            }
            _ => {
                return Err(sqlx::Error::Protocol(
                    "Unsupported filter combination".into(),
                ));
            }
        };

        // Build query with the SQL
        let mut query = sqlx::query_as::<_, UnshieldedUtxo>(sql);

        // Add bindings based on the filter type
        match (&address, &filter) {
            (Some(addr), UnshieldedUtxoFilter::All) => {
                query = query.bind(addr.as_ref());
            }
            (None, UnshieldedUtxoFilter::CreatedByTx(tx_id)) => {
                query = query.bind(*tx_id as i64);
            }
            (None, UnshieldedUtxoFilter::SpentByTx(tx_id)) => {
                query = query.bind(*tx_id as i64);
            }
            (Some(addr), UnshieldedUtxoFilter::CreatedInTxForAddress(tx_id)) => {
                query = query.bind(*tx_id as i64);
                query = query.bind(addr.as_ref());
            }
            (Some(addr), UnshieldedUtxoFilter::SpentInTxForAddress(tx_id)) => {
                query = query.bind(*tx_id as i64);
                query = query.bind(addr.as_ref());
            }
            (Some(addr), UnshieldedUtxoFilter::FromHeight(height)) => {
                query = query.bind(addr.as_ref());
                query = query.bind(*height as i64);
            }
            (Some(addr), UnshieldedUtxoFilter::FromBlockHash(hash)) => {
                query = query.bind(addr.as_ref());
                query = query.bind(hash.as_ref());
            }
            (Some(addr), UnshieldedUtxoFilter::FromTxHash(hash)) => {
                query = query.bind(addr.as_ref());
                query = query.bind(hash.as_ref());
            }
            (Some(addr), UnshieldedUtxoFilter::FromTxIdentifier(identifier)) => {
                query = query.bind(addr.as_ref());
                query = query.bind(identifier);
            }
            _ => {}
        };

        // Execute query and get results
        let mut utxos = query.fetch_all(&mut *tx).await?;

        // Process results
        self.enrich_utxos_with_transaction_data(&mut utxos, &mut tx)
            .await?;

        Ok(utxos)
    }

    async fn get_transactions_involving_unshielded(
        &self,
        address: &UnshieldedAddress,
    ) -> Result<Vec<Transaction>, sqlx::Error> {
        let sql = indoc! {"
        SELECT DISTINCT
            transactions.id,
            transactions.hash,
            blocks.hash AS block_hash,
            transactions.protocol_version,
            transactions.transaction_result,
            transactions.raw,
            transactions.merkle_tree_root,
            transactions.start_index,
            transactions.end_index
        FROM transactions
        INNER JOIN blocks ON blocks.id = transactions.block_id
        INNER JOIN unshielded_utxos ON
            unshielded_utxos.creating_transaction_id = transactions.id OR
            unshielded_utxos.spending_transaction_id = transactions.id
        WHERE unshielded_utxos.owner_address = ?
        ORDER BY transactions.id DESC
    "};

        let mut tx = self.pool.begin().await?;
        let mut transactions = sqlx::query_as::<_, Transaction>(sql)
            .bind(address.as_ref())
            .fetch_all(&mut *tx)
            .await?;

        for transaction in transactions.iter_mut() {
            let identifiers = get_identifiers_by_transaction_id(transaction.id, &mut tx).await?;
            transaction.identifiers = identifiers;

            transaction.unshielded_created_outputs = self
                .get_unshielded_utxos_with_tx(
                    Some(address),
                    UnshieldedUtxoFilter::CreatedInTxForAddress(transaction.id),
                    &mut tx,
                )
                .await?;

            transaction.unshielded_spent_outputs = self
                .get_unshielded_utxos_with_tx(
                    Some(address),
                    UnshieldedUtxoFilter::SpentInTxForAddress(transaction.id),
                    &mut tx,
                )
                .await?;
        }

        Ok(transactions)
    }
}

impl SqliteStorage {
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

    async fn enrich_utxos_with_transaction_data(
        &self,
        utxos: &mut [UnshieldedUtxo],
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    ) -> Result<(), sqlx::Error> {
        for utxo in utxos {
            let sql = indoc! {"
                SELECT
                    transactions.id, transactions.hash, blocks.hash as block_hash,
                    transactions.protocol_version, transactions.transaction_result,
                    transactions.raw, transactions.merkle_tree_root,
                    transactions.start_index, transactions.end_index
                FROM transactions
                INNER JOIN blocks ON blocks.id = transactions.block_id
                WHERE transactions.id = ?
            "};

            let mut creating_tx = sqlx::query_as::<_, Transaction>(sql)
                .bind(utxo.creating_transaction_id as i64)
                .fetch_optional(&mut **tx)
                .await?;

            if let Some(t) = &mut creating_tx {
                t.identifiers = get_identifiers_by_transaction_id(t.id, tx).await?;
            }

            utxo.created_at_transaction = creating_tx;

            if let Some(spending_tx_id) = utxo.spending_transaction_id {
                let mut spending_tx = sqlx::query_as::<_, Transaction>(sql)
                    .bind(spending_tx_id as i64)
                    .fetch_optional(&mut **tx)
                    .await?;

                if let Some(transaction) = &mut spending_tx {
                    transaction.identifiers =
                        get_identifiers_by_transaction_id(transaction.id, tx).await?;
                }

                utxo.spent_at_transaction = spending_tx;
            }
        }

        Ok(())
    }

    async fn get_unshielded_utxos_with_tx(
        &self,
        address: Option<&UnshieldedAddress>,
        filter: UnshieldedUtxoFilter<'_>,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let sql = match (&address, &filter) {
            (Some(_), UnshieldedUtxoFilter::All) => {
                indoc! {"
                    SELECT
                        id, owner_address, token_type, value, output_index, intent_hash,
                        creating_transaction_id, spending_transaction_id
                    FROM unshielded_utxos
                    WHERE owner_address = ?
                    ORDER BY id ASC
                "}
            }
            (None, UnshieldedUtxoFilter::CreatedByTx(_)) => {
                indoc! {"
                    SELECT *
                    FROM unshielded_utxos
                    WHERE creating_transaction_id = ?
                "}
            }
            (None, UnshieldedUtxoFilter::SpentByTx(_)) => {
                indoc! {"
                    SELECT *
                    FROM unshielded_utxos
                    WHERE spending_transaction_id = ?
                "}
            }
            (Some(_), UnshieldedUtxoFilter::CreatedInTxForAddress(_)) => {
                indoc! {"
                    SELECT *
                    FROM unshielded_utxos
                    WHERE creating_transaction_id = ?
                    AND owner_address = ?
                "}
            }
            (Some(_), UnshieldedUtxoFilter::SpentInTxForAddress(_)) => {
                indoc! {"
                    SELECT *
                    FROM unshielded_utxos
                    WHERE spending_transaction_id = ?
                    AND owner_address = ?
                "}
            }
            _ => {
                return Err(sqlx::Error::Protocol(
                    "Unsupported filter combination in get_unshielded_utxos_with_tx".into(),
                ));
            }
        };

        let mut query = sqlx::query_as::<_, UnshieldedUtxo>(sql);

        match (&address, &filter) {
            (Some(addr), UnshieldedUtxoFilter::All) => {
                query = query.bind(addr.as_ref());
            }
            (None, UnshieldedUtxoFilter::CreatedByTx(tx_id)) => {
                query = query.bind(*tx_id as i64);
            }
            (None, UnshieldedUtxoFilter::SpentByTx(tx_id)) => {
                query = query.bind(*tx_id as i64);
            }
            (Some(addr), UnshieldedUtxoFilter::CreatedInTxForAddress(tx_id)) => {
                query = query.bind(*tx_id as i64);
                query = query.bind(addr.as_ref());
            }
            (Some(addr), UnshieldedUtxoFilter::SpentInTxForAddress(tx_id)) => {
                query = query.bind(*tx_id as i64);
                query = query.bind(addr.as_ref());
            }
            _ => {}
        };

        let mut utxos = query.fetch_all(&mut **tx).await?;

        self.enrich_utxos_with_transaction_data(&mut utxos, tx)
            .await?;

        Ok(utxos)
    }
}

async fn get_identifiers_by_transaction_id(
    transaction_id: u64,
    tx: &mut sqlx::Transaction<'_, Sqlite>,
) -> Result<Vec<Identifier>, sqlx::Error> {
    let sql = indoc! {"
        SELECT identifier
        FROM transaction_identifiers
        WHERE transaction_id = $1
    "};

    let identifiers = sqlx::query(sql)
        .bind(transaction_id as i64)
        .try_map(|row: <Sqlite as Database>::Row| Ok(row.try_get::<Vec<u8>, _>(0)?.into()))
        .fetch(&mut **tx)
        .try_collect::<Vec<_>>()
        .await?;

    Ok(identifiers)
}
