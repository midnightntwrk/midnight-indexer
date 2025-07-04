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
    domain::{ContractAction, ContractAttributes, storage::contract_action::ContractActionStorage},
    infra::storage::sqlite::SqliteStorage,
};
use async_stream::try_stream;
use futures::{Stream, stream::TryStreamExt};
use indexer_common::{
    domain::{BlockHash, RawContractAddress, RawTransactionIdentifier, TransactionHash},
    stream::flatten_chunks,
};
use indoc::indoc;
use std::num::NonZeroU32;

impl ContractActionStorage for SqliteStorage {
    async fn get_contract_deploy_by_address(
        &self,
        address: &RawContractAddress,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        // For any address the first contract action is always a deploy.
        let query = indoc! {"
            SELECT
                id,
                address,
                state,
                attributes,
                zswap_state,
                transaction_id
            FROM contract_actions
            WHERE contract_actions.address = $1
            ORDER BY id
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

    async fn get_latest_contract_action_by_address(
        &self,
        address: &RawContractAddress,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                contract_actions.id,
                address,
                state,
                attributes,
                zswap_state,
                transaction_id
            FROM contract_actions
            WHERE address = $1
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
        address: &RawContractAddress,
        hash: BlockHash,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                contract_actions.id,
                address,
                state,
                attributes,
                zswap_state,
                transaction_id
            FROM contract_actions
            INNER JOIN transactions ON transactions.id = transaction_id
            WHERE address = $1
            AND transactions.block_id = (SELECT id FROM blocks WHERE hash = $2)
            ORDER BY contract_actions.id DESC
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
        address: &RawContractAddress,
        height: u32,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                contract_actions.id,
                address,
                state,
                attributes,
                zswap_state,
                transaction_id
            FROM contract_actions
            INNER JOIN transactions ON transactions.id = transaction_id
            INNER JOIN blocks ON blocks.id = transactions.block_id
            WHERE address = $1
            AND blocks.height = $2
            ORDER BY contract_actions.id DESC
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
        address: &RawContractAddress,
        hash: TransactionHash,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                contract_actions.id,
                address,
                state,
                attributes,
                zswap_state,
                transaction_id
            FROM contract_actions
            WHERE address = $1
            AND contract_actions.transaction_id = (
                SELECT id FROM transactions
                WHERE hash = $2
                ORDER BY id
                LIMIT 1
            )
            ORDER BY contract_actions.id DESC
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
        address: &RawContractAddress,
        identifier: &RawTransactionIdentifier,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                contract_actions.id,
                address,
                state,
                attributes,
                zswap_state,
                contract_actions.transaction_id
            FROM contract_actions
            INNER JOIN transactions ON transactions.id = contract_actions.transaction_id
            INNER JOIN transaction_identifiers ON transactions.id = transaction_identifiers.transaction_id
            WHERE address = $1
            AND transaction_identifiers.identifier = $2
            ORDER BY contract_actions.id DESC
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
                id,
                address,
                state,
                attributes,
                zswap_state,
                transaction_id
            FROM contract_actions
            WHERE transaction_id = $1
            ORDER BY id
        "};

        sqlx::query_as::<_, ContractAction>(query)
            .bind(id as i64)
            .fetch_all(&*self.pool)
            .await
    }

    fn get_contract_actions_by_address(
        &self,
        address: &RawContractAddress,
        mut contract_action_id: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<ContractAction, sqlx::Error>> + Send {
        let chunks = try_stream! {
            loop {
                let query = indoc! {"
                    SELECT
                        contract_actions.id,
                        address,
                        state,
                        attributes,
                        zswap_state,
                        transaction_id
                    FROM contract_actions
                    INNER JOIN transactions ON transactions.id = transaction_id
                    INNER JOIN blocks ON blocks.id = transactions.block_id
                    WHERE address = $1
                    AND contract_actions.id >= $2
                    ORDER BY contract_actions.id
                    LIMIT $3
                "};

                let actions = sqlx::query_as(query)
                    .bind(address)
                    .bind(contract_action_id as i64)
                    .bind(batch_size.get() as i64)
                    .fetch(&*self.pool)
                    .map_ok(ContractAction::from)
                    .try_collect::<Vec<_>>()
                    .await?;

                match actions.last() {
                    Some(action) => contract_action_id = action.id + 1,
                    None => break,
                }

                yield actions;
            }
        };

        flatten_chunks(chunks)
    }

    async fn get_unshielded_balances_by_action_id(
        &self,
        contract_action_id: u64,
    ) -> Result<Vec<crate::domain::ContractBalance>, sqlx::Error> {
        let query = indoc! {"
            SELECT token_type, amount 
            FROM contract_balances 
            WHERE contract_action_id = ?
        "};

        sqlx::query_as::<_, crate::domain::ContractBalance>(query)
            .bind(contract_action_id as i64)
            .fetch_all(&*self.pool)
            .await
    }

    async fn get_contract_action_id_by_block_height(
        &self,
        block_height: u32,
    ) -> Result<Option<u64>, sqlx::Error> {
        let query = indoc! {"
            SELECT contract_actions.id
            FROM contract_actions
            JOIN transactions ON transactions.id = contract_actions.transaction_id
            JOIN blocks ON blocks.id = transactions.block_id
            WHERE blocks.height >= $1
            ORDER BY contract_actions.id
            LIMIT 1
        "};

        let id = sqlx::query_as::<_, (i64,)>(query)
            .bind(block_height as i64)
            .fetch_optional(&*self.pool)
            .await?;

        Ok(id.map(|(id,)| id as u64))
    }
}
