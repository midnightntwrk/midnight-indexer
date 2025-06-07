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
    domain::{
        Transaction, UnshieldedUtxo,
        storage::unshielded_utxo::{UnshieldedUtxoFilter, UnshieldedUtxoStorage},
    },
    infra::storage::sqlite::SqliteStorage,
};
use indexer_common::domain::UnshieldedAddress;
use indoc::indoc;
use std::collections::HashMap;

impl UnshieldedUtxoStorage for SqliteStorage {
    async fn get_unshielded_utxos(
        &self,
        address: Option<&UnshieldedAddress>,
        filter: UnshieldedUtxoFilter<'_>,
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

        let mut utxos = query.fetch_all(&*self.pool).await?;
        self.enrich_utxos_with_transaction_data_optimized(&mut utxos)
            .await?;

        Ok(utxos)
    }

    async fn get_highest_end_index_for_address(
        &self,
        address: &UnshieldedAddress,
    ) -> Result<Option<u64>, sqlx::Error> {
        let query = indoc! {"
            SELECT MAX(transactions.end_index)
            FROM transactions
            INNER JOIN unshielded_utxos ON 
                unshielded_utxos.creating_transaction_id = transactions.id OR
                unshielded_utxos.spending_transaction_id = transactions.id
            WHERE unshielded_utxos.owner_address = ?
        "};

        let highest_index: Option<i64> = sqlx::query_scalar(query)
            .bind(address.as_ref())
            .fetch_one(&*self.pool)
            .await?;

        Ok(highest_index.map(|n| n as u64))
    }
}

impl SqliteStorage {
    async fn enrich_utxos_with_transaction_data_optimized(
        &self,
        utxos: &mut [UnshieldedUtxo],
    ) -> Result<(), sqlx::Error> {
        if utxos.is_empty() {
            return Ok(());
        }

        let mut transaction_ids = std::collections::HashSet::new();
        for utxo in utxos.iter() {
            transaction_ids.insert(utxo.creating_transaction_id);
            if let Some(spending_id) = utxo.spending_transaction_id {
                transaction_ids.insert(spending_id);
            }
        }

        let transaction_ids: Vec<u64> = transaction_ids.into_iter().collect();

        let transactions_map = self.get_transactions_by_ids(&transaction_ids).await?;

        for utxo in utxos {
            utxo.created_at_transaction =
                transactions_map.get(&utxo.creating_transaction_id).cloned();

            if let Some(spending_id) = utxo.spending_transaction_id {
                utxo.spent_at_transaction = transactions_map.get(&spending_id).cloned();
            }
        }

        Ok(())
    }

    async fn get_transactions_by_ids(
        &self,
        transaction_ids: &[u64],
    ) -> Result<HashMap<u64, Transaction>, sqlx::Error> {
        if transaction_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let placeholders = (0..transaction_ids.len())
            .map(|i| format!("${}", i + 1))
            .collect::<Vec<_>>()
            .join(", ");
        let query = format!(
            indoc! {"
                SELECT
                    transactions.id, transactions.hash, blocks.hash as block_hash,
                    transactions.protocol_version, transactions.transaction_result,
                    transactions.raw, transactions.merkle_tree_root,
                    transactions.start_index, transactions.end_index
                FROM transactions
                INNER JOIN blocks ON blocks.id = transactions.block_id
                WHERE transactions.id IN ({})
            "},
            placeholders
        );

        let mut query_builder = sqlx::query_as::<_, Transaction>(&query);
        for &id in transaction_ids {
            query_builder = query_builder.bind(id as i64);
        }
        let mut transactions = query_builder.fetch_all(&*self.pool).await?;

        let mut identifiers_map = self
            .get_identifiers_for_transactions(transaction_ids)
            .await?;

        for transaction in &mut transactions {
            if let Some(identifiers) = identifiers_map.remove(&transaction.id) {
                transaction.identifiers = identifiers;
            }
        }

        let mut result = HashMap::new();
        for transaction in transactions {
            result.insert(transaction.id, transaction);
        }

        Ok(result)
    }
}
