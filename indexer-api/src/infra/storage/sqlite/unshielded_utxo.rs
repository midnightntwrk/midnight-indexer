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

impl UnshieldedUtxoStorage for SqliteStorage {
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
        self.enrich_utxos_with_transaction_data(&mut utxos).await?;

        Ok(utxos)
    }
}

impl SqliteStorage {
    async fn enrich_utxos_with_transaction_data(
        &self,
        utxos: &mut [UnshieldedUtxo],
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
                .fetch_optional(&*self.pool)
                .await?;

            if let Some(t) = &mut creating_tx {
                t.identifiers = self.get_identifiers_by_transaction_id(t.id).await?;
            }

            utxo.created_at_transaction = creating_tx;

            if let Some(spending_tx_id) = utxo.spending_transaction_id {
                let mut spending_tx = sqlx::query_as::<_, Transaction>(sql)
                    .bind(spending_tx_id as i64)
                    .fetch_optional(&*self.pool)
                    .await?;

                if let Some(transaction) = &mut spending_tx {
                    transaction.identifiers = self
                        .get_identifiers_by_transaction_id(transaction.id)
                        .await?;
                }

                utxo.spent_at_transaction = spending_tx;
            }
        }

        Ok(())
    }
}
