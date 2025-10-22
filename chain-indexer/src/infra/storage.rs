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

use crate::domain::{
    self, Block, BlockTransactions, ContractAction, DustRegistrationEvent, RegularTransaction,
    SystemTransaction, Transaction, node::BlockInfo,
};
use fastrace::trace;
use futures::{TryFutureExt, TryStreamExt};
use indexer_common::{
    domain::{
        BlockHash, ByteVec, ContractAttributes, ContractBalance, LedgerEvent,
        LedgerEventAttributes, TransactionVariant, UnshieldedUtxo,
    },
    infra::sqlx::U128BeBytes,
};
use indoc::indoc;
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Type, types::Json};
use std::iter;

#[cfg(feature = "cloud")]
/// Sqlx transaction for Postgres.
type SqlxTransaction = sqlx::Transaction<'static, sqlx::Postgres>;

#[cfg(feature = "standalone")]
/// Sqlx transaction for Sqlite.
type SqlxTransaction = sqlx::Transaction<'static, sqlx::Sqlite>;

/// Unified storage implementation for PostgreSQL (cloud) and SQLite (standalone). Uses Cargo
/// features to select the appropriate database backend at build time.
#[derive(Debug, Clone)]
pub struct Storage {
    #[cfg(feature = "cloud")]
    pool: indexer_common::infra::pool::postgres::PostgresPool,

    #[cfg(feature = "standalone")]
    pool: indexer_common::infra::pool::sqlite::SqlitePool,
}

impl Storage {
    #[cfg(feature = "cloud")]
    pub fn new(pool: indexer_common::infra::pool::postgres::PostgresPool) -> Self {
        Self { pool }
    }

    #[cfg(feature = "standalone")]
    pub fn new(pool: indexer_common::infra::pool::sqlite::SqlitePool) -> Self {
        Self { pool }
    }
}

impl domain::storage::Storage for Storage {
    #[trace]
    async fn get_highest_block_info(&self) -> Result<Option<BlockInfo>, sqlx::Error> {
        let query = indoc! {"
            SELECT hash, height
            FROM blocks
            ORDER BY height DESC
            LIMIT 1
        "};

        sqlx::query_as::<_, (ByteVec, i64)>(query)
            .fetch_optional(&*self.pool)
            .await?
            .map(|(hash, height)| {
                let hash = BlockHash::try_from(hash.as_ref())
                    .map_err(|error| sqlx::Error::Decode(error.into()))?;

                Ok(BlockInfo {
                    hash,
                    height: height as u32,
                })
            })
            .transpose()
    }

    #[trace]
    async fn get_transaction_count(&self) -> Result<u64, sqlx::Error> {
        let query = indoc! {"
            SELECT count(*)
            FROM transactions
        "};

        let (count,) = sqlx::query_as::<_, (i64,)>(query)
            .fetch_one(&*self.pool)
            .await?;

        Ok(count as u64)
    }

    #[trace]
    async fn get_contract_action_count(&self) -> Result<(u64, u64, u64), sqlx::Error> {
        let query = indoc! {"
            SELECT count(*)
            FROM contract_actions
            WHERE variant = $1
        "};

        let (deploy_count,) = sqlx::query_as::<_, (i64,)>(query)
            .bind(ContractActionVariant::Deploy)
            .fetch_one(&*self.pool)
            .await?;
        let (call_count,) = sqlx::query_as::<_, (i64,)>(query)
            .bind(ContractActionVariant::Call)
            .fetch_one(&*self.pool)
            .await?;
        let (update_count,) = sqlx::query_as::<_, (i64,)>(query)
            .bind(ContractActionVariant::Update)
            .fetch_one(&*self.pool)
            .await?;

        Ok((deploy_count as u64, call_count as u64, update_count as u64))
    }

    #[trace(properties = { "block_height": "{block_height}" })]
    async fn get_block_transactions(
        &self,
        block_height: u32,
    ) -> Result<BlockTransactions, sqlx::Error> {
        let query = indoc! {"
            SELECT
                id,
                protocol_version,
                parent_hash,
                timestamp
            FROM blocks
            WHERE height = $1
        "};

        let (block_id, protocol_version, block_parent_hash, block_timestamp) =
            sqlx::query_as::<_, (i64, i64, ByteVec, i64)>(query)
                .bind(block_height as i64)
                .fetch_one(&*self.pool)
                .await?;
        let block_parent_hash = BlockHash::try_from(block_parent_hash.as_ref())
            .map_err(|error| sqlx::Error::Decode(error.into()))?;

        let query = indoc! {"
            SELECT
                variant,
                raw
            FROM transactions
            WHERE block_id = $1
        "};

        let transactions = sqlx::query_as::<_, (TransactionVariant, ByteVec)>(query)
            .bind(block_id)
            .fetch(&*self.pool)
            .try_collect::<Vec<_>>()
            .await?;

        Ok(BlockTransactions {
            transactions,
            protocol_version: (protocol_version as u32).into(),
            block_parent_hash,
            block_timestamp: block_timestamp as u64,
        })
    }

    #[trace]
    async fn save_block(
        &self,
        block: &Block,
        transactions: &[Transaction],
        dust_registration_events: &[DustRegistrationEvent],
    ) -> Result<Option<u64>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let max_transaction_id =
            save_block(block, transactions, dust_registration_events, &mut tx).await?;
        tx.commit().await?;

        Ok(max_transaction_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "cloud", sqlx(type_name = "CONTRACT_ACTION_VARIANT"))]
enum ContractActionVariant {
    Deploy,
    Call,
    Update,
}

impl From<&ContractAttributes> for ContractActionVariant {
    fn from(attributes: &ContractAttributes) -> Self {
        match attributes {
            ContractAttributes::Deploy => Self::Deploy,
            ContractAttributes::Call { .. } => Self::Call,
            ContractAttributes::Update => Self::Update,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "cloud", sqlx(type_name = "LEDGER_EVENT_VARIANT"))]
pub enum LedgerEventVariant {
    ZswapInput,
    ZswapOutput,
    ParamChange,
    DustInitialUtxo,
    DustGenerationDtimeUpdate,
    DustSpendProcessed,
}

impl From<&LedgerEventAttributes> for LedgerEventVariant {
    fn from(attributes: &LedgerEventAttributes) -> Self {
        match attributes {
            LedgerEventAttributes::ZswapInput => Self::ZswapInput,
            LedgerEventAttributes::ZswapOutput => Self::ZswapOutput,
            LedgerEventAttributes::ParamChange => Self::ParamChange,
            LedgerEventAttributes::DustInitialUtxo { .. } => Self::DustInitialUtxo,
            LedgerEventAttributes::DustGenerationDtimeUpdate { .. } => {
                Self::DustGenerationDtimeUpdate
            }
            LedgerEventAttributes::DustSpendProcessed => Self::DustSpendProcessed,
        }
    }
}

#[trace]
async fn save_block(
    block: &Block,
    transactions: &[Transaction],
    dust_registration_events: &[DustRegistrationEvent],
    tx: &mut SqlxTransaction,
) -> Result<Option<u64>, sqlx::Error> {
    let query = indoc! {"
        INSERT INTO blocks (
            hash,
            height,
            protocol_version,
            parent_hash,
            author,
            timestamp,
            ledger_parameters
        )
    "};

    let block_id = QueryBuilder::new(query)
        .push_values([block], |mut q, block| {
            let Block {
                hash,
                height,
                protocol_version,
                parent_hash,
                author,
                timestamp,
                ledger_parameters,
                ..
            } = block;

            q.push_bind(hash.as_ref())
                .push_bind(*height as i64)
                .push_bind(protocol_version.0 as i64)
                .push_bind(parent_hash.as_ref())
                .push_bind(author.as_ref().map(|a| a.as_ref()))
                .push_bind(*timestamp as i64)
                .push_bind(ledger_parameters.as_ref());
        })
        .push(" RETURNING id")
        .build_query_as::<(i64,)>()
        .fetch_one(&mut **tx)
        .map_ok(|(id,)| id)
        .await?;

    let max_transaction_id = save_transactions(transactions, block_id, tx).await?;
    save_dust_registration_events(dust_registration_events, block_id, block.timestamp, tx).await?;
    Ok(max_transaction_id)
}

#[trace(properties = { "block_id": "{block_id}" })]
async fn save_transactions(
    transactions: &[Transaction],
    block_id: i64,
    tx: &mut SqlxTransaction,
) -> Result<Option<u64>, sqlx::Error> {
    let mut highest_transaction_id = None;

    for transaction in transactions {
        let query = indoc! {"
            INSERT INTO transactions (
                block_id,
                variant,
                hash,
                protocol_version,
                raw
            )
        "};

        let hash = transaction.hash();
        let transaction_id = QueryBuilder::new(query)
            .push_values(iter::once(transaction), |mut q, transaction| {
                q.push_bind(block_id)
                    .push_bind(transaction.variant())
                    .push_bind(hash.as_ref())
                    .push_bind(transaction.protocol_version().0 as i64)
                    .push_bind(transaction.raw());
            })
            .push(" RETURNING id")
            .build_query_as::<(i64,)>()
            .fetch_one(&mut **tx)
            .map_ok(|(id,)| id)
            .await?;

        match transaction {
            Transaction::Regular(transaction) => {
                highest_transaction_id = Some(
                    save_regular_transaction(transaction, transaction_id, block_id, tx).await?,
                );
            }

            Transaction::System(transaction) => {
                save_system_transaction(transaction, transaction_id, tx).await?
            }
        }
    }

    Ok(highest_transaction_id)
}

#[trace(properties = { "block_id": "{block_id}" })]
async fn save_regular_transaction(
    transaction: &RegularTransaction,
    transaction_id: i64,
    block_id: i64,
    tx: &mut SqlxTransaction,
) -> Result<u64, sqlx::Error> {
    #[cfg(feature = "cloud")]
    let query = indoc! {"
        INSERT INTO regular_transactions (
            id,
            transaction_result,
            merkle_tree_root,
            start_index,
            end_index,
            paid_fees,
            estimated_fees,
            identifiers
        )
    "};
    #[cfg(feature = "standalone")]
    let query = indoc! {"
        INSERT INTO regular_transactions (
            id,
            transaction_result,
            merkle_tree_root,
            start_index,
            end_index,
            paid_fees,
            estimated_fees
        )
    "};

    let transaction_id = QueryBuilder::new(query)
        .push_values(iter::once(transaction), |mut q, transaction| {
            q.push_bind(transaction_id)
                .push_bind(Json(&transaction.transaction_result))
                .push_bind(&transaction.merkle_tree_root)
                .push_bind(transaction.start_index as i64)
                .push_bind(transaction.end_index as i64)
                .push_bind(U128BeBytes::from(transaction.paid_fees))
                .push_bind(U128BeBytes::from(transaction.estimated_fees));
            #[cfg(feature = "cloud")]
            q.push_bind(&transaction.identifiers);
        })
        .push(" RETURNING id")
        .build_query_as::<(i64,)>()
        .fetch_one(&mut **tx)
        .map_ok(|(id,)| id)
        .await?;

    #[cfg(feature = "standalone")]
    save_identifiers(&transaction.identifiers, transaction_id, tx).await?;

    save_contract_actions(&transaction.contract_actions, transaction_id, tx).await?;

    save_unshielded_utxos(
        &transaction.created_unshielded_utxos,
        transaction_id,
        false,
        tx,
    )
    .await?;
    save_unshielded_utxos(
        &transaction.spent_unshielded_utxos,
        transaction_id,
        true,
        tx,
    )
    .await?;

    save_ledger_events(&transaction.ledger_events, transaction_id, tx).await?;

    Ok(transaction_id as u64)
}

#[trace]
async fn save_system_transaction(
    transaction: &SystemTransaction,
    transaction_id: i64,
    tx: &mut SqlxTransaction,
) -> Result<(), sqlx::Error> {
    save_unshielded_utxos(
        &transaction.created_unshielded_utxos,
        transaction_id,
        false,
        tx,
    )
    .await?;

    save_ledger_events(&transaction.ledger_events, transaction_id, tx).await
}

#[trace(properties = { "transaction_id": "{transaction_id}" })]
async fn save_contract_actions(
    contract_actions: &[ContractAction],
    transaction_id: i64,
    tx: &mut SqlxTransaction,
) -> Result<(), sqlx::Error> {
    if contract_actions.is_empty() {
        return Ok(());
    }

    let query = indoc! {"
        INSERT INTO contract_actions (
            transaction_id,
            variant,
            address,
            state,
            zswap_state,
            attributes
        )
    "};

    let contract_action_ids = QueryBuilder::new(query)
        .push_values(contract_actions.iter(), |mut q, action| {
            q.push_bind(transaction_id)
                .push_bind(ContractActionVariant::from(&action.attributes))
                .push_bind(&action.address)
                .push_bind(&action.state)
                .push_bind(&action.zswap_state)
                .push_bind(Json(&action.attributes));
        })
        .push(" RETURNING id")
        .build_query_as::<(i64,)>()
        .fetch_all(&mut **tx)
        .await?
        .into_iter()
        .map(|(id,)| id);

    let contract_balances = contract_actions
        .iter()
        .zip(contract_action_ids)
        .flat_map(|(action, action_id)| {
            action
                .extracted_balances
                .iter()
                .map(move |&balance| (action_id, balance))
        })
        .collect::<Vec<_>>();
    save_contract_balances(&contract_balances, tx).await?;

    Ok(())
}

#[trace(properties = { "transaction_id": "{transaction_id}" })]
async fn save_unshielded_utxos(
    utxos: &[UnshieldedUtxo],
    transaction_id: i64,
    spent: bool,
    tx: &mut SqlxTransaction,
) -> Result<(), sqlx::Error> {
    if utxos.is_empty() {
        return Ok(());
    }

    if spent {
        for &utxo in utxos {
            let query = indoc! {"
                INSERT INTO unshielded_utxos (
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
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                ON CONFLICT (intent_hash, output_index)
                DO UPDATE SET spending_transaction_id = $2
                WHERE unshielded_utxos.spending_transaction_id IS NULL
            "};

            let UnshieldedUtxo {
                owner,
                token_type,
                value,
                intent_hash,
                output_index,
                ctime,
                initial_nonce,
                registered_for_dust_generation,
            } = utxo;

            sqlx::query(query)
                .bind(transaction_id)
                .bind(transaction_id)
                .bind(owner.as_ref())
                .bind(token_type.as_ref())
                .bind(U128BeBytes::from(value))
                .bind(intent_hash.as_ref())
                .bind(output_index as i64)
                .bind(ctime.map(|n| n as i64))
                .bind(initial_nonce.as_ref())
                .bind(registered_for_dust_generation)
                .execute(&mut **tx)
                .await?;
        }
    } else {
        let query_base = indoc! {"
            INSERT INTO unshielded_utxos (
                creating_transaction_id,
                owner,
                token_type,
                value,
                intent_hash,
                output_index,
                ctime,
                initial_nonce,
                registered_for_dust_generation
            )
        "};

        QueryBuilder::new(query_base)
            .push_values(utxos.iter(), |mut q, utxo| {
                let UnshieldedUtxo {
                    owner,
                    token_type,
                    value,
                    intent_hash,
                    output_index,
                    ctime,
                    initial_nonce,
                    registered_for_dust_generation,
                } = utxo;

                q.push_bind(transaction_id)
                    .push_bind(owner.as_ref())
                    .push_bind(token_type.as_ref())
                    .push_bind(U128BeBytes::from(value))
                    .push_bind(intent_hash.as_ref())
                    .push_bind(*output_index as i64)
                    .push_bind(ctime.map(|n| n as i64))
                    .push_bind(initial_nonce.as_ref())
                    .push_bind(registered_for_dust_generation);
            })
            .build()
            .execute(&mut **tx)
            .await?;
    }

    Ok(())
}

#[trace(properties = { "transaction_id": "{transaction_id}" })]
async fn save_ledger_events(
    ledger_events: &[LedgerEvent],
    transaction_id: i64,
    tx: &mut SqlxTransaction,
) -> Result<(), sqlx::Error> {
    if ledger_events.is_empty() {
        return Ok(());
    }

    let query = indoc! {"
        INSERT INTO ledger_events (
            transaction_id,
            variant,
            grouping,
            raw,
            attributes
        )
    "};

    QueryBuilder::new(query)
        .push_values(ledger_events.iter(), |mut q, ledger_event| {
            q.push_bind(transaction_id)
                .push_bind(LedgerEventVariant::from(&ledger_event.attributes))
                .push_bind(ledger_event.grouping)
                .push_bind(ledger_event.raw.as_ref())
                .push_bind(Json(&ledger_event.attributes));
        })
        .build()
        .execute(&mut **tx)
        .await?;

    Ok(())
}

#[trace]
async fn save_contract_balances(
    balances: &[(i64, ContractBalance)],
    tx: &mut SqlxTransaction,
) -> Result<(), sqlx::Error> {
    if balances.is_empty() {
        return Ok(());
    }

    let query = indoc! {"
        INSERT INTO contract_balances (
            contract_action_id,
            token_type,
            amount
        )
    "};

    QueryBuilder::new(query)
        .push_values(balances.iter(), |mut q, (action_id, balance)| {
            q.push_bind(*action_id)
                .push_bind(balance.token_type.as_ref())
                .push_bind(U128BeBytes::from(balance.amount));
        })
        .build()
        .execute(&mut **tx)
        .await?;

    Ok(())
}

#[cfg(feature = "standalone")]
async fn save_identifiers(
    identifiers: &[indexer_common::domain::SerializedTransactionIdentifier],
    transaction_id: i64,
    tx: &mut SqlxTransaction,
) -> Result<(), sqlx::Error> {
    if identifiers.is_empty() {
        return Ok(());
    }

    let query = indoc! {"
        INSERT INTO transaction_identifiers (
            transaction_id,
            identifier
        )
    "};

    QueryBuilder::new(query)
        .push_values(identifiers.iter(), |mut q, identifier| {
            q.push_bind(transaction_id).push_bind(identifier);
        })
        .build()
        .execute(&mut **tx)
        .await?;

    Ok(())
}

#[trace]
async fn save_dust_registration_events(
    events: &[DustRegistrationEvent],
    block_id: i64,
    block_timestamp: u64,
    tx: &mut SqlxTransaction,
) -> Result<(), sqlx::Error> {
    for event in events {
        match event {
            DustRegistrationEvent::Registration {
                cardano_address,
                dust_address,
            } => {
                let query = indoc! {"
                    INSERT INTO cnight_registrations (
                        cardano_address,
                        dust_address,
                        valid,
                        registered_at,
                        block_id
                    ) VALUES ($1, $2, $3, $4, $5)
                    ON CONFLICT (cardano_address, dust_address)
                    DO UPDATE SET
                        valid = EXCLUDED.valid,
                        registered_at = EXCLUDED.registered_at,
                        removed_at = NULL,
                        block_id = EXCLUDED.block_id
                "};

                sqlx::query(query)
                    .bind(cardano_address.as_ref())
                    .bind(dust_address.as_ref())
                    .bind(true)
                    .bind(block_timestamp as i64)
                    .bind(block_id)
                    .execute(&mut **tx)
                    .await?;
            }

            DustRegistrationEvent::Deregistration {
                cardano_address,
                dust_address,
            } => {
                let query = indoc! {"
                    UPDATE cnight_registrations
                    SET valid = $1,
                        removed_at = $2,
                        block_id = $3
                    WHERE cardano_address = $4
                    AND dust_address = $5
                "};

                sqlx::query(query)
                    .bind(false)
                    .bind(block_timestamp as i64)
                    .bind(block_id)
                    .bind(cardano_address.as_ref())
                    .bind(dust_address.as_ref())
                    .execute(&mut **tx)
                    .await?;
            }

            // MappingAdded and MappingRemoved are not part of this minimal cherry-pick.
            _ => {}
        }
    }

    Ok(())
}
