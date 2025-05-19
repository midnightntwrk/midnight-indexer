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
    domain::{self, Block, BlockHash, Storage, Transaction, TransactionHash, UnshieldedUtxo},
    infra::storage::ContractAction,
};
use async_stream::try_stream;
use chacha20poly1305::ChaCha20Poly1305;
use derive_more::Debug;
use futures::{stream::TryStreamExt, Stream};
use indexer_common::{
    domain::{ContractAddress, Identifier, NetworkId, SessionId, UnshieldedAddress, ViewingKey},
    flatten_chunks,
    infra::pool::sqlite::SqlitePool,
};
use indoc::indoc;
use sqlx::{
    query::QueryAs,
    types::{time::OffsetDateTime, Uuid},
    Database, Row, Sqlite,
};
use std::num::NonZeroU32;

/// Sqlite based implementation of [Storage].
#[derive(Debug, Clone)]
pub struct SqliteStorage {
    #[debug(skip)]
    cipher: ChaCha20Poly1305,
    pool: SqlitePool,
    network_id: NetworkId,
}

impl SqliteStorage {
    /// Create a new [SqliteStorage].
    pub fn new(cipher: ChaCha20Poly1305, pool: SqlitePool, network_id: NetworkId) -> Self {
        Self {
            cipher,
            pool,
            network_id,
        }
    }
}

impl Storage for SqliteStorage {
    async fn get_latest_block(&self) -> Result<Option<Block>, sqlx::Error> {
        let sql = indoc! {"
            SELECT *
            FROM blocks
            ORDER BY height DESC
            LIMIT 1
        "};

        let query = sqlx::query_as::<_, Block>(sql);

        get_block(query, &self.pool).await
    }

    async fn get_block_by_hash(&self, hash: &BlockHash) -> Result<Option<Block>, sqlx::Error> {
        let sql = indoc! {"
            SELECT *
            FROM blocks
            WHERE hash = $1
            LIMIT 1
        "};

        let query = sqlx::query_as::<_, Block>(sql).bind(hash.as_ref());

        get_block(query, &self.pool).await
    }

    async fn get_block_by_height(&self, height: u32) -> Result<Option<Block>, sqlx::Error> {
        let sql = indoc! {"
            SELECT *
            FROM blocks
            WHERE height = $1
            LIMIT 1
        "};

        let query = sqlx::query_as::<_, Block>(sql).bind(height as i64);

        get_block(query, &self.pool).await
    }

    fn get_blocks(
        &self,
        mut from_height: u32,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<Block, sqlx::Error>> {
        let chunks = try_stream! {
            loop {
                let mut tx = self.pool.begin().await?;

                let sql = indoc! {"
                    SELECT *
                    FROM blocks
                    WHERE height >= $1
                    ORDER BY id
                    LIMIT $2
                "};

                let mut blocks = sqlx::query_as::<_, Block>(sql)
                    .bind(from_height as i64)
                    .bind(batch_size.get() as i64)
                    .fetch_all(&mut *tx)
                    .await?;
                if blocks.is_empty() {
                    break;
                } else {
                    from_height += blocks.len() as u32;
                }

                for block in blocks.iter_mut() {
                    let transactions = get_transactions_by_block_hash(&block.hash, &mut tx).await?;
                    block.transactions = transactions;
                }

                yield blocks;
            }
        };

        flatten_chunks(chunks)
    }

    async fn get_transaction_by_db_id(
        &self,
        tx_db_id: u64,
    ) -> Result<Option<Transaction>, sqlx::Error> {
        let sql = indoc! {"
            SELECT
                transactions.id, transactions.hash, blocks.hash AS block_hash, transactions.protocol_version, transactions.apply_stage,
                transactions.raw, transactions.merkle_tree_root, transactions.start_index, transactions.end_index
            FROM transactions
            INNER JOIN blocks ON blocks.id = transactions.block_id
            WHERE transactions.id = ?
        "};
        let mut tx = self.pool.begin().await?;
        let mut transaction_option = sqlx::query_as::<_, Transaction>(sql)
            .bind(tx_db_id as i64)
            .fetch_optional(&mut *tx)
            .await?;

        if let Some(transaction) = &mut transaction_option {
            transaction.identifiers =
                get_identifiers_by_transaction_id(transaction.id, &mut tx).await?;
            transaction.contract_actions =
                get_contract_actions_by_transaction_id(transaction.id, &mut tx).await?;

            transaction.unshielded_created_outputs = self
                .get_unshielded_utxos_by_creating_tx_id(transaction.id, &mut tx)
                .await?;
            transaction.unshielded_spent_outputs = self
                .get_unshielded_utxos_by_spending_tx_id(transaction.id, &mut tx)
                .await?;
        }
        Ok(transaction_option)
    }

    async fn get_transactions_by_hash(
        &self,
        hash: &TransactionHash,
    ) -> Result<Vec<Transaction>, sqlx::Error> {
        let sql = indoc! {"
            SELECT
                transactions.id,
                transactions.hash,
                blocks.hash AS block_hash,
                transactions.protocol_version,
                transactions.apply_stage,
                transactions.raw,
                transactions.merkle_tree_root,
                transactions.start_index,
                transactions.end_index
            FROM transactions
            INNER JOIN blocks ON blocks.id = transactions.block_id
            WHERE transactions.hash = $1
            ORDER BY transactions.id DESC
        "};

        let mut tx = self.pool.begin().await?;

        let mut transactions = sqlx::query_as::<_, Transaction>(sql)
            .bind(hash.as_ref())
            .fetch_all(&mut *tx)
            .await?;

        for transaction in transactions.iter_mut() {
            let identifiers = get_identifiers_by_transaction_id(transaction.id, &mut tx).await?;
            transaction.identifiers = identifiers;
            let actions = get_contract_actions_by_transaction_id(transaction.id, &mut tx).await?;
            transaction.contract_actions = actions;

            transaction.unshielded_created_outputs = self
                .get_unshielded_utxos_by_creating_tx_id(transaction.id, &mut tx)
                .await?;
            transaction.unshielded_spent_outputs = self
                .get_unshielded_utxos_by_spending_tx_id(transaction.id, &mut tx)
                .await?;
        }

        Ok(transactions)
    }

    async fn get_transaction_by_identifier(
        &self,
        identifier: &Identifier,
    ) -> Result<Option<Transaction>, sqlx::Error> {
        let sql = indoc! {"
            SELECT
                transactions.id,
                transactions.hash,
                blocks.hash AS block_hash,
                transactions.protocol_version,
                transactions.apply_stage,
                transactions.raw,
                transactions.merkle_tree_root,
                transactions.start_index,
                transactions.end_index
            FROM transactions
            INNER JOIN blocks ON blocks.id = transactions.block_id
            INNER JOIN transaction_identifiers ON transactions.id = transaction_identifiers.transaction_id
            WHERE transaction_identifiers.identifier = $1
            LIMIT 1
        "};

        let query = sqlx::query_as::<_, Transaction>(sql).bind(identifier);

        let mut transaction_option = get_transaction(query, &self.pool).await?;

        // Fetch unshielded UTXOs for the transaction if it exists
        if let Some(transaction) = &mut transaction_option {
            let mut tx = self.pool.begin().await?;
            transaction.unshielded_created_outputs = self
                .get_unshielded_utxos_by_creating_tx_id(transaction.id, &mut tx)
                .await?;
            transaction.unshielded_spent_outputs = self
                .get_unshielded_utxos_by_spending_tx_id(transaction.id, &mut tx)
                .await?;
        }

        Ok(transaction_option)
    }

    async fn get_latest_contract_action_by_address(
        &self,
        address: &ContractAddress,
    ) -> Result<Option<domain::ContractAction>, sqlx::Error> {
        let sql = indoc! {"
            SELECT
                contract_actions.id AS id,
                contract_actions.address,
                contract_actions.state,
                contract_actions.attributes,
                contract_actions.zswap_state
            FROM contract_actions
            WHERE contract_actions.address = $1
            ORDER BY id DESC
            LIMIT 1
        "};

        sqlx::query_as::<_, ContractAction>(sql)
            .bind(address)
            .fetch_optional(&*self.pool)
            .await
            .map(|action| action.map(|action| action.into()))
    }

    async fn get_contract_action_by_address_and_block_hash(
        &self,
        address: &ContractAddress,
        hash: &BlockHash,
    ) -> Result<Option<domain::ContractAction>, sqlx::Error> {
        let sql = indoc! {"
            SELECT
                contract_actions.id AS id,
                contract_actions.address,
                contract_actions.state,
                contract_actions.attributes,
                contract_actions.zswap_state
            FROM contract_actions
            INNER JOIN transactions ON transactions.id = contract_actions.transaction_id
            WHERE contract_actions.address = $1
            AND transactions.block_id = (SELECT id FROM blocks WHERE hash = $2)
            ORDER BY id DESC
            LIMIT 1
        "};

        sqlx::query_as::<_, ContractAction>(sql)
            .bind(address.as_ref())
            .bind(hash.as_ref())
            .fetch_optional(&*self.pool)
            .await
            .map(|action| action.map(|action| action.into()))
    }

    async fn get_contract_action_by_address_and_block_height(
        &self,
        address: &ContractAddress,
        height: u32,
    ) -> Result<Option<domain::ContractAction>, sqlx::Error> {
        let sql = indoc! {"
            SELECT
                contract_actions.id AS id,
                contract_actions.address,
                contract_actions.state,
                contract_actions.attributes,
                contract_actions.zswap_state
            FROM contract_actions
            INNER JOIN transactions ON transactions.id = contract_actions.transaction_id
            INNER JOIN blocks ON blocks.id = transactions.block_id
            WHERE contract_actions.address = $1
            AND blocks.height = $2
            ORDER BY id DESC
            LIMIT 1
        "};

        sqlx::query_as::<_, ContractAction>(sql)
            .bind(address)
            .bind(height as i64)
            .fetch_optional(&*self.pool)
            .await
            .map(|action| action.map(|action| action.into()))
    }

    async fn get_contract_action_by_address_and_transaction_hash(
        &self,
        address: &ContractAddress,
        hash: &TransactionHash,
    ) -> Result<Option<domain::ContractAction>, sqlx::Error> {
        let sql = indoc! {"
            SELECT
                contract_actions.id AS id,
                contract_actions.address,
                contract_actions.state,
                contract_actions.attributes,
                contract_actions.zswap_state
            FROM contract_actions
            WHERE contract_actions.address = $1
            AND contract_actions.transaction_id = (
                SELECT id FROM transactions
                WHERE hash = $2
                AND apply_stage = 'Success'
                ORDER BY id DESC
                LIMIT 1
            )
            ORDER BY id DESC
            LIMIT 1
        "};

        sqlx::query_as::<_, ContractAction>(sql)
            .bind(address.as_ref())
            .bind(hash.as_ref())
            .fetch_optional(&*self.pool)
            .await
            .map(|action| action.map(|action| action.into()))
    }

    async fn get_contract_action_by_address_and_transaction_identifier(
        &self,
        address: &ContractAddress,
        identifier: &Identifier,
    ) -> Result<Option<domain::ContractAction>, sqlx::Error> {
        let sql = indoc! {"
            SELECT
                contract_actions.id AS id,
                contract_actions.address,
                contract_actions.state,
                contract_actions.attributes,
                contract_actions.zswap_state
            FROM contract_actions
            INNER JOIN transactions ON transactions.id = contract_actions.transaction_id
            INNER JOIN transaction_identifiers ON transactions.id = transaction_identifiers.transaction_id
            WHERE contract_actions.address = $1
            AND transaction_identifiers.identifier = $2
            ORDER BY id DESC
            LIMIT 1
        "};

        sqlx::query_as::<_, ContractAction>(sql)
            .bind(address)
            .bind(identifier)
            .fetch_optional(&*self.pool)
            .await
            .map(|action| action.map(|action| action.into()))
    }

    fn get_contract_actions_by_address(
        &self,
        address: &ContractAddress,
        from_block_height: u32,
        mut from_contract_action_id: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<domain::ContractAction, sqlx::Error>> + Send {
        let chunks = try_stream! {
            loop {
                let sql = indoc! {"
                    SELECT
                        contract_actions.id AS id,
                        contract_actions.address,
                        contract_actions.state,
                        contract_actions.attributes,
                        contract_actions.zswap_state
                    FROM contract_actions
                    INNER JOIN transactions ON transactions.id = contract_actions.transaction_id
                    INNER JOIN blocks ON blocks.id = transactions.block_id
                    WHERE transactions.apply_stage = 'Success'
                    AND contract_actions.address = $1
                    AND blocks.height >= $2
                    AND contract_actions.id >= $3
                    ORDER BY id ASC
                    LIMIT $4
                "};

                let actions = sqlx::query_as::<_, ContractAction>(sql)
                    .bind(address)
                    .bind(from_block_height as i64)
                    .bind(from_contract_action_id as i64)
                    .bind(batch_size.get() as i64)
                    .fetch(&*self.pool)
                    .map_ok(domain::ContractAction::from)
                    .try_collect::<Vec<_>>()
                    .await?;

                let max_id = actions.iter().map(|action| action.id).max();
                match max_id {
                    Some(max_id) => from_contract_action_id = max_id + 1,
                    None => break,
                }

                yield actions;
            }
        };

        flatten_chunks(chunks)
    }

    async fn get_last_end_index_for_wallet(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<u64>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        let query = indoc! {"
            SELECT end_index
            FROM transactions
            INNER JOIN wallets ON transactions.id = wallets.last_indexed_transaction_id
            WHERE wallets.session_id = $1
        "};

        let index = sqlx::query_scalar::<_, i64>(query)
            .bind(session_id.as_ref())
            .fetch_optional(&mut *tx)
            .await?
            .map(|n| n as u64);

        Ok(index)
    }

    async fn get_last_relevant_end_index_for_wallet(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<u64>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        let query = indoc! {"
            SELECT end_index
            FROM transactions
            INNER JOIN relevant_transactions ON transactions.id = relevant_transactions.transaction_id
            INNER JOIN wallets ON wallets.id = relevant_transactions.wallet_id
            WHERE wallets.session_id = $1
            ORDER BY end_index DESC
            LIMIT 1
        "};

        let index = sqlx::query_scalar::<_, i64>(query)
            .bind(session_id.as_ref())
            .fetch_optional(&mut *tx)
            .await?
            .map(|n| n as u64);

        Ok(index)
    }

    fn get_relevant_transactions(
        &self,
        session_id: &SessionId,
        mut from_index: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<Transaction, sqlx::Error>> + Send {
        let chunks = try_stream! {
            loop {
                let mut tx = self.pool.begin().await?;

                let query = indoc! {"
                    SELECT
                        transactions.id,
                        transactions.hash,
                        blocks.hash AS block_hash,
                        transactions.protocol_version,
                        transactions.apply_stage,
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
                    .bind(from_index as i64)
                    .bind(batch_size.get() as i64)
                    .fetch_all(&mut *tx)
                    .await?;

                from_index = match transactions.iter().map(|tx| tx.end_index).max() {
                    Some(end_index) => end_index + 1,
                    None => break,
                };

                for transaction in transactions.iter_mut() {
                    let identifiers =
                        get_identifiers_by_transaction_id(transaction.id, &mut tx).await?;
                    transaction.identifiers = identifiers;
                    let actions =
                        get_contract_actions_by_transaction_id(transaction.id, &mut tx).await?;
                    transaction.contract_actions = actions;

                    transaction.unshielded_created_outputs =
                        self.get_unshielded_utxos_by_creating_tx_id(transaction.id, &mut tx).await?;
                    transaction.unshielded_spent_outputs =
                        self.get_unshielded_utxos_by_spending_tx_id(transaction.id, &mut tx).await?;
                }

                yield transactions;
            }
        };

        flatten_chunks(chunks)
    }

    async fn connect_wallet(&self, viewing_key: &ViewingKey) -> Result<(), sqlx::Error> {
        let id = Uuid::now_v7();
        let session_id = viewing_key.as_session_id();
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

    async fn disconnect_wallet(&self, session_id: &SessionId) -> Result<(), sqlx::Error> {
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

    async fn set_wallet_active(&self, session_id: &SessionId) -> Result<(), sqlx::Error> {
        let query = indoc! {"
            UPDATE wallets
            SET active = TRUE, last_active = $1
            WHERE session_id = $2
        "};

        sqlx::query(query)
            .bind(OffsetDateTime::now_utc())
            .bind(session_id.as_ref())
            .execute(&*self.pool)
            .await?;

        Ok(())
    }

    async fn get_unshielded_utxos_by_address(
        &self,
        address: &UnshieldedAddress,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        let sql = indoc! {"
                SELECT
                    id, owner_address, token_type, value, output_index, intent_hash,
                    creating_transaction_id as creating_tx_id,
                    spending_transaction_id as spending_tx_id
                FROM unshielded_utxos
                WHERE owner_address = $1
                ORDER BY id ASC
            "};

        let mut utxos = sqlx::query_as::<_, UnshieldedUtxo>(sql)
            .bind(address.as_ref())
            .fetch_all(&mut *tx)
            .await?;

        self.enrich_utxos_with_transaction_data(&mut utxos, &mut tx)
            .await?;

        Ok(utxos)
    }

    async fn get_unshielded_utxos_by_address_created_in_tx(
        &self,
        tx_id: u64,
        address: &UnshieldedAddress,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        let sql = indoc! {"
        SELECT *
        FROM unshielded_utxos
        WHERE creating_transaction_id = $1
        AND owner_address = $2
        "};

        let utxos = sqlx::query_as::<_, UnshieldedUtxo>(sql)
            .bind(tx_id as i64)
            .bind(address.as_ref())
            .fetch_all(&mut *tx)
            .await?
            .into_iter()
            .map(|utxo| self.with_network_id(utxo))
            .collect();

        Ok(utxos)
    }

    async fn get_unshielded_utxos_by_address_spent_in_tx(
        &self,
        tx_id: u64,
        address: &UnshieldedAddress,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        let sql = indoc! {"
        SELECT *
        FROM unshielded_utxos
        WHERE spending_transaction_id = $1
        AND owner_address = $2
        "};

        let utxos = sqlx::query_as::<_, UnshieldedUtxo>(sql)
            .bind(tx_id as i64)
            .bind(address.as_ref())
            .fetch_all(&mut *tx)
            .await?
            .into_iter()
            .map(|utxo| self.with_network_id(utxo))
            .collect();

        Ok(utxos)
    }

    async fn get_unshielded_utxos_by_address_from_height(
        &self,
        address: &UnshieldedAddress,
        start_height: u32,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let sql = indoc! {"
         SELECT unshielded_utxos.*
         FROM   unshielded_utxos
         JOIN   transactions   ON  transactions.id = unshielded_utxos.creating_transaction_id
         JOIN   blocks         ON  blocks.id = transactions.block_id
         WHERE  unshielded_utxos.owner_address = $1
           AND  blocks.height >= $2
         ORDER  BY unshielded_utxos.id ASC
         "};

        self.query_utxos_with_height(sql, address, start_height)
            .await
    }

    async fn get_unshielded_utxos_by_address_from_block_hash(
        &self,
        address: &UnshieldedAddress,
        block_hash: &BlockHash,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let sql = indoc! {"
          SELECT unshielded_utxos.*
          FROM   unshielded_utxos
          JOIN   transactions  ON   transactions.id = unshielded_utxos.creating_transaction_id
          JOIN   blocks        ON   blocks.id = transactions.block_id
          WHERE  unshielded_utxos.owner_address = $1
            AND  blocks.hash = $2
          ORDER  BY unshielded_utxos.id ASC
          "};

        self.query_utxos_with_block_hash(sql, address, block_hash)
            .await
    }

    async fn get_unshielded_utxos_by_address_from_tx_hash(
        &self,
        address: &UnshieldedAddress,
        tx_hash: &TransactionHash,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let sql = indoc! {"
        SELECT unshielded_utxos.*
        FROM   unshielded_utxos
        JOIN   transactions   ON  transactions.id = unshielded_utxos.creating_transaction_id
        WHERE  unshielded_utxos.owner_address = $1
          AND  transactions.hash = $2
        ORDER  BY unshielded_utxos.id ASC
        "};

        self.query_utxos_with_tx_hash(sql, address, tx_hash).await
    }

    async fn get_unshielded_utxos_by_address_from_tx_identifier(
        &self,
        address: &UnshieldedAddress,
        identifier: &Identifier,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let sql = indoc! {"
        SELECT unshielded_utxos.*
        FROM   unshielded_utxos
        JOIN   transaction_identifiers ON transaction_identifiers.transaction_id = unshielded_utxos.creating_transaction_id
        WHERE  unshielded_utxos.owner_address = $1
          AND  transaction_identifiers.identifier = $2
        ORDER  BY unshielded_utxos.id ASC
        "};

        self.query_utxos_with_identifier(sql, address, identifier)
            .await
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
                transactions.apply_stage,
                transactions.raw,
                transactions.merkle_tree_root,
                transactions.start_index,
                transactions.end_index
            FROM transactions
            INNER JOIN blocks ON blocks.id = transactions.block_id
            INNER JOIN unshielded_utxos ON unshielded_utxos.creating_transaction_id = transactions.id OR
                unshielded_utxos.spending_transaction_id = transactions.id
            WHERE unshielded_utxos.owner_address = $1
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

            let actions = get_contract_actions_by_transaction_id(transaction.id, &mut tx).await?;
            transaction.contract_actions = actions;

            transaction.unshielded_created_outputs = self
                .get_unshielded_utxos_by_address_created_in_tx_with_tx(
                    transaction.id,
                    address,
                    &mut tx,
                )
                .await?;

            transaction.unshielded_spent_outputs = self
                .get_unshielded_utxos_by_address_spent_in_tx_with_tx(
                    transaction.id,
                    address,
                    &mut tx,
                )
                .await?;
        }

        Ok(transactions)
    }
}

impl SqliteStorage {
    async fn get_unshielded_utxos_by_creating_tx_id(
        &self,
        tx_id: u64,
        db_tx: &mut sqlx::Transaction<'_, Sqlite>,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let sql = indoc! {"
            SELECT *
            FROM unshielded_utxos
            WHERE creating_transaction_id = $1
        "};

        let utxos = sqlx::query_as::<_, UnshieldedUtxo>(sql)
            .bind(tx_id as i64)
            .fetch_all(&mut **db_tx)
            .await?
            .into_iter()
            .map(|utxo| self.with_network_id(utxo))
            .collect();

        Ok(utxos)
    }

    async fn get_unshielded_utxos_by_spending_tx_id(
        &self,
        tx_id: u64,
        db_tx: &mut sqlx::Transaction<'_, Sqlite>,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let sql = indoc! {"
            SELECT *
            FROM unshielded_utxos
            WHERE spending_transaction_id = $1
        "};

        let utxos = sqlx::query_as::<_, UnshieldedUtxo>(sql)
            .bind(tx_id as i64)
            .fetch_all(&mut **db_tx)
            .await?
            .into_iter()
            .map(|utxo| self.with_network_id(utxo))
            .collect();

        Ok(utxos)
    }

    async fn get_unshielded_utxos_by_address_created_in_tx_with_tx(
        &self,
        tx_id: u64,
        address: &UnshieldedAddress,
        db_tx: &mut sqlx::Transaction<'_, Sqlite>,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let sql = indoc! {"
            SELECT *
            FROM unshielded_utxos
            WHERE creating_transaction_id = $1
            AND owner_address = $2
        "};

        let utxos = sqlx::query_as::<_, UnshieldedUtxo>(sql)
            .bind(tx_id as i64)
            .bind(address.as_ref())
            .fetch_all(&mut **db_tx)
            .await?
            .into_iter()
            .map(|utxo| self.with_network_id(utxo))
            .collect();

        Ok(utxos)
    }

    async fn get_unshielded_utxos_by_address_spent_in_tx_with_tx(
        &self,
        tx_id: u64,
        address: &UnshieldedAddress,
        db_tx: &mut sqlx::Transaction<'_, Sqlite>,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let sql = indoc! {"
            SELECT *
            FROM unshielded_utxos
            WHERE spending_transaction_id = $1
            AND owner_address = $2
        "};

        let utxos = sqlx::query_as::<_, UnshieldedUtxo>(sql)
            .bind(tx_id as i64)
            .bind(address.as_ref())
            .fetch_all(&mut **db_tx)
            .await?
            .into_iter()
            .map(|utxo| self.with_network_id(utxo))
            .collect();

        Ok(utxos)
    }

    async fn query_utxos_with_height(
        &self,
        sql: &str,
        address: &UnshieldedAddress,
        height: u32,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        let mut utxos = sqlx::query_as::<_, UnshieldedUtxo>(sql)
            .bind(address.as_ref())
            .bind(height as i64)
            .fetch_all(&mut *tx)
            .await?;

        self.enrich_utxos_with_transaction_data(&mut utxos, &mut tx)
            .await?;
        Ok(utxos)
    }

    async fn query_utxos_with_block_hash(
        &self,
        sql: &str,
        address: &UnshieldedAddress,
        hash: &BlockHash,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        let mut utxos = sqlx::query_as::<_, UnshieldedUtxo>(sql)
            .bind(address.as_ref())
            .bind(hash.as_ref())
            .fetch_all(&mut *tx)
            .await?;

        self.enrich_utxos_with_transaction_data(&mut utxos, &mut tx)
            .await?;
        Ok(utxos)
    }

    async fn query_utxos_with_tx_hash(
        &self,
        sql: &str,
        address: &UnshieldedAddress,
        hash: &TransactionHash,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        let mut utxos = sqlx::query_as::<_, UnshieldedUtxo>(sql)
            .bind(address.as_ref())
            .bind(hash.as_ref())
            .fetch_all(&mut *tx)
            .await?;

        self.enrich_utxos_with_transaction_data(&mut utxos, &mut tx)
            .await?;
        Ok(utxos)
    }

    async fn query_utxos_with_identifier(
        &self,
        sql: &str,
        address: &UnshieldedAddress,
        identifier: &Identifier,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        let mut utxos = sqlx::query_as::<_, UnshieldedUtxo>(sql)
            .bind(address.as_ref())
            .bind(identifier)
            .fetch_all(&mut *tx)
            .await?;

        self.enrich_utxos_with_transaction_data(&mut utxos, &mut tx)
            .await?;
        Ok(utxos)
    }

    /// Enriches basic UTXO records with their related transaction data and network context.
    ///
    /// For each UTXO in the provided slice:
    /// - Fetches and attaches the transaction that created this UTXO
    /// - If the UTXO has been spent, fetches and attaches the spending transaction
    /// - Sets the network ID for proper Bech32m address formatting
    ///
    /// # Arguments
    /// * `utxos` - Mutable slice of UTXOs to be enriched with transaction data
    /// * `tx` - SQLite transaction to use for queries
    ///
    /// # Returns
    /// * `Result<(), sqlx::Error>` - Success or database error
    // Example for the enrichment function:
    async fn enrich_utxos_with_transaction_data(
        &self,
        utxos: &mut [UnshieldedUtxo],
        tx: &mut sqlx::Transaction<'_, Sqlite>,
    ) -> Result<(), sqlx::Error> {
        for utxo in utxos {
            let sql = indoc! {"
            SELECT
                transactions.id, transactions.hash, blocks.hash AS block_hash, transactions.protocol_version, transactions.apply_stage,
                transactions.raw, transactions.merkle_tree_root, transactions.start_index, transactions.end_index
            FROM transactions
            INNER JOIN blocks ON blocks.id = transactions.block_id
            WHERE transactions.id = $1
        "};

            let mut creating_tx = sqlx::query_as::<_, Transaction>(sql)
                .bind(utxo.creating_transaction_id as i64)
                .fetch_optional(&mut **tx)
                .await?;

            if let Some(t) = &mut creating_tx {
                t.identifiers = get_identifiers_by_transaction_id(t.id, tx).await?;
                t.contract_actions = get_contract_actions_by_transaction_id(t.id, tx).await?;
            }

            utxo.created_at_transaction = creating_tx;

            // Similar fix for spent transaction
            if let Some(spending_tx_id) = utxo.spending_transaction_id {
                let mut spending_tx = sqlx::query_as::<_, Transaction>(sql)
                    .bind(spending_tx_id as i64)
                    .fetch_optional(&mut **tx)
                    .await?;

                if let Some(transaction) = &mut spending_tx {
                    transaction.identifiers =
                        get_identifiers_by_transaction_id(transaction.id, tx).await?;
                    transaction.contract_actions =
                        get_contract_actions_by_transaction_id(transaction.id, tx).await?;
                }

                utxo.spent_at_transaction = spending_tx;
            }

            utxo.network_id = Some(self.network_id);
        }

        Ok(())
    }

    fn with_network_id(&self, mut utxo: UnshieldedUtxo) -> UnshieldedUtxo {
        utxo.network_id = Some(self.network_id);
        utxo
    }
}

async fn get_block<'a>(
    query: QueryAs<'a, Sqlite, Block, <Sqlite as Database>::Arguments<'a>>,
    pool: &SqlitePool,
) -> Result<Option<Block>, sqlx::Error> {
    let mut tx = pool.begin().await?;

    let mut block = query.fetch_optional(&mut *tx).await?;

    if let Some(block) = &mut block {
        let transactions = get_transactions_by_block_hash(&block.hash, &mut tx).await?;
        block.transactions = transactions;
    }

    Ok(block)
}

async fn get_transactions_by_block_hash(
    block_hash: &BlockHash,
    tx: &mut sqlx::Transaction<'_, Sqlite>,
) -> Result<Vec<Transaction>, sqlx::Error> {
    let sql = indoc! {"
        SELECT
            transactions.id,
            transactions.hash,
            blocks.hash AS block_hash,
            transactions.protocol_version,
            transactions.apply_stage,
            transactions.raw,
            transactions.merkle_tree_root,
            transactions.start_index,
            transactions.end_index
        FROM transactions
        INNER JOIN blocks ON blocks.id = transactions.block_id
        WHERE blocks.hash = $1
    "};

    let mut transactions = sqlx::query_as::<_, Transaction>(sql)
        .bind(block_hash.as_ref())
        .fetch_all(&mut **tx)
        .await?;

    for transaction in transactions.iter_mut() {
        let identifiers = get_identifiers_by_transaction_id(transaction.id, tx).await?;
        transaction.identifiers = identifiers;
        let actions = get_contract_actions_by_transaction_id(transaction.id, tx).await?;
        transaction.contract_actions = actions;
    }

    Ok(transactions)
}

async fn get_transaction<'a>(
    query: QueryAs<'a, Sqlite, Transaction, <Sqlite as Database>::Arguments<'a>>,
    pool: &SqlitePool,
) -> Result<Option<Transaction>, sqlx::Error> {
    let mut tx = pool.begin().await?;

    let mut transaction = query.fetch_optional(&mut *tx).await?;

    if let Some(transaction) = &mut transaction {
        let identifiers = get_identifiers_by_transaction_id(transaction.id, &mut tx).await?;
        transaction.identifiers = identifiers;
        let actions = get_contract_actions_by_transaction_id(transaction.id, &mut tx).await?;
        transaction.contract_actions = actions;
    }

    Ok(transaction)
}

async fn get_contract_actions_by_transaction_id(
    transaction_id: u64,
    tx: &mut sqlx::Transaction<'_, Sqlite>,
) -> Result<Vec<domain::ContractAction>, sqlx::Error> {
    let sql = indoc! {"
        SELECT
            contract_actions.id AS id,
            contract_actions.address,
            contract_actions.state,
            contract_actions.attributes,
            contract_actions.zswap_state
        FROM contract_actions
        WHERE contract_actions.transaction_id = $1
        ORDER BY id ASC
    "};

    sqlx::query_as::<_, ContractAction>(sql)
        .bind(transaction_id as i64)
        .fetch(&mut **tx)
        .map_ok(domain::ContractAction::from)
        .try_collect::<Vec<_>>()
        .await
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
