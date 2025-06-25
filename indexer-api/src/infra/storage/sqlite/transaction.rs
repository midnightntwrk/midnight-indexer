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
    domain::{Transaction, storage::transaction::TransactionStorage},
    infra::storage::sqlite::SqliteStorage,
};
use async_stream::try_stream;
use futures::Stream;
use indexer_common::{
    domain::{RawTransactionIdentifier, RawUnshieldedAddress, SessionId, TransactionHash},
    stream::flatten_chunks,
};
use indoc::indoc;
use std::num::NonZeroU32;

impl TransactionStorage for SqliteStorage {
    async fn get_transaction_by_id(&self, id: u64) -> Result<Option<Transaction>, sqlx::Error> {
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
                transactions.end_index,
                transactions.paid_fees,
                transactions.estimated_fees
            FROM transactions
            INNER JOIN blocks ON blocks.id = transactions.block_id
            WHERE transactions.id = $1
        "};

        let mut transaction = sqlx::query_as::<_, Transaction>(query)
            .bind(id as i64)
            .fetch_optional(&*self.pool)
            .await?;

        if let Some(transaction) = &mut transaction {
            transaction.identifiers = self
                .get_identifiers_by_transaction_id(transaction.id)
                .await?;
        }

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
                transactions.end_index,
                transactions.paid_fees,
                transactions.estimated_fees
            FROM transactions
            INNER JOIN blocks ON blocks.id = transactions.block_id
            WHERE transactions.block_id = $1
            ORDER BY transactions.id
        "};

        let mut transactions = sqlx::query_as::<_, Transaction>(query)
            .bind(id as i64)
            .fetch_all(&*self.pool)
            .await?;

        let transaction_ids: Vec<u64> = transactions.iter().map(|t| t.id).collect();
        let identifiers_map = self
            .get_identifiers_for_transactions(&transaction_ids)
            .await?;

        for transaction in transactions.iter_mut() {
            transaction.identifiers = identifiers_map
                .get(&transaction.id)
                .cloned()
                .unwrap_or_default();
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
                transactions.end_index,
                transactions.paid_fees,
                transactions.estimated_fees
            FROM transactions
            INNER JOIN blocks ON blocks.id = transactions.block_id
            WHERE transactions.hash = $1
            ORDER BY transactions.id DESC
        "};

        let mut transactions = sqlx::query_as::<_, Transaction>(query)
            .bind(hash.as_ref())
            .fetch_all(&*self.pool)
            .await?;

        let transaction_ids: Vec<u64> = transactions.iter().map(|t| t.id).collect();
        let identifiers_map = self
            .get_identifiers_for_transactions(&transaction_ids)
            .await?;

        for transaction in transactions.iter_mut() {
            transaction.identifiers = identifiers_map
                .get(&transaction.id)
                .cloned()
                .unwrap_or_default();
        }

        Ok(transactions)
    }

    async fn get_transactions_by_identifier(
        &self,
        identifier: &RawTransactionIdentifier,
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
                transactions.end_index,
                transactions.paid_fees,
                transactions.estimated_fees
            FROM transactions
            INNER JOIN blocks ON blocks.id = transactions.block_id
            INNER JOIN transaction_identifiers ON transactions.id = transaction_identifiers.transaction_id
            WHERE transaction_identifiers.identifier = $1
            ORDER BY transactions.id
        "};

        let mut transactions = sqlx::query_as::<_, Transaction>(query)
            .bind(identifier)
            .fetch_all(&*self.pool)
            .await?;

        let transaction_ids: Vec<u64> = transactions.iter().map(|t| t.id).collect();
        let identifiers_map = self
            .get_identifiers_for_transactions(&transaction_ids)
            .await?;

        for transaction in transactions.iter_mut() {
            transaction.identifiers = identifiers_map
                .get(&transaction.id)
                .cloned()
                .unwrap_or_default();
        }

        Ok(transactions)
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
                        transactions.end_index,
                        transactions.paid_fees,
                        transactions.estimated_fees
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

                let transaction_ids: Vec<u64> = transactions.iter().map(|t| t.id).collect();
                let identifiers_map = self.get_identifiers_for_transactions(&transaction_ids).await?;

                for transaction in transactions.iter_mut() {
                    transaction.identifiers = identifiers_map
                        .get(&transaction.id)
                        .cloned()
                        .unwrap_or_default();
                }

                yield transactions;
            }
        };

        flatten_chunks(chunks)
    }

    async fn get_transactions_involving_unshielded(
        &self,
        address: &RawUnshieldedAddress,
        from_transaction_id: u64,
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
            transactions.end_index,
            transactions.paid_fees,
            transactions.estimated_fees
        FROM transactions
        INNER JOIN blocks ON blocks.id = transactions.block_id
        INNER JOIN unshielded_utxos ON
            unshielded_utxos.creating_transaction_id = transactions.id OR
            unshielded_utxos.spending_transaction_id = transactions.id
        WHERE unshielded_utxos.owner_address = $1
        AND transactions.id >= $2
        ORDER BY transactions.id
    "};

        let mut transactions = sqlx::query_as::<_, Transaction>(sql)
            .bind(address.as_ref())
            .bind(from_transaction_id as i64)
            .fetch_all(&*self.pool)
            .await?;

        let transaction_ids: Vec<u64> = transactions.iter().map(|t| t.id).collect();
        let identifiers_map = self
            .get_identifiers_for_transactions(&transaction_ids)
            .await?;

        for transaction in transactions.iter_mut() {
            transaction.identifiers = identifiers_map
                .get(&transaction.id)
                .cloned()
                .unwrap_or_default();
        }

        Ok(transactions)
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
                SELECT end_index
                FROM transactions
                WHERE id = (
                    SELECT MAX(last_indexed_transaction_id)
                    FROM wallets
                )
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
}
