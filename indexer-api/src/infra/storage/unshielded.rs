// This file is part of midnight-indexer.
// Copyright (C) Midnight Foundation
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
    infra::storage::Storage,
};
use fastrace::trace;
use indexer_common::domain::UnshieldedAddress;
use indoc::indoc;

impl UnshieldedUtxoStorage for Storage {
    #[trace(properties = { "address": "{address}" })]
    async fn get_unshielded_utxos_by_address(
        &self,
        address: UnshieldedAddress,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                id,
                creating_transaction_id,
                spending_transaction_id,
                owner,
                token_type,
                value,
                intent_hash,
                output_index,
                ctime,
                initial_nonce,
                registered_for_dust_generation
            FROM unshielded_utxos
            WHERE owner = $1
            ORDER BY id
        "};

        let utxos = sqlx::query_as(query)
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
                intent_hash,
                output_index,
                ctime,
                initial_nonce,
                registered_for_dust_generation
            FROM unshielded_utxos
            WHERE creating_transaction_id = $1
            ORDER BY output_index
        "};

        let utxos = sqlx::query_as(query)
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
                intent_hash,
                output_index,
                ctime,
                initial_nonce,
                registered_for_dust_generation
            FROM unshielded_utxos
            WHERE spending_transaction_id = $1
            ORDER BY output_index
        "};

        let utxos = sqlx::query_as(query)
            .bind(transaction_id as i64)
            .fetch_all(&*self.pool)
            .await?;

        Ok(utxos)
    }

    #[trace(properties = { "address": "{address}", "transaction_id": "{transaction_id}" })]
    async fn get_unshielded_utxos_by_address_created_by_transaction(
        &self,
        address: UnshieldedAddress,
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
                intent_hash,
                output_index,
                ctime,
                initial_nonce,
                registered_for_dust_generation
            FROM unshielded_utxos
            WHERE creating_transaction_id = $1
            AND owner = $2
            ORDER BY output_index
        "};

        let utxos = sqlx::query_as(query)
            .bind(transaction_id as i64)
            .bind(address.as_ref())
            .fetch_all(&*self.pool)
            .await?;

        Ok(utxos)
    }

    async fn get_unshielded_utxos_by_address_spent_by_transaction(
        &self,
        address: UnshieldedAddress,
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
                intent_hash,
                output_index,
                ctime,
                initial_nonce,
                registered_for_dust_generation
            FROM unshielded_utxos
            WHERE spending_transaction_id = $1
            AND owner = $2
            ORDER BY output_index
        "};

        let utxos = sqlx::query_as(query)
            .bind(transaction_id as i64)
            .bind(address.as_ref())
            .fetch_all(&*self.pool)
            .await?;

        Ok(utxos)
    }

}

impl Storage {
    pub(crate) async fn get_unshielded_utxos_created_by_transaction_ids(
        &self,
        transaction_ids: &[u64],
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let ids = transaction_ids.iter().map(|&id| id as i64).collect::<Vec<_>>();

        #[cfg(feature = "cloud")]
        let query = indoc! {"
            SELECT
                id, creating_transaction_id, spending_transaction_id, owner, token_type, value,
                intent_hash, output_index, ctime, initial_nonce, registered_for_dust_generation
            FROM unshielded_utxos
            WHERE creating_transaction_id = ANY($1)
            ORDER BY creating_transaction_id, output_index
        "};

        #[cfg(feature = "standalone")]
        let query = indoc! {"
            SELECT
                unshielded_utxos.id, creating_transaction_id, spending_transaction_id, owner,
                token_type, value, intent_hash, output_index, ctime, initial_nonce,
                registered_for_dust_generation
            FROM unshielded_utxos
            INNER JOIN json_each($1) as batch_ids ON creating_transaction_id = batch_ids.value
            ORDER BY creating_transaction_id, output_index
        "};

        #[cfg(feature = "cloud")]
        {
            sqlx::query_as(query)
                .bind(ids)
                .fetch_all(&*self.pool)
                .await
        }

        #[cfg(feature = "standalone")]
        {
            let ids_json = serde_json::to_string(&ids)
                .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
            sqlx::query_as(query)
                .bind(ids_json)
                .fetch_all(&*self.pool)
                .await
        }
    }

    pub(crate) async fn get_unshielded_utxos_spent_by_transaction_ids(
        &self,
        transaction_ids: &[u64],
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let ids = transaction_ids.iter().map(|&id| id as i64).collect::<Vec<_>>();

        #[cfg(feature = "cloud")]
        let query = indoc! {"
            SELECT
                id, creating_transaction_id, spending_transaction_id, owner, token_type, value,
                intent_hash, output_index, ctime, initial_nonce, registered_for_dust_generation
            FROM unshielded_utxos
            WHERE spending_transaction_id = ANY($1)
            ORDER BY spending_transaction_id, output_index
        "};

        #[cfg(feature = "standalone")]
        let query = indoc! {"
            SELECT
                unshielded_utxos.id, creating_transaction_id, spending_transaction_id, owner,
                token_type, value, intent_hash, output_index, ctime, initial_nonce,
                registered_for_dust_generation
            FROM unshielded_utxos
            INNER JOIN json_each($1) as batch_ids ON spending_transaction_id = batch_ids.value
            ORDER BY spending_transaction_id, output_index
        "};

        #[cfg(feature = "cloud")]
        {
            sqlx::query_as(query)
                .bind(ids)
                .fetch_all(&*self.pool)
                .await
        }

        #[cfg(feature = "standalone")]
        {
            let ids_json = serde_json::to_string(&ids)
                .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
            sqlx::query_as(query)
                .bind(ids_json)
                .fetch_all(&*self.pool)
                .await
        }
    }
}

