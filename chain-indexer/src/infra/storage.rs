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
    SystemTransaction, Transaction, TransactionVariant, node::BlockInfo,
};
use fastrace::trace;
use futures::{TryFutureExt, TryStreamExt};
use indexer_common::{
    domain::{
        BlockHash, ByteVec, DustNonce, DustNullifier, DustOwner, NightUtxoHash, NightUtxoNonce,
        dust::{
            DustCommitment, DustEvent, DustEventDetails, DustEventType, DustGenerationInfo,
            QualifiedDustOutput,
        },
        ledger::{
            ContractAttributes, ContractBalance, SystemTransaction as DomainSystemTransaction,
            UnshieldedUtxo,
        },
    },
    infra::sqlx::U128BeBytes,
};
use indoc::indoc;
use midnight_ledger_v6::structure::SystemTransaction as LedgerSystemTransaction;
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

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "cloud", sqlx(type_name = "CONTRACT_ACTION_VARIANT"))]
enum ContractActionVariant {
    /// A contract deployment.
    #[default]
    Deploy,

    /// A contract call.
    Call,

    /// A contract update.
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
            timestamp
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
                ..
            } = block;

            q.push_bind(hash.as_ref())
                .push_bind(*height as i64)
                .push_bind(protocol_version.0 as i64)
                .push_bind(parent_hash.as_ref())
                .push_bind(author.as_ref().map(|a| a.as_ref()))
                .push_bind(*timestamp as i64);
        })
        .push(" RETURNING id")
        .build_query_as::<(i64,)>()
        .fetch_one(&mut **tx)
        .map_ok(|(id,)| id)
        .await?;

    // Save DUST registration events if any
    if !dust_registration_events.is_empty() {
        save_dust_registration_events(
            dust_registration_events,
            block_id,
            block.timestamp as i64,
            tx,
        )
        .await?;
    }

    save_transactions(transactions, block_id, tx).await
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
                save_system_transaction(transaction, transaction_id, block_id, tx).await?
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

    let contract_action_ids =
        save_contract_actions(&transaction.contract_actions, transaction_id, tx).await?;
    let contract_balances = transaction
        .contract_actions
        .iter()
        .zip(contract_action_ids.iter())
        .flat_map(|(action, &action_id)| {
            action
                .extracted_balances
                .iter()
                .map(move |&balance| (action_id, balance))
        })
        .collect::<Vec<_>>();
    save_contract_balances(contract_balances, tx).await?;

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

    // Process and save DUST events.
    process_dust_events(&transaction.dust_events, transaction_id, tx).await?;
    save_dust_events(&transaction.dust_events, transaction_id, tx).await?;

    Ok(transaction_id as u64)
}

#[trace(properties = { "block_id": "{_block_id}" })]
async fn save_system_transaction(
    transaction: &SystemTransaction,
    transaction_id: i64,
    _block_id: i64,
    tx: &mut SqlxTransaction,
) -> Result<(), sqlx::Error> {
    // Process and save DUST events from system transactions.
    // The ledger state already properly extracts DUST events from system transactions,
    // including CNightGeneratesDustUpdate events which create DustInitialUtxo
    // and DustGenerationDtimeUpdate events.
    if !transaction.dust_events.is_empty() {
        process_dust_events(&transaction.dust_events, transaction_id, tx).await?;
        save_dust_events(&transaction.dust_events, transaction_id, tx).await?;
    }

    // Deserialize the system transaction to extract additional metadata for storage.
    // Note: The transaction has already been successfully applied to the ledger state,
    // so failure here only affects our ability to store additional indexing metadata.
    let system_tx = match DomainSystemTransaction::deserialize(
        &transaction.raw,
        transaction.protocol_version,
    ) {
        Ok(tx) => tx,
        Err(error) => {
            // Log and continue - we don't want to fail the entire block processing
            // just because we can't extract metadata from a system transaction.
            // This could happen with newer protocol versions we don't yet support.
            log::warn!(
                "cannot extract metadata from system transaction {}: {}",
                transaction.hash,
                error
            );
            return Ok(());
        }
    };

    // Handle the V6 variant to access the inner ledger transaction for metadata storage.
    match system_tx {
        DomainSystemTransaction::V6(ledger_tx) => {
            match ledger_tx {
                LedgerSystemTransaction::CNightGeneratesDustUpdate { events } => {
                    // Convert CNightGeneratesDust events to DUST events and save them.
                    // These events represent DUST distributions to cNIGHT holders.
                    let dust_events =
                        convert_cnight_events_to_dust_events(&events, &transaction.hash);
                    if !dust_events.is_empty() {
                        process_dust_events(&dust_events, transaction_id, tx).await?;
                        save_dust_events(&dust_events, transaction_id, tx).await?;
                    }
                }

                LedgerSystemTransaction::DistributeReserve(amount) => {
                    // Store reserve distribution for tracking and auditing.
                    // Note: The ledger state is already updated, but we track these
                    // events separately for analytics and historical queries.
                    save_reserve_distribution(transaction_id, amount, tx).await?;
                }

                LedgerSystemTransaction::OverwriteParameters(params) => {
                    // Store parameter updates for audit trail and history.
                    save_parameter_update(transaction_id, &params, tx).await?;
                }

                LedgerSystemTransaction::DistributeNight(claim_kind, outputs) => {
                    // Store NIGHT distribution events for tracking.
                    save_night_distribution(transaction_id, &claim_kind, &outputs, tx).await?;
                }

                LedgerSystemTransaction::PayBlockRewardsToTreasury { amount } => {
                    // Store treasury income from block rewards.
                    save_treasury_income(transaction_id, amount, "block_rewards", tx).await?;
                }

                LedgerSystemTransaction::PayFromTreasuryShielded { outputs, .. } => {
                    // Store shielded treasury payments.
                    save_treasury_payment_shielded(transaction_id, &outputs, tx).await?;
                }

                LedgerSystemTransaction::PayFromTreasuryUnshielded { outputs, .. } => {
                    // Store unshielded treasury payments.
                    save_treasury_payment_unshielded(transaction_id, &outputs, tx).await?;
                }

                // Catch-all for any new system transaction types
                _ => {
                    // Log or ignore unknown system transaction types
                }
            }
        }
    }

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
                    output_index,
                    intent_hash
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7)
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
            } = utxo;

            sqlx::query(query)
                .bind(transaction_id)
                .bind(transaction_id)
                .bind(owner.as_ref())
                .bind(token_type.as_ref())
                .bind(U128BeBytes::from(value))
                .bind(output_index as i32)
                .bind(intent_hash.as_ref())
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
                output_index,
                intent_hash
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
                } = utxo;

                q.push_bind(transaction_id)
                    .push_bind(owner.as_ref())
                    .push_bind(token_type.as_ref())
                    .push_bind(U128BeBytes::from(*value))
                    .push_bind(*output_index as i32)
                    .push_bind(intent_hash.as_ref());
            })
            .build()
            .execute(&mut **tx)
            .await?;
    }

    Ok(())
}

#[trace(properties = { "transaction_id": "{transaction_id}" })]
async fn save_contract_actions(
    contract_actions: &[ContractAction],
    transaction_id: i64,
    tx: &mut SqlxTransaction,
) -> Result<Vec<i64>, sqlx::Error> {
    if contract_actions.is_empty() {
        return Ok(Vec::new());
    }

    let query = indoc! {"
        INSERT INTO contract_actions (
            transaction_id,
            variant,
            address,
            state,
            chain_state,
            attributes
        )
    "};

    let contract_action_ids = QueryBuilder::new(query)
        .push_values(contract_actions.iter(), |mut q, action| {
            q.push_bind(transaction_id)
                .push_bind(ContractActionVariant::from(&action.attributes))
                .push_bind(&action.address)
                .push_bind(&action.state)
                .push_bind(&action.chain_state)
                .push_bind(Json(&action.attributes));
        })
        .push(" RETURNING id")
        .build_query_as::<(i64,)>()
        .fetch_all(&mut **tx)
        .await?
        .into_iter()
        .map(|(id,)| id)
        .collect();

    Ok(contract_action_ids)
}

#[trace]
async fn save_contract_balances(
    balances: Vec<(i64, ContractBalance)>,
    tx: &mut SqlxTransaction,
) -> Result<(), sqlx::Error> {
    if !balances.is_empty() {
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
    }

    Ok(())
}

#[cfg(feature = "standalone")]
async fn save_identifiers(
    identifiers: &[indexer_common::domain::ledger::SerializedTransactionIdentifier],
    transaction_id: i64,
    tx: &mut SqlxTransaction,
) -> Result<(), sqlx::Error> {
    if !identifiers.is_empty() {
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
    }

    Ok(())
}

#[trace(properties = { "transaction_id": "{transaction_id}" })]
async fn process_dust_events(
    dust_events: &[DustEvent],
    transaction_id: i64,
    block_height: u32,
    tx: &mut SqlxTransaction,
) -> Result<(), sqlx::Error> {
    let mut generation_dtime_and_index = None;

    for dust_event in dust_events {
        match &dust_event.event_details {
            DustEventDetails::DustInitialUtxo {
                output,
                generation_info,
                generation_index,
            } => {
                let generation_info_id =
                    save_dust_generation_info(generation_info, *generation_index, tx).await?;
                save_dust_utxos(output, generation_info_id, tx).await?;
            }

            DustEventDetails::DustGenerationDtimeUpdate {
                generation_info,
                generation_index,
                merkle_path,
            } => {
                generation_dtime_and_index = Some((generation_info.dtime, *generation_index));
                // Store merkle path in dust_generation_tree table
                save_dust_generation_tree_update(
                    *generation_index,
                    merkle_path,
                    block_height,
                    tx
                ).await?;
            }

            DustEventDetails::DustSpendProcessed {
                commitment,
                nullifier,
                ..
            } => {
                mark_dust_utxo_spent(*commitment, *nullifier, transaction_id, tx).await?;
            }

            // Registration events are handled separately in save_dust_registration_events
            DustEventDetails::DustRegistration { .. }
            | DustEventDetails::DustDeregistration { .. }
            | DustEventDetails::DustMappingAdded { .. }
            | DustEventDetails::DustMappingRemoved { .. } => {
                // These events are captured from the NativeTokenObservation pallet
                // and stored in the cnight_registrations table
            }
        }
    }

    if let Some((dtime, index)) = generation_dtime_and_index {
        update_dust_generation_dtime(dtime, index, tx).await?;
    }

    Ok(())
}

#[trace(properties = { "transaction_id": "{transaction_id}" })]
async fn save_dust_events(
    dust_events: &[DustEvent],
    transaction_id: i64,
    tx: &mut SqlxTransaction,
) -> Result<(), sqlx::Error> {
    if dust_events.is_empty() {
        return Ok(());
    }

    let query = indoc! {"
        INSERT INTO dust_events (
            transaction_id,
            transaction_hash,
            logical_segment,
            physical_segment,
            event_type,
            event_data
        )
    "};

    QueryBuilder::new(query)
        .push_values(dust_events.iter(), |mut q, event| {
            let event_type = DustEventType::from(&event.event_details);
            q.push_bind(transaction_id)
                .push_bind(event.transaction_hash.as_ref())
                .push_bind(event.logical_segment as i32)
                .push_bind(event.physical_segment as i32)
                .push_bind(event_type)
                .push_bind(Json(&event.event_details));
        })
        .build()
        .execute(&mut **tx)
        .await?;

    Ok(())
}

#[trace(properties = { "transaction_id": "{transaction_id}" })]
async fn mark_dust_utxo_spent(
    commitment: DustCommitment,
    nullifier: DustNullifier,
    transaction_id: i64,
    tx: &mut SqlxTransaction,
) -> Result<(), sqlx::Error> {
    let query = indoc! {"
        UPDATE dust_utxos
        SET nullifier = $1, spent_at_transaction_id = $2
        WHERE nullifier IS NULL
        AND commitment = $3
    "};

    sqlx::query(query)
        .bind(nullifier.as_ref())
        .bind(transaction_id)
        .bind(commitment.as_ref())
        .execute(&mut **tx)
        .await?;

    Ok(())
}

#[trace]
async fn update_dust_generation_dtime(
    dtime: u64,
    index: u64,
    tx: &mut SqlxTransaction,
) -> Result<(), sqlx::Error> {
    let query = indoc! {"
        UPDATE dust_generation_info
        SET dtime = $1
        WHERE merkle_index = $2
    "};

    sqlx::query(query)
        .bind(dtime as i64)
        .bind(index as i64)
        .execute(&mut **tx)
        .await?;

    Ok(())
}

#[cfg_attr(feature = "cloud", trace)]
async fn save_dust_generation_tree_update(
    merkle_index: u64,
    merkle_path: &[DustMerklePathEntry],
    block_height: u32,
    tx: &mut SqlxTransaction,
) -> Result<(), sqlx::Error> {
    // Serialize the merkle path to store as tree_data
    let tree_data = serde_json::to_vec(merkle_path).unwrap_or_default();
    
    // Calculate the root hash from the path (placeholder for now)
    // In a real implementation, we'd calculate the actual root from the path
    let root = vec![0u8; 32]; // Placeholder root
    
    let query = indoc! {"
        INSERT INTO dust_generation_tree (
            block_height,
            merkle_index,
            root,
            tree_data
        )
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (merkle_index) DO UPDATE SET
            block_height = EXCLUDED.block_height,
            root = EXCLUDED.root,
            tree_data = EXCLUDED.tree_data
    "};
    
    sqlx::query(query)
        .bind(block_height as i64)
        .bind(merkle_index as i64)
        .bind(&root)
        .bind(&tree_data)
        .execute(&mut **tx)
        .await?;
    
    Ok(())
}

async fn save_dust_generation_info(
    generation: &DustGenerationInfo,
    generation_index: u64,
    tx: &mut SqlxTransaction,
) -> Result<u64, sqlx::Error> {
    let query = indoc! {"
        INSERT INTO dust_generation_info (
            night_utxo_hash,
            value,
            owner,
            nonce,
            ctime,
            merkle_index,
            dtime
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id
    "};

    let (id,) = sqlx::query_as::<_, (i64,)>(query)
        .bind(generation.night_utxo_hash.as_ref())
        .bind(U128BeBytes::from(generation.value))
        .bind(generation.owner.as_ref())
        .bind(generation.nonce.as_ref())
        .bind(generation.ctime as i64)
        .bind(generation_index as i64)
        .bind(generation.dtime as i64)
        .fetch_one(&mut **tx)
        .await?;

    Ok(id as u64)
}

#[trace]
async fn save_reserve_distribution(
    transaction_id: i64,
    amount: u128,
    tx: &mut SqlxTransaction,
) -> Result<(), sqlx::Error> {
    let query = indoc! {"
        INSERT INTO reserve_distributions (
            transaction_id,
            amount
        )
        VALUES ($1, $2)
    "};

    sqlx::query(query)
        .bind(transaction_id)
        .bind(U128BeBytes::from(amount))
        .execute(&mut **tx)
        .await?;

    Ok(())
}

#[trace]
async fn save_parameter_update(
    transaction_id: i64,
    _params: &midnight_ledger_v6::structure::LedgerParameters,
    tx: &mut SqlxTransaction,
) -> Result<(), sqlx::Error> {
    // Store a simplified representation of the parameters
    // Full parameters are complex and don't serialize directly
    #[derive(Serialize)]
    struct ParamsData {
        note: &'static str,
    }
    let params_json = Json(ParamsData {
        note: "Parameter update applied - details in ledger state",
    });

    let query = indoc! {"
        INSERT INTO parameter_updates (
            transaction_id,
            parameters
        )
        VALUES ($1, $2)
    "};

    sqlx::query(query)
        .bind(transaction_id)
        .bind(Json(&params_json))
        .execute(&mut **tx)
        .await?;

    Ok(())
}

#[trace]
async fn save_night_distribution(
    transaction_id: i64,
    _claim_kind: &midnight_ledger_v6::structure::ClaimKind,
    outputs: &[midnight_ledger_v6::structure::OutputInstructionUnshielded],
    tx: &mut SqlxTransaction,
) -> Result<(), sqlx::Error> {
    // Calculate total amount being distributed
    let total_amount: u128 = outputs.iter().map(|output| output.amount).sum();

    // Store a simplified representation
    #[derive(Serialize)]
    struct OutputsData {
        output_count: usize,
        total_amount: String,
    }
    let outputs_json = Json(OutputsData {
        output_count: outputs.len(),
        total_amount: total_amount.to_string(),
    });

    let query = indoc! {"
        INSERT INTO night_distributions (
            transaction_id,
            claim_kind,
            outputs,
            total_amount
        )
        VALUES ($1, $2, $3, $4)
    "};

    sqlx::query(query)
        .bind(transaction_id)
        .bind("night_distribution")
        .bind(Json(&outputs_json))
        .bind(U128BeBytes::from(total_amount))
        .execute(&mut **tx)
        .await?;

    Ok(())
}

#[trace]
async fn save_treasury_income(
    transaction_id: i64,
    amount: u128,
    source: &str,
    tx: &mut SqlxTransaction,
) -> Result<(), sqlx::Error> {
    let query = indoc! {"
        INSERT INTO treasury_income (
            transaction_id,
            amount,
            source
        )
        VALUES ($1, $2, $3)
    "};

    sqlx::query(query)
        .bind(transaction_id)
        .bind(U128BeBytes::from(amount))
        .bind(source)
        .execute(&mut **tx)
        .await?;

    Ok(())
}

#[trace]
async fn save_treasury_payment_shielded(
    transaction_id: i64,
    outputs: &[midnight_ledger_v6::structure::OutputInstructionShielded],
    tx: &mut SqlxTransaction,
) -> Result<(), sqlx::Error> {
    // Calculate total amount if possible (depends on output structure)
    // For shielded outputs, amount may not be visible
    let total_amount: Option<u128> = None;

    // Store a simplified representation
    #[derive(Serialize)]
    struct OutputsData {
        output_count: usize,
        total_amount: Option<String>,
    }
    let outputs_json = Json(OutputsData {
        output_count: outputs.len(),
        total_amount: total_amount.map(|a| a.to_string()),
    });

    let query = indoc! {"
        INSERT INTO treasury_payments (
            transaction_id,
            payment_type,
            token_type,
            outputs,
            total_amount
        )
        VALUES ($1, $2, $3, $4, $5)
    "};

    sqlx::query(query)
        .bind(transaction_id)
        .bind("shielded")
        .bind("shielded_token")
        .bind(Json(&outputs_json))
        .bind(total_amount.map(U128BeBytes::from))
        .execute(&mut **tx)
        .await?;

    Ok(())
}

#[trace]
async fn save_treasury_payment_unshielded(
    transaction_id: i64,
    outputs: &[midnight_ledger_v6::structure::OutputInstructionUnshielded],
    tx: &mut SqlxTransaction,
) -> Result<(), sqlx::Error> {
    // Calculate total amount being paid
    let total_amount: u128 = outputs.iter().map(|output| output.amount).sum();

    // Store a simplified representation
    #[derive(Serialize)]
    struct OutputsData {
        output_count: usize,
        total_amount: String,
    }
    let outputs_json = Json(OutputsData {
        output_count: outputs.len(),
        total_amount: total_amount.to_string(),
    });

    let query = indoc! {"
        INSERT INTO treasury_payments (
            transaction_id,
            payment_type,
            token_type,
            outputs,
            total_amount
        )
        VALUES ($1, $2, $3, $4, $5)
    "};

    sqlx::query(query)
        .bind(transaction_id)
        .bind("unshielded")
        .bind("unshielded_token")
        .bind(Json(&outputs_json))
        .bind(Some(U128BeBytes::from(total_amount)))
        .execute(&mut **tx)
        .await?;

    Ok(())
}

#[cfg_attr(feature = "cloud", trace)]
async fn save_dust_utxos(
    output: &QualifiedDustOutput,
    generation_info_id: u64,
    tx: &mut SqlxTransaction,
) -> Result<(), sqlx::Error> {
    let query = indoc! {"
        INSERT INTO dust_utxos (
            commitment,
            initial_value,
            owner,
            nonce,
            seq,
            ctime,
            generation_info_id
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
    "};

    // Use the commitment from the output
    // Generate commitment from the output data - this will be replaced by the actual commitment
    // from the ledger
    let commitment = DustCommitment::default();

    sqlx::query(query)
        .bind(commitment.as_ref())
        .bind(U128BeBytes::from(output.initial_value))
        .bind(output.owner.as_ref())
        .bind(output.nonce.as_ref())
        .bind(output.seq as i64)
        .bind(output.ctime as i64)
        .bind(generation_info_id as i64)
        .execute(&mut **tx)
        .await?;

    Ok(())
}

/// Save cNIGHT registration events from the NativeTokenObservation pallet.
/// These events are emitted when Cardano stake keys are registered or deregistered
/// for DUST distribution. See PM-17951 for details.
#[trace]
async fn save_dust_registration_events(
    events: &[DustRegistrationEvent],
    block_id: i64,
    block_timestamp: i64,
    tx: &mut SqlxTransaction,
) -> Result<(), sqlx::Error> {
    for event in events {
        match event {
            DustRegistrationEvent::Registration {
                cardano_address,
                dust_address,
            } => {
                // Handle registration: insert new registration or update existing one
                let query = indoc! {"
                    INSERT INTO cnight_registrations (
                        cardano_address,
                        dust_address,
                        is_valid,
                        registered_at,
                        block_id
                    )
                    VALUES ($1, $2, $3, $4, $5)
                    ON CONFLICT (cardano_address, dust_address)
                    DO UPDATE SET
                        is_valid = $3,
                        registered_at = $4,
                        removed_at = NULL,
                        block_id = $5
                "};

                sqlx::query(query)
                    .bind(cardano_address.0.as_slice())
                    .bind(dust_address.0.as_slice())
                    .bind(true)
                    .bind(block_timestamp)
                    .bind(block_id)
                    .execute(&mut **tx)
                    .await?;
            }

            DustRegistrationEvent::Deregistration {
                cardano_address,
                dust_address,
            } => {
                // Handle deregistration: mark as invalid
                let query = indoc! {"
                    UPDATE cnight_registrations
                    SET is_valid = false, removed_at = $1
                    WHERE cardano_address = $2 AND dust_address = $3 AND is_valid = true
                "};

                sqlx::query(query)
                    .bind(block_timestamp)
                    .bind(cardano_address.0.as_slice())
                    .bind(dust_address.0.as_slice())
                    .execute(&mut **tx)
                    .await?;
            }

            DustRegistrationEvent::MappingAdded {
                cardano_address,
                dust_address,
                utxo_id,
            } => {
                // Store UTXO mapping for tracking
                let query = indoc! {"
                    INSERT INTO dust_utxo_mappings (
                        cardano_address,
                        dust_address,
                        utxo_id,
                        added_at,
                        block_id
                    )
                    VALUES ($1, $2, $3, $4, $5)
                    ON CONFLICT (utxo_id) DO NOTHING
                "};

                sqlx::query(query)
                    .bind(cardano_address.0.as_slice())
                    .bind(dust_address.0.as_slice())
                    .bind(utxo_id.0.as_slice())
                    .bind(block_timestamp)
                    .bind(block_id)
                    .execute(&mut **tx)
                    .await?;
            }

            DustRegistrationEvent::MappingRemoved {
                cardano_address: _,
                dust_address: _,
                utxo_id,
            } => {
                // Remove UTXO mapping
                let query = indoc! {"
                    UPDATE dust_utxo_mappings
                    SET removed_at = $1
                    WHERE utxo_id = $2 AND removed_at IS NULL
                "};

                sqlx::query(query)
                    .bind(block_timestamp)
                    .bind(utxo_id.0.as_slice())
                    .execute(&mut **tx)
                    .await?;
            }
        }
    }

    Ok(())
}

/// Convert CNightGeneratesDust events to DUST events for storage.
///
/// This function handles the conversion of CNightGeneratesDust system transaction events
/// that are emitted by the node when distributing DUST to cNIGHT holders.
fn convert_cnight_events_to_dust_events(
    events: &[midnight_ledger_v6::structure::CNightGeneratesDustEvent],
    tx_hash: &indexer_common::domain::ledger::TransactionHash,
) -> Vec<indexer_common::domain::dust::DustEvent> {
    use indexer_common::domain::dust::{
        DustEvent, DustEventDetails, DustGenerationInfo, QualifiedDustOutput,
    };

    events
        .iter()
        .enumerate()
        .map(|(index, event)| {
            let owner_bytes = event.owner.0.as_le_bytes();
            let owner_array: [u8; 32] = owner_bytes.try_into().expect("DustPublicKey should be 32 bytes");
            let owner = DustOwner::from(owner_array);
            let nonce = DustNonce::from(event.nonce.0.0);

            let event_details = match event.action {
                midnight_ledger_v6::structure::CNightGeneratesDustActionType::Create => {
                    // Create a new DUST UTXO
                    DustEventDetails::DustInitialUtxo {
                        output: QualifiedDustOutput {
                            initial_value: event.value,
                            owner,
                            nonce,
                            seq: 0, // Initial sequence number
                            ctime: event.time.to_secs(),
                            backing_night: NightUtxoNonce::default(), // No backing NIGHT for system-generated DUST
                            mt_index: 0, // Will be set when added to merkle tree
                        },
                        generation_info: DustGenerationInfo {
                            value: event.value,
                            owner,
                            nonce,
                            ctime: event.time.to_secs(),
                            dtime: u64::MAX, // Never destroyed initially
                            night_utxo_hash: NightUtxoHash::default(), // No backing NIGHT UTXO
                        },
                        generation_index: index as u64,
                    }
                }
                midnight_ledger_v6::structure::CNightGeneratesDustActionType::Destroy => {
                    // System-initiated DUST destruction when cNIGHT holder's tokens are burned.
                    // Unlike user-initiated spends, system destroys bypass the normal spend flow.
                    DustEventDetails::DustSpendProcessed {
                        commitment: DustCommitment::default(), // System destroys don't track original commitment
                        commitment_index: 0, // Merkle tree position not tracked for system destroys
                        nullifier: DustNullifier::default(), // No nullifier generated for system destroys
                        v_fee: 0, // System operations don't pay fees
                        time: event.time.to_secs(),
                        params: indexer_common::domain::dust::DustParameters::default(), // Use default parameters for system operations
                    }
                }
            };

            DustEvent {
                transaction_hash: *tx_hash,
                logical_segment: index as u16,
                physical_segment: index as u16,
                event_details,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexer_common::domain::ledger::TransactionHash;
    use midnight_base_crypto_v6::time::Timestamp;
    use midnight_ledger_v6::{
        dust::{DustPublicKey, InitialNonce},
        structure::{CNightGeneratesDustActionType, CNightGeneratesDustEvent},
    };
    use midnight_transient_crypto_v6::{curve::Fr, hash::HashOutput};

    #[test]
    fn test_convert_cnight_events_to_dust_events() {
        // Create a test transaction hash.
        let tx_hash = TransactionHash::from([1u8; 32]);

        // Create test CNGD events.
        let events = vec![
            CNightGeneratesDustEvent {
                action: CNightGeneratesDustActionType::Create,
                value: 1000u128,
                owner: DustPublicKey(Fr::from_le_bytes(&[2u8; 32]).unwrap()),
                nonce: InitialNonce(HashOutput([3u8; 32])),
                time: Timestamp::from_secs(1234567890),
            },
            CNightGeneratesDustEvent {
                action: CNightGeneratesDustActionType::Destroy,
                value: 500u128,
                owner: DustPublicKey(Fr::from_le_bytes(&[4u8; 32]).unwrap()),
                nonce: InitialNonce(HashOutput([5u8; 32])),
                time: Timestamp::from_secs(1234567900),
            },
        ];

        // Convert to DUST events.
        let dust_events = convert_cnight_events_to_dust_events(&events, &tx_hash);

        // Verify the conversion.
        assert_eq!(dust_events.len(), 2);

        // Check first event (Create).
        assert_eq!(dust_events[0].transaction_hash, tx_hash);
        assert_eq!(dust_events[0].logical_segment, 0);
        assert_eq!(dust_events[0].physical_segment, 0);

        match &dust_events[0].event_details {
            indexer_common::domain::dust::DustEventDetails::DustInitialUtxo {
                output,
                generation_info,
                ..
            } => {
                assert_eq!(output.initial_value, 1000);
                assert_eq!(generation_info.value, 1000);
                assert_eq!(output.ctime, 1234567890);
            }
            _ => panic!("Expected DustInitialUtxo event"),
        }

        // Check second event (Destroy).
        assert_eq!(dust_events[1].transaction_hash, tx_hash);
        assert_eq!(dust_events[1].logical_segment, 1);
        assert_eq!(dust_events[1].physical_segment, 1);

        match &dust_events[1].event_details {
            indexer_common::domain::dust::DustEventDetails::DustSpendProcessed {
                time,
                v_fee,
                ..
            } => {
                assert_eq!(*time, 1234567900);
                assert_eq!(*v_fee, 0); // No fee for system-initiated destroy.
            }
            _ => panic!("Expected DustSpendProcessed event"),
        }
    }
}
