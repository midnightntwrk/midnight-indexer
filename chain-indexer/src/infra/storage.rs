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
    self, Block, BlockTransactions, ContractAction, RegularTransaction, SystemTransaction,
    Transaction, TransactionVariant, node::BlockInfo,
};
use fastrace::trace;
use futures::{TryFutureExt, TryStreamExt};
use indexer_common::{
    domain::{
        BlockHash, ByteArray, ByteVec,
        dust::{
            DustCommitment, DustEvent, DustEventDetails, DustEventType, DustGenerationInfo,
            DustNullifier, QualifiedDustOutput,
        },
        ledger::{
            ContractAttributes, ContractBalance, SystemTransaction as DomainSystemTransaction,
            UnshieldedUtxo,
        },
    },
    infra::sqlx::U128BeBytes,
};
use indoc::indoc;
use midnight_ledger_v6::structure::{
    CNightGeneratesDustActionType, CNightGeneratesDustEvent,
    SystemTransaction as LedgerSystemTransaction,
};
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
    ) -> Result<Option<u64>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let max_transaction_id = save_block(block, transactions, &mut tx).await?;
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
    // Deserialize the system transaction to extract additional metadata.
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

    // Handle the V6 variant to access the inner ledger transaction.
    match system_tx {
        DomainSystemTransaction::V6(ledger_tx) => {
            match ledger_tx {
                LedgerSystemTransaction::CNightGeneratesDustUpdate { events } => {
                    // Process and save DUST events.
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

                _ => {
                    // Other system transactions (OverwriteParameters, DistributeNight,
                    // PayBlockRewardsToTreasury, PayFromTreasuryShielded, PayFromTreasuryUnshielded)
                    // have their effects already captured through ledger state application.
                    // No additional storage needed.
                }
            }
        }
    }

    Ok(())
}

/// Convert CNightGeneratesDust events to DUST events for storage.
fn convert_cnight_events_to_dust_events(
    events: &[CNightGeneratesDustEvent],
    transaction_hash: &indexer_common::domain::ledger::TransactionHash,
) -> Vec<DustEvent> {
    events
        .iter()
        .enumerate()
        .map(|(index, event)| {
            let event_details = match event.action {
                CNightGeneratesDustActionType::Create => {
                    DustEventDetails::DustInitialUtxo {
                        output: QualifiedDustOutput {
                            initial_value: event.value,
                            owner: event.owner.0.0.to_bytes_le().into(),
                            nonce: event.nonce.0.0.into(),
                            seq: 0,
                            ctime: event.time.to_secs(),
                            backing_night: event.nonce.0.0.into(),
                            mt_index: 0,
                        },
                        generation_info: DustGenerationInfo {
                            night_utxo_hash: ByteArray::default(),
                            value: event.value,
                            owner: event.owner.0.0.to_bytes_le().into(),
                            nonce: event.nonce.0.0.into(),
                            ctime: event.time.to_secs(),
                            dtime: 0,
                        },
                        generation_index: 0,
                    }
                }
                CNightGeneratesDustActionType::Destroy => {
                    DustEventDetails::DustSpendProcessed {
                        commitment: DustCommitment::default(),
                        commitment_index: 0,
                        nullifier: DustNullifier::default(),
                        v_fee: 0,
                        time: event.time.to_secs(),
                        params: indexer_common::domain::dust::DustParameters::default(),
                    }
                }
            };

            DustEvent {
                transaction_hash: *transaction_hash,
                logical_segment: index as u16,
                physical_segment: index as u16,
                event_details,
            }
        })
        .collect()
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
            } => generation_dtime_and_index = Some((generation_info.dtime, *generation_index)),

            DustEventDetails::DustSpendProcessed {
                commitment,
                nullifier,
                ..
            } => {
                mark_dust_utxo_spent(*commitment, *nullifier, transaction_id, tx).await?;
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

    // TODO: Implement proper cryptographic commitment calculation once the new ledger
    // and node versions with DUST features are available. The commitment algorithm
    // must match the node's implementation to ensure consistency across the system.
    // For now, using a deterministic SHA256 placeholder based on all DUST UTXO fields
    // to enable tracking functionality for PM-16218.
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(output.owner.as_ref());
    hasher.update(output.nonce.as_ref());
    hasher.update(output.initial_value.to_be_bytes());
    hasher.update(output.seq.to_be_bytes());
    hasher.update(output.ctime.to_be_bytes());
    hasher.update(output.backing_night.as_ref());
    hasher.update(output.mt_index.to_be_bytes());
    let commitment_bytes: [u8; 32] = hasher.finalize().into();
    let commitment = DustCommitment::from(commitment_bytes);

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

// TODO: Uncomment this function when node team implements registration events.
// Registration events will be emitted by the node when it detects Cardano stake
// key registrations/deregistrations. See PM-17951 for node integration work.
//
// #[trace]
// async fn save_cnight_registration(
//     cardano_stake_key: &str,
//     dust_address: DustOwner,
//     is_registration: bool,
//     tx: &mut SqlxTransaction,
// ) -> Result<(), sqlx::Error> {
//     // TODO: Implement cNIGHT registration tracking once the new ledger and node
//     // versions with DUST features are available. The registration event format
//     // and validation logic will be determined by the node implementation.
//     // For now, this is a placeholder to handle registration events when they
//     // are eventually emitted by the node.
//
//     if is_registration {
//         // Handle registration: insert new registration or update existing one
//         let query = indoc! {"
//             INSERT INTO cnight_registrations (
//                 cardano_address,
//                 dust_address,
//                 is_valid,
//                 registered_at
//             )
//             VALUES ($1, $2, $3, $4)
//             ON CONFLICT (cardano_address, dust_address)
//             DO UPDATE SET
//                 is_valid = $3,
//                 registered_at = $4,
//                 removed_at = NULL
//         "};
//
//         let dust_address_bytes = dust_address.as_ref();
//
//         // TODO: Use proper timestamp from the event when available.
//         let current_time = std::time::SystemTime::now()
//             .duration_since(std::time::UNIX_EPOCH)
//             .unwrap()
//             .as_secs() as i64;
//
//         sqlx::query(query)
//             .bind(cardano_stake_key.as_bytes())
//             .bind(dust_address_bytes)
//             .bind(true)
//             .bind(current_time)
//             .execute(&mut **tx)
//             .await?;
//     } else {
//         // Handle deregistration: mark as invalid
//         let query = indoc! {"
//             UPDATE cnight_registrations
//             SET is_valid = false, removed_at = $1
//             WHERE cardano_address = $2 AND dust_address = $3 AND is_valid = true
//         "};
//
//         let dust_address_bytes = dust_address.as_ref();
//
//         // TODO: Use proper timestamp from the event when available.
//         let current_time = std::time::SystemTime::now()
//             .duration_since(std::time::UNIX_EPOCH)
//             .unwrap()
//             .as_secs() as i64;
//
//         sqlx::query(query)
//             .bind(current_time)
//             .bind(cardano_stake_key.as_bytes())
//             .bind(dust_address_bytes)
//             .execute(&mut **tx)
//             .await?;
//     }
//
//     Ok(())
// }

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
            DustEventDetails::DustInitialUtxo {
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
            DustEventDetails::DustSpendProcessed { time, v_fee, .. } => {
                assert_eq!(*time, 1234567900);
                assert_eq!(*v_fee, 0); // No fee for system-initiated destroy.
            }
            _ => panic!("Expected DustSpendProcessed event"),
        }
    }
}
