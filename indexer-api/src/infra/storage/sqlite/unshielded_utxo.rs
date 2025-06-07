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
    domain::{UnshieldedUtxo, storage::unshielded_utxo::UnshieldedUtxoStorage},
    infra::storage::sqlite::SqliteStorage,
};
use indexer_common::domain::{BlockHash, Identifier, TransactionHash, UnshieldedAddress};
use indoc::indoc;

impl UnshieldedUtxoStorage for SqliteStorage {
    async fn get_unshielded_utxos_by_address(
        &self,
        address: &UnshieldedAddress,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                id, owner_address, token_type, value, output_index, intent_hash,
                creating_transaction_id, spending_transaction_id
            FROM unshielded_utxos
            WHERE owner_address = ?
            ORDER BY id ASC
        "};

        let utxos = sqlx::query_as::<_, UnshieldedUtxo>(query)
            .bind(address.as_ref())
            .fetch_all(&*self.pool)
            .await?;

        Ok(utxos)
    }

    async fn get_unshielded_utxos_created_by_transaction(
        &self,
        transaction_id: u64,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let query = indoc! {"
            SELECT *
            FROM unshielded_utxos
            WHERE creating_transaction_id = ?
        "};

        let utxos = sqlx::query_as::<_, UnshieldedUtxo>(query)
            .bind(transaction_id as i64)
            .fetch_all(&*self.pool)
            .await?;

        Ok(utxos)
    }

    async fn get_unshielded_utxos_spent_by_transaction(
        &self,
        transaction_id: u64,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let query = indoc! {"
            SELECT *
            FROM unshielded_utxos
            WHERE spending_transaction_id = ?
        "};

        let utxos = sqlx::query_as::<_, UnshieldedUtxo>(query)
            .bind(transaction_id as i64)
            .fetch_all(&*self.pool)
            .await?;

        Ok(utxos)
    }

    async fn get_unshielded_utxos_created_in_transaction_for_address(
        &self,
        address: &UnshieldedAddress,
        transaction_id: u64,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let query = indoc! {"
            SELECT *
            FROM unshielded_utxos
            WHERE creating_transaction_id = ?
            AND owner_address = ?
        "};

        let utxos = sqlx::query_as::<_, UnshieldedUtxo>(query)
            .bind(transaction_id as i64)
            .bind(address.as_ref())
            .fetch_all(&*self.pool)
            .await?;

        Ok(utxos)
    }

    async fn get_unshielded_utxos_spent_in_transaction_for_address(
        &self,
        address: &UnshieldedAddress,
        transaction_id: u64,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let query = indoc! {"
            SELECT *
            FROM unshielded_utxos
            WHERE spending_transaction_id = ?
            AND owner_address = ?
        "};

        let utxos = sqlx::query_as::<_, UnshieldedUtxo>(query)
            .bind(transaction_id as i64)
            .bind(address.as_ref())
            .fetch_all(&*self.pool)
            .await?;

        Ok(utxos)
    }

    async fn get_unshielded_utxos_by_address_from_height(
        &self,
        address: &UnshieldedAddress,
        height: u32,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let query = indoc! {"
            SELECT unshielded_utxos.*
            FROM unshielded_utxos
            JOIN transactions  ON   transactions.id = unshielded_utxos.creating_transaction_id
            JOIN blocks        ON   blocks.id = transactions.block_id
            WHERE unshielded_utxos.owner_address = ?
              AND blocks.height >= ?
            ORDER BY unshielded_utxos.id ASC
        "};

        let utxos = sqlx::query_as::<_, UnshieldedUtxo>(query)
            .bind(address.as_ref())
            .bind(height as i64)
            .fetch_all(&*self.pool)
            .await?;

        Ok(utxos)
    }

    async fn get_unshielded_utxos_by_address_from_block_hash(
        &self,
        address: &UnshieldedAddress,
        block_hash: &BlockHash,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let query = indoc! {"
            SELECT unshielded_utxos.*
            FROM unshielded_utxos
            JOIN transactions  ON   transactions.id = unshielded_utxos.creating_transaction_id
            JOIN blocks        ON   blocks.id = transactions.block_id
            WHERE unshielded_utxos.owner_address = ?
            AND blocks.hash = ?
            ORDER BY unshielded_utxos.id ASC
        "};

        let utxos = sqlx::query_as::<_, UnshieldedUtxo>(query)
            .bind(address.as_ref())
            .bind(block_hash.as_ref())
            .fetch_all(&*self.pool)
            .await?;

        Ok(utxos)
    }

    async fn get_unshielded_utxos_by_address_from_transaction_hash(
        &self,
        address: &UnshieldedAddress,
        transaction_hash: &TransactionHash,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let query = indoc! {"
            SELECT unshielded_utxos.*
            FROM unshielded_utxos
            JOIN transactions   ON  transactions.id = unshielded_utxos.creating_transaction_id
            WHERE unshielded_utxos.owner_address = ?
            AND transactions.hash = ?
            ORDER BY unshielded_utxos.id ASC
        "};

        let utxos = sqlx::query_as::<_, UnshieldedUtxo>(query)
            .bind(address.as_ref())
            .bind(transaction_hash.as_ref())
            .fetch_all(&*self.pool)
            .await?;

        Ok(utxos)
    }

    async fn get_unshielded_utxos_by_address_from_transaction_identifier(
        &self,
        address: &UnshieldedAddress,
        identifier: &Identifier,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let query = indoc! {"
            SELECT unshielded_utxos.*
            FROM unshielded_utxos
            JOIN transaction_identifiers
                ON transaction_identifiers.transaction_id = unshielded_utxos.creating_transaction_id
            WHERE unshielded_utxos.owner_address = ?
              AND transaction_identifiers.identifier = ?
            ORDER BY unshielded_utxos.id ASC
        "};

        let utxos = sqlx::query_as::<_, UnshieldedUtxo>(query)
            .bind(address.as_ref())
            .bind(identifier)
            .fetch_all(&*self.pool)
            .await?;

        Ok(utxos)
    }

    async fn get_highest_indices_for_address(
        &self,
        address: &UnshieldedAddress,
    ) -> Result<(Option<u64>, Option<u64>), sqlx::Error> {
        let query = indoc! {"
            SELECT (
                SELECT MAX(end_index) FROM transactions
            ) AS highest_end_index,
            (
                SELECT MAX(transactions.end_index)
                FROM transactions
                INNER JOIN unshielded_utxos ON 
                    unshielded_utxos.creating_transaction_id = transactions.id OR
                    unshielded_utxos.spending_transaction_id = transactions.id
                WHERE unshielded_utxos.owner_address = ?
            ) AS highest_end_index_for_address
        "};

        let (highest_index, highest_index_for_address) =
            sqlx::query_as::<_, (Option<i64>, Option<i64>)>(query)
                .bind(address.as_ref())
                .fetch_one(&*self.pool)
                .await?;

        Ok((
            highest_index.map(|n| n as u64),
            highest_index_for_address.map(|n| n as u64),
        ))
    }
}
