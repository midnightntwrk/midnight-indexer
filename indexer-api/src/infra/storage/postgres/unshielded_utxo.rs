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
        UnshieldedUtxo,
        storage::{
            transaction::TransactionStorage,
            unshielded_utxo::{UnshieldedUtxoFilter, UnshieldedUtxoStorage},
        },
    },
    infra::storage::postgres::PostgresStorage,
};
use indexer_common::domain::UnshieldedAddress;
use indoc::indoc;

impl UnshieldedUtxoStorage for PostgresStorage {
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
                WHERE owner_address = $1
                ORDER BY id ASC
            "}
            }
            (None, UnshieldedUtxoFilter::CreatedByTx(_)) => {
                indoc! {"
                SELECT
                    id, owner_address, token_type, value, output_index, intent_hash,
                    creating_transaction_id, spending_transaction_id
                FROM unshielded_utxos
                WHERE creating_transaction_id = $1
            "}
            }
            (None, UnshieldedUtxoFilter::SpentByTx(_)) => {
                indoc! {"
                SELECT
                    id, owner_address, token_type, value, output_index, intent_hash,
                    creating_transaction_id, spending_transaction_id
                FROM unshielded_utxos
                WHERE spending_transaction_id = $1
            "}
            }
            (Some(_), UnshieldedUtxoFilter::CreatedInTxForAddress(_)) => {
                indoc! {"
                SELECT
                    id, owner_address, token_type, value, output_index, intent_hash,
                    creating_transaction_id, spending_transaction_id
                FROM unshielded_utxos
                WHERE creating_transaction_id = $1
                AND owner_address = $2
            "}
            }
            (Some(_), UnshieldedUtxoFilter::SpentInTxForAddress(_)) => {
                indoc! {"
                SELECT
                    id, owner_address, token_type, value, output_index, intent_hash,
                    creating_transaction_id, spending_transaction_id
                FROM unshielded_utxos
                WHERE spending_transaction_id = $1
                AND owner_address = $2
            "}
            }
            (Some(_), UnshieldedUtxoFilter::FromHeight(_)) => {
                indoc! {"
                SELECT unshielded_utxos.id,
                       unshielded_utxos.owner_address,
                       unshielded_utxos.token_type,
                       unshielded_utxos.value,
                       unshielded_utxos.output_index,
                       unshielded_utxos.intent_hash,
                       unshielded_utxos.creating_transaction_id,
                       unshielded_utxos.spending_transaction_id
                FROM   unshielded_utxos
                JOIN   transactions ON transactions.id = unshielded_utxos.creating_transaction_id
                JOIN   blocks ON blocks.id = transactions.block_id
                WHERE  unshielded_utxos.owner_address = $1
                  AND  blocks.height >= $2
                ORDER  BY unshielded_utxos.id ASC
            "}
            }
            (Some(_), UnshieldedUtxoFilter::FromBlockHash(_)) => {
                indoc! {"
                SELECT unshielded_utxos.id,
                       unshielded_utxos.owner_address,
                       unshielded_utxos.token_type,
                       unshielded_utxos.value,
                       unshielded_utxos.output_index,
                       unshielded_utxos.intent_hash,
                       unshielded_utxos.creating_transaction_id,
                       unshielded_utxos.spending_transaction_id
                FROM   unshielded_utxos
                JOIN   transactions ON transactions.id = unshielded_utxos.creating_transaction_id
                JOIN   blocks ON blocks.id = transactions.block_id
                WHERE  unshielded_utxos.owner_address = $1
                  AND  blocks.hash = $2
                ORDER  BY unshielded_utxos.id ASC
            "}
            }
            (Some(_), UnshieldedUtxoFilter::FromTxHash(_)) => {
                indoc! {"
                SELECT unshielded_utxos.id,
                       unshielded_utxos.owner_address,
                       unshielded_utxos.token_type,
                       unshielded_utxos.value,
                       unshielded_utxos.output_index,
                       unshielded_utxos.intent_hash,
                       unshielded_utxos.creating_transaction_id,
                       unshielded_utxos.spending_transaction_id
                FROM   unshielded_utxos
                JOIN   transactions ON transactions.id = unshielded_utxos.creating_transaction_id
                WHERE  unshielded_utxos.owner_address = $1
                  AND  transactions.hash = $2
                ORDER  BY unshielded_utxos.id ASC
            "}
            }
            (Some(_), UnshieldedUtxoFilter::FromTxIdentifier(_)) => {
                indoc! {"
                SELECT unshielded_utxos.id,
                       unshielded_utxos.owner_address,
                       unshielded_utxos.token_type,
                       unshielded_utxos.value,
                       unshielded_utxos.output_index,
                       unshielded_utxos.intent_hash,
                       unshielded_utxos.creating_transaction_id,
                       unshielded_utxos.spending_transaction_id
                FROM   unshielded_utxos
                JOIN   transactions ON transactions.id = unshielded_utxos.creating_transaction_id
                WHERE  unshielded_utxos.owner_address = $1
                  AND  $2 = ANY(transactions.identifiers)
                ORDER  BY unshielded_utxos.id ASC
            "}
            }
            _ => {
                return Err(sqlx::Error::Protocol(
                    "Unsupported filter combination".into(),
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
            (Some(addr), UnshieldedUtxoFilter::FromHeight(height)) => {
                query = query.bind(addr.as_ref());
                query = query.bind(*height as i64);
            }
            (Some(addr), UnshieldedUtxoFilter::FromBlockHash(hash)) => {
                query = query.bind(addr.as_ref());
                query = query.bind(*hash);
            }
            (Some(addr), UnshieldedUtxoFilter::FromTxHash(hash)) => {
                query = query.bind(addr.as_ref());
                query = query.bind(*hash);
            }
            (Some(addr), UnshieldedUtxoFilter::FromTxIdentifier(identifier)) => {
                query = query.bind(addr.as_ref());
                query = query.bind(*identifier);
            }
            _ => {}
        };

        let mut utxos = query.fetch_all(&*self.pool).await?;

        for utxo in utxos.iter_mut() {
            utxo.created_at_transaction = self
                .get_transaction_by_id(utxo.creating_transaction_id)
                .await?;

            if let Some(spending_tx_id) = utxo.spending_transaction_id {
                utxo.spent_at_transaction = self.get_transaction_by_id(spending_tx_id).await?;
            }
        }

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
            WHERE unshielded_utxos.owner_address = $1
        "};

        let highest_index: Option<i64> = sqlx::query_scalar(query)
            .bind(address.as_ref())
            .fetch_one(&*self.pool)
            .await?;

        Ok(highest_index.map(|n| n as u64))
    }
}
