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
    domain::{UnshieldedUtxo, storage::unshielded::UnshieldedUtxoStorage},
    infra::storage::postgres::PostgresStorage,
};
use fastrace::trace;
use indexer_common::domain::{
    BlockHash, RawTransactionIdentifier, RawUnshieldedAddress, TransactionHash,
};
use indoc::indoc;

impl UnshieldedUtxoStorage for PostgresStorage {
    #[trace(properties = { "address": "{address}" })]
    async fn get_unshielded_utxos_by_address(
        &self,
        address: RawUnshieldedAddress,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                id,
                creating_transaction_id,
                spending_transaction_id,
                owner,
                token_type,
                value,
                output_index,
                intent_hash
            FROM unshielded_utxos
            WHERE owner = $1
            ORDER BY id
        "};

        let utxos = sqlx::query_as::<_, UnshieldedUtxo>(query)
            .bind(address.as_ref())
            .fetch_all(&*self.pool)
            .await?;

        Ok(utxos)
    }

    #[trace(properties = { "transaction_id": "{transaction_id}" })]
    async fn get_unshielded_utxos_created_by_transaction(
        &self,
        transaction_id: u64,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                id,
                creating_transaction_id,
                spending_transaction_id,
                owner,
                token_type,
                value,
                output_index,
                intent_hash
            FROM unshielded_utxos
            WHERE creating_transaction_id = $1
            ORDER BY output_index
        "};

        let utxos = sqlx::query_as::<_, UnshieldedUtxo>(query)
            .bind(transaction_id as i64)
            .fetch_all(&*self.pool)
            .await?;

        Ok(utxos)
    }

    #[trace(properties = { "transaction_id": "{transaction_id}" })]
    async fn get_unshielded_utxos_spent_by_transaction(
        &self,
        transaction_id: u64,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                id,
                creating_transaction_id,
                spending_transaction_id,
                owner,
                token_type,
                value,
                output_index,
                intent_hash
            FROM unshielded_utxos
            WHERE spending_transaction_id = $1
            ORDER BY output_index
        "};

        let utxos = sqlx::query_as::<_, UnshieldedUtxo>(query)
            .bind(transaction_id as i64)
            .fetch_all(&*self.pool)
            .await?;

        Ok(utxos)
    }

    #[trace(properties = { "address": "{address}", "transaction_id": "{transaction_id}" })]
    async fn get_unshielded_utxos_created_in_transaction_for_address(
        &self,
        address: RawUnshieldedAddress,
        transaction_id: u64,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                id,
                creating_transaction_id,
                spending_transaction_id,
                owner,
                token_type,
                value,
                output_index,
                intent_hash
            FROM unshielded_utxos
            WHERE creating_transaction_id = $1
            AND owner = $2
            ORDER BY output_index
        "};

        let utxos = sqlx::query_as::<_, UnshieldedUtxo>(query)
            .bind(transaction_id as i64)
            .bind(address.as_ref())
            .fetch_all(&*self.pool)
            .await?;

        Ok(utxos)
    }

    #[trace(properties = { "address": "{address}", "transaction_id": "{transaction_id}" })]
    async fn get_unshielded_utxos_spent_in_transaction_for_address(
        &self,
        address: RawUnshieldedAddress,
        transaction_id: u64,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                id,
                creating_transaction_id,
                spending_transaction_id,
                owner,
                token_type,
                value,
                output_index,
                intent_hash
            FROM unshielded_utxos
            WHERE spending_transaction_id = $1
            AND owner = $2
            ORDER BY output_index
        "};

        let utxos = sqlx::query_as::<_, UnshieldedUtxo>(query)
            .bind(transaction_id as i64)
            .bind(address.as_ref())
            .fetch_all(&*self.pool)
            .await?;

        Ok(utxos)
    }

    #[trace(properties = { "address": "{address}", "height": "{height}" })]
    async fn get_unshielded_utxos_by_address_from_height(
        &self,
        address: RawUnshieldedAddress,
        height: u32,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let query = indoc! {"
            SELECT 
                id,
                creating_transaction_id,
                spending_transaction_id,
                owner,
                token_type,
                value,
                output_index,
                intent_hash
            FROM unshielded_utxos
            JOIN transactions ON transactions.id = creating_transaction_id
            JOIN blocks ON blocks.id = transactions.block_id
            WHERE owner = $1
            AND blocks.height >= $2
            ORDER BY id
        "};

        let utxos = sqlx::query_as::<_, UnshieldedUtxo>(query)
            .bind(address.as_ref())
            .bind(height as i64)
            .fetch_all(&*self.pool)
            .await?;

        Ok(utxos)
    }

    #[trace(properties = { "address": "{address}", "hash": "{hash}" })]
    async fn get_unshielded_utxos_by_address_from_block_hash(
        &self,
        address: RawUnshieldedAddress,
        hash: BlockHash,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                id,
                creating_transaction_id,
                spending_transaction_id,
                owner,
                token_type,
                value,
                output_index,
                intent_hash
            FROM unshielded_utxos
            JOIN transactions ON transactions.id = creating_transaction_id
            JOIN blocks ON blocks.id = transactions.block_id
            WHERE owner = $1
            AND blocks.hash = $2
            ORDER BY id
        "};

        let utxos = sqlx::query_as::<_, UnshieldedUtxo>(query)
            .bind(address.as_ref())
            .bind(hash)
            .fetch_all(&*self.pool)
            .await?;

        Ok(utxos)
    }

    #[trace(properties = { "address": "{address}", "hash": "{hash}" })]
    async fn get_unshielded_utxos_by_address_from_transaction_hash(
        &self,
        address: RawUnshieldedAddress,
        hash: TransactionHash,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let query = indoc! {"
            SELECT 
                id,
                creating_transaction_id,
                spending_transaction_id,
                owner,
                token_type,
                value,
                output_index,
                intent_hash
            FROM unshielded_utxos
            JOIN transactions ON transactions.id = creating_transaction_id
            WHERE owner = $1
            AND transactions.hash = $2
            ORDER BY id
        "};

        let utxos = sqlx::query_as::<_, UnshieldedUtxo>(query)
            .bind(address.as_ref())
            .bind(hash)
            .fetch_all(&*self.pool)
            .await?;

        Ok(utxos)
    }

    #[trace(properties = { "address": "{address}", "identifier": "{identifier}" })]
    async fn get_unshielded_utxos_by_address_from_transaction_identifier(
        &self,
        address: RawUnshieldedAddress,
        identifier: &RawTransactionIdentifier,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let query = indoc! {"
            SELECT 
                id,
                creating_transaction_id,
                spending_transaction_id,
                owner,
                token_type,
                value,
                output_index,
                intent_hash
            FROM unshielded_utxos
            JOIN transactions ON transactions.id = creating_transaction_id
            WHERE owner = $1
            AND $2 = ANY(transactions.identifiers)
            ORDER BY id
        "};

        let utxos = sqlx::query_as::<_, UnshieldedUtxo>(query)
            .bind(address.as_ref())
            .bind(identifier)
            .fetch_all(&*self.pool)
            .await?;

        Ok(utxos)
    }
}
