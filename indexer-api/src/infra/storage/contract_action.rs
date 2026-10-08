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
    domain::{
        ContractAction, ContractActionAtBlock, storage::contract_action::ContractActionStorage,
    },
    infra::storage::Storage,
};
use async_stream::try_stream;
use fastrace::trace;
use futures::{Stream, TryStreamExt};
use indexer_common::{
    domain::{
        BlockHash, ContractAttributes, SerializedContractAddress, SerializedTransactionIdentifier,
        TransactionHash,
    },
    stream::flatten_chunks,
};
use indoc::indoc;
use std::num::NonZeroU32;

impl ContractActionStorage for Storage {
    #[trace(properties = { "address": "{address}" })]
    async fn get_contract_deploy_by_address(
        &self,
        address: &SerializedContractAddress,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        // For any address the first contract action is always a deploy.
        let query = indoc! {"
            SELECT
                id,
                address,
                state_key,
                attributes,
                zswap_state_key,
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

    // The view queries below serve the newest translation of the action recorded at or before the
    // anchor block, else the action's own key.
    #[trace(properties = { "address": "{address}" })]
    async fn get_latest_contract_action_by_address(
        &self,
        address: &SerializedContractAddress,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                contract_actions.id,
                address,
                COALESCE(
                    (
                        SELECT t.state_key
                        FROM contract_action_translations t
                        JOIN blocks b ON b.id = t.block_id
                        WHERE t.contract_action_id = contract_actions.id
                        ORDER BY b.height DESC
                        LIMIT 1
                    ),
                    state_key
                ) AS state_key,
                attributes,
                zswap_state_key,
                transaction_id,
                (
                    SELECT b.height
                    FROM contract_action_translations t
                    JOIN blocks b ON b.id = t.block_id
                    WHERE t.contract_action_id = contract_actions.id
                    ORDER BY b.height DESC
                    LIMIT 1
                ) AS translated_at
            FROM contract_actions
            WHERE address = $1
            ORDER BY id DESC
            LIMIT 1
        "};

        sqlx::query_as(query)
            .bind(address)
            .fetch_optional(&*self.pool)
            .await
    }

    #[trace(properties = { "address": "{address}", "hash": "{hash}" })]
    async fn get_contract_action_by_address_and_block_hash(
        &self,
        address: &SerializedContractAddress,
        hash: BlockHash,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                contract_actions.id,
                address,
                state_key,
                attributes,
                zswap_state_key,
                transaction_id
            FROM contract_actions
            INNER JOIN transactions ON transactions.id = transaction_id
            WHERE address = $1
            AND transactions.block_id = (SELECT id FROM blocks WHERE hash = $2)
            ORDER BY contract_actions.id DESC
            LIMIT 1
        "};

        sqlx::query_as(query)
            .bind(address.as_ref())
            .bind(hash.as_ref())
            .fetch_optional(&*self.pool)
            .await
    }

    #[trace(properties = { "address": "{address}", "block_height": "{block_height}" })]
    async fn get_contract_action_by_address_and_block_height(
        &self,
        address: &SerializedContractAddress,
        block_height: u32,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                contract_actions.id,
                address,
                state_key,
                attributes,
                zswap_state_key,
                transaction_id
            FROM contract_actions
            INNER JOIN transactions ON transactions.id = transaction_id
            INNER JOIN blocks ON blocks.id = transactions.block_id
            WHERE address = $1
            AND blocks.height = $2
            ORDER BY contract_actions.id DESC
            LIMIT 1
        "};

        sqlx::query_as(query)
            .bind(address)
            .bind(block_height as i64)
            .fetch_optional(&*self.pool)
            .await
    }

    #[trace(properties = { "address": "{address}", "hash": "{hash}" })]
    async fn get_contract_action_by_address_as_of_block_hash(
        &self,
        address: &SerializedContractAddress,
        hash: BlockHash,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        // "State as of" the given block: the latest action for the address in any block at or
        // before the one with the given hash, not just actions in that exact block. Lets a contract
        // deployed in an earlier block still resolve at a later pinned block.
        let query = indoc! {"
            SELECT
                contract_actions.id,
                address,
                COALESCE(
                    (
                        SELECT t.state_key
                        FROM contract_action_translations t
                        JOIN blocks b ON b.id = t.block_id
                        WHERE t.contract_action_id = contract_actions.id
                        AND b.height <= (SELECT height FROM blocks WHERE hash = $2)
                        ORDER BY b.height DESC
                        LIMIT 1
                    ),
                    state_key
                ) AS state_key,
                attributes,
                zswap_state_key,
                transaction_id,
                (
                    SELECT b.height
                    FROM contract_action_translations t
                    JOIN blocks b ON b.id = t.block_id
                    WHERE t.contract_action_id = contract_actions.id
                    AND b.height <= (SELECT height FROM blocks WHERE hash = $2)
                    ORDER BY b.height DESC
                    LIMIT 1
                ) AS translated_at
            FROM contract_actions
            INNER JOIN transactions ON transactions.id = transaction_id
            INNER JOIN blocks ON blocks.id = transactions.block_id
            WHERE address = $1
            AND blocks.height <= (SELECT height FROM blocks WHERE hash = $2)
            ORDER BY contract_actions.id DESC
            LIMIT 1
        "};

        sqlx::query_as(query)
            .bind(address.as_ref())
            .bind(hash.as_ref())
            .fetch_optional(&*self.pool)
            .await
    }

    #[trace(properties = { "address": "{address}", "hash": "{hash}" })]
    async fn contract_action_exists_by_address_as_of_block_hash(
        &self,
        address: &SerializedContractAddress,
        hash: BlockHash,
    ) -> Result<bool, sqlx::Error> {
        // Existence-only variant of the "as of" lookup above: avoids fetching a row at all when
        // only presence matters.
        let query = indoc! {"
            SELECT 1
            FROM contract_actions
            INNER JOIN transactions ON transactions.id = transaction_id
            INNER JOIN blocks ON blocks.id = transactions.block_id
            WHERE address = $1
            AND blocks.height <= (SELECT height FROM blocks WHERE hash = $2)
            LIMIT 1
        "};

        sqlx::query(query)
            .bind(address.as_ref())
            .bind(hash.as_ref())
            .fetch_optional(&*self.pool)
            .await
            .map(|row| row.is_some())
    }

    #[trace(properties = { "address": "{address}", "block_height": "{block_height}" })]
    async fn get_contract_action_by_address_as_of_block_height(
        &self,
        address: &SerializedContractAddress,
        block_height: u32,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        // A height above the tip bounds nothing out, so it resolves to the tip.
        let query = indoc! {"
            SELECT
                contract_actions.id,
                address,
                COALESCE(
                    (
                        SELECT t.state_key
                        FROM contract_action_translations t
                        JOIN blocks b ON b.id = t.block_id
                        WHERE t.contract_action_id = contract_actions.id
                        AND b.height <= $2
                        ORDER BY b.height DESC
                        LIMIT 1
                    ),
                    state_key
                ) AS state_key,
                attributes,
                zswap_state_key,
                transaction_id,
                (
                    SELECT b.height
                    FROM contract_action_translations t
                    JOIN blocks b ON b.id = t.block_id
                    WHERE t.contract_action_id = contract_actions.id
                    AND b.height <= $2
                    ORDER BY b.height DESC
                    LIMIT 1
                ) AS translated_at
            FROM contract_actions
            INNER JOIN transactions ON transactions.id = transaction_id
            INNER JOIN blocks ON blocks.id = transactions.block_id
            WHERE address = $1
            AND blocks.height <= $2
            ORDER BY contract_actions.id DESC
            LIMIT 1
        "};

        sqlx::query_as(query)
            .bind(address)
            .bind(block_height as i64)
            .fetch_optional(&*self.pool)
            .await
    }

    #[trace(properties = { "address": "{address}", "limit": "{limit}", "variant": "{variant:?}" })]
    async fn get_recent_contract_actions_by_address(
        &self,
        address: &SerializedContractAddress,
        limit: u32,
        variant: Option<&str>,
    ) -> Result<Vec<ContractAction>, sqlx::Error> {
        let mut query_builder = sqlx::QueryBuilder::new(indoc! {"
            SELECT
                contract_actions.id,
                address,
                state_key,
                attributes,
                zswap_state_key,
                transaction_id
            FROM contract_actions
            WHERE address =
        "});
        query_builder.push_bind(address.as_ref());

        if let Some(variant) = variant {
            // The variant column is a Postgres enum (cast to text to compare) and a SQLite TEXT.
            #[cfg(feature = "cloud")]
            query_builder
                .push(" AND variant::text = ")
                .push_bind(variant);
            #[cfg(feature = "standalone")]
            query_builder.push(" AND variant = ").push_bind(variant);
        }

        query_builder
            .push(" ORDER BY contract_actions.id DESC LIMIT ")
            .push_bind(limit as i64);

        query_builder
            .build_query_as::<ContractAction>()
            .fetch_all(&*self.pool)
            .await
    }

    #[trace(properties = { "address": "{address}", "hash": "{hash}" })]
    async fn get_contract_action_by_address_and_transaction_hash(
        &self,
        address: &SerializedContractAddress,
        hash: TransactionHash,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                contract_actions.id,
                address,
                state_key,
                attributes,
                zswap_state_key,
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

        sqlx::query_as(query)
            .bind(address.as_ref())
            .bind(hash.as_ref())
            .fetch_optional(&*self.pool)
            .await
    }

    #[trace(properties = { "address": "{address}", "identifier": "{identifier}" })]
    async fn get_contract_action_by_address_and_transaction_identifier(
        &self,
        address: &SerializedContractAddress,
        identifier: &SerializedTransactionIdentifier,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        #[cfg(feature = "cloud")]
        let query = indoc! {"
            SELECT
                contract_actions.id,
                address,
                state_key,
                attributes,
                zswap_state_key,
                contract_actions.transaction_id
            FROM contract_actions
            INNER JOIN regular_transactions ON regular_transactions.id = contract_actions.transaction_id
            WHERE address = $1
            AND $2 = ANY(regular_transactions.identifiers)
            ORDER BY contract_actions.id DESC
            LIMIT 1
        "};

        #[cfg(feature = "standalone")]
        let query = indoc! {"
            SELECT
                contract_actions.id,
                address,
                state_key,
                attributes,
                zswap_state_key,
                contract_actions.transaction_id
            FROM contract_actions
            INNER JOIN regular_transactions ON regular_transactions.id = contract_actions.transaction_id
            WHERE address = $1
            AND EXISTS (
                SELECT 1
                FROM transaction_identifiers
                WHERE transaction_identifiers.transaction_id = regular_transactions.id
                AND transaction_identifiers.identifier = $2
            )
            ORDER BY contract_actions.id DESC
            LIMIT 1
        "};

        sqlx::query_as(query)
            .bind(address)
            .bind(identifier)
            .fetch_optional(&*self.pool)
            .await
    }

    #[trace(properties = { "id": "{id}" })]
    async fn get_contract_actions_by_transaction_id(
        &self,
        id: u64,
    ) -> Result<Vec<ContractAction>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                id,
                address,
                state_key,
                attributes,
                zswap_state_key,
                transaction_id
            FROM contract_actions
            WHERE transaction_id = $1
            ORDER BY id
        "};

        sqlx::query_as(query)
            .bind(id as i64)
            .fetch_all(&*self.pool)
            .await
    }

    #[trace(properties = { "ids": "{ids:?}" })]
    async fn get_contract_actions_by_transaction_ids(
        &self,
        ids: &[u64],
    ) -> Result<Vec<ContractAction>, sqlx::Error> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        self.fetch_contract_actions_by_transaction_ids(ids).await
    }

    fn get_contract_actions_by_address(
        &self,
        address: &SerializedContractAddress,
        mut contract_action_id: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<ContractActionAtBlock, sqlx::Error>> + Send {
        let chunks = try_stream! {
            loop {
                let actions = self
                    .get_contract_actions_by_address(address, contract_action_id, batch_size)
                    .await?;

                match actions.last() {
                    Some(action) => contract_action_id = action.action.id + 1,
                    None => break,
                }

                yield actions;
            }
        };

        flatten_chunks(chunks)
    }

    #[trace(properties = { "address": "{address}" })]
    async fn get_contract_state_translations_by_address(
        &self,
        address: &SerializedContractAddress,
    ) -> Result<Vec<ContractAction>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                contract_actions.id,
                address,
                t.state_key AS state_key,
                attributes,
                zswap_state_key,
                transaction_id,
                b.height AS translated_at
            FROM contract_action_translations t
            INNER JOIN contract_actions ON contract_actions.id = t.contract_action_id
            INNER JOIN blocks b ON b.id = t.block_id
            WHERE address = $1
            ORDER BY b.height, contract_actions.id
        "};

        sqlx::query_as(query)
            .bind(address)
            .fetch_all(&*self.pool)
            .await
    }

    #[trace(properties = {
        "address": "{address}",
        "after_height": "{after_height}",
        "through_height": "{through_height}"
    })]
    async fn get_contract_state_translations_between(
        &self,
        address: &SerializedContractAddress,
        after_height: u32,
        through_height: u32,
    ) -> Result<Vec<ContractAction>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                contract_actions.id,
                address,
                t.state_key AS state_key,
                attributes,
                zswap_state_key,
                transaction_id,
                b.height AS translated_at
            FROM blocks b
            INNER JOIN contract_action_translations t ON t.block_id = b.id
            INNER JOIN contract_actions ON contract_actions.id = t.contract_action_id
            WHERE b.height > $2
            AND b.height <= $3
            AND address = $1
            ORDER BY b.height, contract_actions.id
        "};

        sqlx::query_as(query)
            .bind(address)
            .bind(after_height as i64)
            .bind(through_height as i64)
            .fetch_all(&*self.pool)
            .await
    }

    #[trace(properties = { "contract_action_id": "{contract_action_id}" })]
    async fn get_unshielded_balances_by_contract_action_id(
        &self,
        contract_action_id: u64,
    ) -> Result<Vec<crate::domain::ContractBalance>, sqlx::Error> {
        let query = indoc! {"
            SELECT token_type, amount
            FROM contract_balances
            WHERE contract_action_id = $1
        "};

        sqlx::query_as(query)
            .bind(contract_action_id as i64)
            .fetch_all(&*self.pool)
            .await
    }

    #[trace(properties = { "block_height": "{block_height}" })]
    async fn get_contract_action_id_by_block_height(
        &self,
        block_height: u32,
    ) -> Result<u64, sqlx::Error> {
        // The cursor a `contractActions` stream starts at: it then reads actions with `id >=` it.
        // - No action from the height on, e.g. a stream starting at a tip without actions: one past
        //   the highest ID, so nothing is replayed and the next action indexed is still delivered.
        // - No actions at all: `0`.
        // One statement, so `max(id)` comes from the same snapshot as the lookup; split in two, an
        // action indexed in between would fall below the cursor and be skipped.
        let query = indoc! {"
            SELECT COALESCE(
                (
                    SELECT contract_actions.id
                    FROM contract_actions
                    JOIN transactions ON transactions.id = transaction_id
                    JOIN blocks ON blocks.id = transactions.block_id
                    WHERE blocks.height >= $1
                    ORDER BY contract_actions.id
                    LIMIT 1
                ),
                (SELECT max(id) + 1 FROM contract_actions),
                0
            )
        "};

        let id = sqlx::query_scalar::<_, i64>(query)
            .bind(block_height as i64)
            .fetch_one(&*self.pool)
            .await?;

        Ok(id as u64)
    }
}

impl Storage {
    #[trace(properties = {
        "address": "{address}",
        "contract_action_id": "{contract_action_id}",
        "batch_size": "{batch_size}"
    })]
    async fn get_contract_actions_by_address(
        &self,
        address: &SerializedContractAddress,
        contract_action_id: u64,
        batch_size: NonZeroU32,
    ) -> Result<Vec<ContractActionAtBlock>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                contract_actions.id,
                address,
                state_key,
                attributes,
                zswap_state_key,
                transaction_id,
                blocks.height AS block_height
            FROM contract_actions
            INNER JOIN transactions ON transactions.id = transaction_id
            INNER JOIN blocks ON blocks.id = transactions.block_id
            WHERE address = $1
            AND contract_actions.id >= $2
            ORDER BY contract_actions.id
            LIMIT $3
        "};

        sqlx::query_as(query)
            .bind(address)
            .bind(contract_action_id as i64)
            .bind(batch_size.get() as i64)
            .fetch(&*self.pool)
            .try_collect::<Vec<_>>()
            .await
    }

    #[cfg(feature = "cloud")]
    #[trace(properties = { "ids": "{ids:?}" })]
    async fn fetch_contract_actions_by_transaction_ids(
        &self,
        ids: &[u64],
    ) -> Result<Vec<ContractAction>, sqlx::Error> {
        let ids = ids.iter().map(|id| *id as i64).collect::<Vec<_>>();

        let query = indoc! {"
            SELECT
                id,
                address,
                state_key,
                attributes,
                zswap_state_key,
                transaction_id
            FROM contract_actions
            WHERE transaction_id = ANY($1)
            ORDER BY id
        "};

        sqlx::query_as(query).bind(ids).fetch_all(&*self.pool).await
    }

    #[cfg(feature = "standalone")]
    #[trace(properties = { "ids": "{ids:?}" })]
    async fn fetch_contract_actions_by_transaction_ids(
        &self,
        ids: &[u64],
    ) -> Result<Vec<ContractAction>, sqlx::Error> {
        use sqlx::{QueryBuilder, Sqlite};

        let mut qb = QueryBuilder::<Sqlite>::new("WITH transaction_ids(id) AS (VALUES (");
        let mut sep = qb.separated("), (");
        for id in ids {
            sep.push_bind(*id as i64);
        }
        qb.push(indoc! {"
            ))
            SELECT
                id,
                address,
                state_key,
                attributes,
                zswap_state_key,
                transaction_id
            FROM contract_actions
            WHERE transaction_id IN (SELECT id FROM transaction_ids)
            ORDER BY id
        "});

        qb.build_query_as().fetch_all(&*self.pool).await
    }
}

#[cfg(all(test, feature = "standalone"))]
mod tests {
    use super::*;
    use chacha20poly1305::{ChaCha20Poly1305, KeyInit};
    use indexer_common::{
        domain::SerializedContractStateKey,
        infra::{
            migrations,
            pool::sqlite::{self, SqlitePool},
        },
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TRANSACTION_SEED: AtomicUsize = AtomicUsize::new(0);

    /// A migrated SQLite database in a temporary directory, removed on drop, and a `Storage` over
    /// it.
    async fn storage() -> (tempfile::TempDir, SqlitePool, Storage) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let url = dir.path().join("indexer.sqlite").display().to_string();
        let pool = SqlitePool::new(sqlite::Config::with_url(url))
            .await
            .expect("create pool");
        migrations::sqlite::run(&pool)
            .await
            .expect("run migrations");
        let storage = Storage::new(ChaCha20Poly1305::new(&[0; 32].into()), pool.clone());

        (dir, pool, storage)
    }

    /// A block hash derived from the height.
    fn block_hash(height: u32) -> Vec<u8> {
        let mut hash = [0; 32];
        hash[..4].copy_from_slice(&height.to_be_bytes());
        hash.to_vec()
    }

    async fn insert_block(pool: &SqlitePool, height: u32, protocol_version: u32) -> i64 {
        sqlx::query_scalar(indoc! {"
            INSERT INTO blocks (
                hash,
                height,
                protocol_version,
                parent_hash,
                timestamp,
                zswap_merkle_tree_root,
                ledger_parameters,
                ledger_state_key
            )
            VALUES ($1, $2, $3, $4, 0, X'00', X'00', X'00')
            RETURNING id
        "})
        .bind(block_hash(height))
        .bind(height as i64)
        .bind(protocol_version as i64)
        .bind(block_hash(height.wrapping_sub(1)))
        .fetch_one(&**pool)
        .await
        .expect("insert block")
    }

    async fn insert_transaction(pool: &SqlitePool, block_id: i64) -> i64 {
        sqlx::query_scalar(indoc! {"
            INSERT INTO transactions (block_id, variant, hash, protocol_version, raw)
            VALUES ($1, 'Regular', $2, 0, X'00')
            RETURNING id
        "})
        .bind(block_id)
        .bind(block_hash(
            TRANSACTION_SEED.fetch_add(1, Ordering::Relaxed) as u32
        ))
        .fetch_one(&**pool)
        .await
        .expect("insert transaction")
    }

    async fn insert_contract_action(
        pool: &SqlitePool,
        transaction_id: i64,
        address: &[u8],
        state_key: Option<&[u8]>,
    ) -> i64 {
        sqlx::query_scalar(indoc! {"
            INSERT INTO contract_actions (
                transaction_id,
                variant,
                address,
                attributes,
                state_key,
                zswap_state_key
            )
            VALUES ($1, 'Deploy', $2, '\"Deploy\"', $3, NULL)
            RETURNING id
        "})
        .bind(transaction_id)
        .bind(address)
        .bind(state_key)
        .fetch_one(&**pool)
        .await
        .expect("insert contract action")
    }

    /// With no action at or after the height, the start id is one past the highest; `0` on an empty
    /// table.
    #[tokio::test]
    async fn start_id_is_first_action_at_or_after_height_else_past_every_action() {
        let (_dir, pool, storage) = storage().await;
        assert_eq!(start_id(&storage, 0).await, 0, "empty table");

        insert_block(&pool, 0, 1_000_000).await;
        let block_1 = insert_block(&pool, 1, 1_000_000).await;
        insert_block(&pool, 2, 1_000_000).await;
        insert_block(&pool, 3, 1_000_000).await;
        let transaction = insert_transaction(&pool, block_1).await;
        let first = insert_contract_action(&pool, transaction, &[1; 32], None).await as u64;
        let last = insert_contract_action(&pool, transaction, &[2; 32], None).await as u64;

        assert_eq!(start_id(&storage, 0).await, first);
        assert_eq!(start_id(&storage, 1).await, first);
        assert_eq!(
            start_id(&storage, 2).await,
            last + 1,
            "no action at or after height 2"
        );
        assert_eq!(
            start_id(&storage, 3).await,
            last + 1,
            "the tip holds no action"
        );
        assert_eq!(start_id(&storage, 100).await, last + 1, "above the tip");
    }

    /// Block heights are read for the given transactions only, each with its transaction id.
    #[tokio::test]
    async fn block_heights_by_transaction_ids() {
        use crate::domain::storage::transaction::TransactionStorage;

        let (_dir, pool, storage) = storage().await;
        let block_7 = insert_block(&pool, 7, 1_000_000).await;
        let block_9 = insert_block(&pool, 9, 1_000_000).await;
        let a = insert_transaction(&pool, block_7).await as u64;
        let b = insert_transaction(&pool, block_9).await as u64;
        insert_transaction(&pool, block_9).await;

        let mut heights = storage
            .get_block_heights_by_transaction_ids(&[a, b, 999])
            .await
            .expect("get block heights");
        heights.sort();
        assert_eq!(heights, [(a, 7), (b, 9)]);

        assert!(
            storage
                .get_block_heights_by_transaction_ids(&[])
                .await
                .expect("get block heights")
                .is_empty()
        );
    }

    async fn start_id(storage: &Storage, height: u32) -> u64 {
        storage
            .get_contract_action_id_by_block_height(height)
            .await
            .expect("get start id")
    }

    async fn insert_translation(pool: &SqlitePool, action_id: i64, block_id: i64, key: &[u8]) {
        sqlx::query(indoc! {"
            INSERT INTO contract_action_translations (contract_action_id, block_id, state_key)
            VALUES ($1, $2, $3)
        "})
        .bind(action_id)
        .bind(block_id)
        .bind(key)
        .execute(&**pool)
        .await
        .expect("insert translation");
    }

    /// Contract A, deployed at block 100 with key `k6` and translated at block 500 to `k8`; blocks
    /// 499 and 600 exist.
    struct Fork {
        _dir: tempfile::TempDir,
        pool: SqlitePool,
        storage: Storage,
        address: Vec<u8>,
        deploy: u64,
    }

    impl Fork {
        async fn before_the_call() -> Self {
            let (dir, pool, storage) = storage().await;
            let address = vec![0xa; 32];
            let block_100 = insert_block(&pool, 100, 1_000_000).await;
            insert_block(&pool, 499, 1_000_000).await;
            let fork_block = insert_block(&pool, 500, 2_000_000).await;
            insert_block(&pool, 600, 2_000_000).await;
            let deploy_tx = insert_transaction(&pool, block_100).await;
            let deploy = insert_contract_action(&pool, deploy_tx, &address, Some(b"k6")).await;
            insert_translation(&pool, deploy, fork_block, b"k8").await;

            Self {
                _dir: dir,
                pool,
                storage,
                address,
                deploy: deploy as u64,
            }
        }

        /// Adds a call at block 700 with key `k8b`; returns its id.
        async fn with_the_call(&self) -> u64 {
            let block_700 = insert_block(&self.pool, 700, 2_000_000).await;
            let call_tx = insert_transaction(&self.pool, block_700).await;
            insert_contract_action(&self.pool, call_tx, &self.address, Some(b"k8b")).await as u64
        }

        fn address(&self) -> SerializedContractAddress {
            self.address.clone().into()
        }

        async fn as_of(&self, height: u32) -> ContractAction {
            self.storage
                .get_contract_action_by_address_as_of_block_height(&self.address(), height)
                .await
                .expect("as of height")
                .expect("the contract has an action by then")
        }

        async fn as_of_hash(&self, height: u32) -> ContractAction {
            self.storage
                .get_contract_action_by_address_as_of_block_hash(
                    &self.address(),
                    BlockHash::try_from(block_hash(height).as_slice()).expect("32 bytes"),
                )
                .await
                .expect("as of hash")
                .expect("the contract has an action by then")
        }
    }

    fn key(bytes: &[u8]) -> Option<SerializedContractStateKey> {
        Some(bytes.to_vec().into())
    }

    fn view(action: &ContractAction) -> (u64, Option<SerializedContractStateKey>, Option<u32>) {
        (action.id, action.state_key.clone(), action.translated_at)
    }

    /// The latest view serves the translation until a newer action exists.
    #[tokio::test]
    async fn latest_view_serves_the_translation_until_a_newer_action_exists() {
        let fork = Fork::before_the_call().await;
        let latest = || async {
            fork.storage
                .get_latest_contract_action_by_address(&fork.address())
                .await
                .expect("latest")
                .expect("the contract has an action")
        };

        assert_eq!(view(&latest().await), (fork.deploy, key(b"k8"), Some(500)));

        let call = fork.with_the_call().await;
        assert_eq!(view(&latest().await), (call, key(b"k8b"), None));
    }

    /// As-of views apply the translation from the fork block on; a height above the tip is the tip.
    #[tokio::test]
    async fn as_of_views_apply_the_translation_from_the_fork_block_on() {
        let fork = Fork::before_the_call().await;

        assert_eq!(
            view(&fork.as_of(499).await),
            (fork.deploy, key(b"k6"), None)
        );
        assert_eq!(
            view(&fork.as_of(500).await),
            (fork.deploy, key(b"k8"), Some(500))
        );
        assert_eq!(
            view(&fork.as_of(600).await),
            (fork.deploy, key(b"k8"), Some(500))
        );
        assert_eq!(
            view(&fork.as_of(10_000).await),
            (fork.deploy, key(b"k8"), Some(500))
        );

        let call = fork.with_the_call().await;
        assert_eq!(view(&fork.as_of(10_000).await), (call, key(b"k8b"), None));
        assert_eq!(
            view(&fork.as_of(650).await),
            (fork.deploy, key(b"k8"), Some(500))
        );

        assert!(
            fork.storage
                .get_contract_action_by_address_as_of_block_height(&fork.address(), 99)
                .await
                .expect("as of height")
                .is_none(),
            "before the deploy there is no contract"
        );
    }

    /// The by-hash view resolves the hash to a height and bounds the same way.
    #[tokio::test]
    async fn as_of_block_hash_view_is_bounded_by_that_block() {
        let fork = Fork::before_the_call().await;

        assert_eq!(
            view(&fork.as_of_hash(499).await),
            (fork.deploy, key(b"k6"), None)
        );
        assert_eq!(
            view(&fork.as_of_hash(600).await),
            (fork.deploy, key(b"k8"), Some(500))
        );
    }

    /// Translations read as their actions with the translated key and height.
    #[tokio::test]
    async fn translations_read_as_reemittable_rows() {
        let fork = Fork::before_the_call().await;
        fork.with_the_call().await;

        let rows = fork
            .storage
            .get_contract_state_translations_by_address(&fork.address())
            .await
            .expect("by address");
        assert_eq!(rows.len(), 1);
        assert_eq!(view(&rows[0]), (fork.deploy, key(b"k8"), Some(500)));

        let between = async |after, through| {
            fork.storage
                .get_contract_state_translations_between(&fork.address(), after, through)
                .await
                .expect("between")
        };
        assert_eq!(between(499, 500).await, rows, "the fork block alone");
        assert_eq!(
            between(498, 501).await,
            rows,
            "a range spanning a lost fork block"
        );
        assert!(
            between(500, 501).await.is_empty(),
            "the lower bound is exclusive"
        );
        assert!(
            between(400, 499).await.is_empty(),
            "a range ending before the fork"
        );
        assert!(
            fork.storage
                .get_contract_state_translations_by_address(&vec![0xb; 32].into())
                .await
                .expect("by address")
                .is_empty(),
            "another address has none"
        );
    }

    /// Stream rows carry their own keys and block heights.
    #[tokio::test]
    async fn stream_rows_are_records_with_their_block_height() {
        let fork = Fork::before_the_call().await;
        fork.with_the_call().await;

        let rows = fork
            .storage
            .get_contract_actions_by_address(&fork.address(), 0, NonZeroU32::new(10).unwrap())
            .await
            .expect("stream rows");
        let summary = rows
            .iter()
            .map(|row| {
                (
                    row.block_height,
                    row.action.state_key.clone(),
                    row.action.translated_at,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            summary,
            vec![(100, key(b"k6"), None), (700, key(b"k8b"), None)]
        );
    }
}
