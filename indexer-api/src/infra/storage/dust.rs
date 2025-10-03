// This file is part of midnight-indexer.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use sqlx::types::Json;

use crate::{
    domain::{
        dust::{
            AddressType, DustCommitmentEvent, DustCommitmentInfo, DustCommitmentMerkleUpdate,
            DustGenerationEvent, DustGenerationInfo, DustGenerationMerkleUpdate,
            DustGenerationStatus, DustMerkleTreeType, DustNullifierTransaction,
            DustNullifierTransactionEvent, DustSystemState, RegistrationAddress,
            RegistrationUpdate,
        },
        storage::dust::DustStorage,
    },
    infra::storage::Storage,
};
use async_stream::try_stream;
use fastrace::trace;
use futures::Stream;
use indexer_common::{
    domain::{
        CardanoStakeKey, DustAddress, DustCommitment, DustMerkleRoot, DustMerkleUpdate, DustNonce,
        DustNullifier, DustOwner, DustPrefix, NightUtxoHash,
        dust::{DustEvent, DustEventAttributes, DustEventVariant, DustMerklePathEntry},
        ledger::TransactionHash,
    },
    infra::sqlx::{SqlxOption, U128BeBytes},
};
use indoc::indoc;
use itertools::Itertools;
use sqlx::FromRow;
use std::num::NonZeroU32;

impl DustStorage for Storage {
    #[trace]
    async fn get_current_dust_state(&self) -> Result<DustSystemState, sqlx::Error> {
        // Get latest commitment tree root.
        let query = indoc! {"
            SELECT root
            FROM dust_commitment_tree
            ORDER BY block_height DESC
            LIMIT 1
        "};

        let commitment_tree_root = sqlx::query_as::<_, (DustMerkleRoot,)>(query)
            .fetch_optional(&*self.pool)
            .await?
            .map(|(x,)| x)
            .unwrap_or_default();

        // Get latest generation tree root.
        let query = indoc! {"
            SELECT root
            FROM dust_generation_tree
            ORDER BY block_height DESC
            LIMIT 1
        "};

        let generation_tree_root = sqlx::query_as::<_, (DustMerkleRoot,)>(query)
            .fetch_optional(&*self.pool)
            .await?
            .map(|(x,)| x)
            .unwrap_or_default();

        // Get latest block info.
        let block_query = indoc! {"
            SELECT height, timestamp
            FROM blocks
            ORDER BY height DESC
            LIMIT 1
        "};

        let (block_height, timestamp) = sqlx::query_as::<_, (i64, i64)>(block_query)
            .fetch_optional(&*self.pool)
            .await?
            .unwrap_or((0, 0));

        // Count active registrations.
        let registration_query = indoc! {"
            SELECT COUNT(*)
            FROM cnight_registrations
            WHERE is_valid = true
            AND removed_at IS NULL
        "};

        let (total_registrations,) = sqlx::query_as::<_, (i64,)>(registration_query)
            .fetch_one(&*self.pool)
            .await?;

        Ok(DustSystemState {
            commitment_tree_root,
            generation_tree_root,
            block_height: block_height as u32,
            timestamp: timestamp as u64,
            total_registrations: total_registrations as u32,
        })
    }

    #[trace]
    async fn get_dust_generation_status(
        &self,
        cardano_stake_keys: &[CardanoStakeKey],
    ) -> Result<Vec<DustGenerationStatus>, sqlx::Error> {
        let mut statuses = Vec::new();

        for stake_key in cardano_stake_keys {
            // Query registration info.
            let registration_query = indoc! {"
                SELECT dust_address, is_valid
                FROM cnight_registrations
                WHERE cardano_address = $1 AND removed_at IS NULL
                ORDER BY registered_at DESC
                LIMIT 1
            "};

            let (dust_address, is_registered) =
                sqlx::query_as::<_, (DustAddress, bool)>(registration_query)
                    .bind(stake_key.as_ref())
                    .fetch_optional(&*self.pool)
                    .await?
                    .unwrap_or_default();

            let mut generation_rate = 0u128;
            let mut current_capacity = 0u128;
            let mut night_balance = 0u128;

            // Query active generation info if registered.
            if is_registered {
                let generation_query = indoc! {"
                    SELECT value, ctime
                    FROM dust_generation_info
                    WHERE owner = $1 AND dtime IS NULL
                    ORDER BY ctime DESC
                    LIMIT 1
                "};

                let result = sqlx::query_as::<_, (U128BeBytes, i64)>(generation_query)
                    .bind(dust_address.as_ref())
                    .fetch_optional(&*self.pool)
                    .await?;

                if let Some((value, ctime)) = result {
                    let value_u128: u128 = value.into();
                    night_balance = value_u128;

                    // DUST generation rate calculation based on ledger spec:
                    // - generation_decay_rate = 8,267 Specks per Star per second
                    // - 1 Night = 10^6 Stars
                    // - Therefore: generation_rate = Stars * 8,267 Specks/second
                    const GENERATION_DECAY_RATE: u128 = 8_267;
                    generation_rate = value_u128.saturating_mul(GENERATION_DECAY_RATE);

                    // Calculate current capacity based on elapsed time since creation.
                    // Get current timestamp from latest block.
                    let current_time_query = indoc! {"
                        SELECT timestamp
                        FROM blocks
                        ORDER BY height DESC
                        LIMIT 1
                    "};

                    let current_timestamp = sqlx::query_as::<_, (i64,)>(current_time_query)
                        .fetch_optional(&*self.pool)
                        .await?
                        .map(|(t,)| t)
                        .unwrap_or(ctime);

                    // Calculate elapsed seconds since creation.
                    let elapsed_seconds = ((current_timestamp - ctime).max(0) as u128) / 1000; // Convert from ms to seconds.

                    // Current capacity = Stars * generation_decay_rate * elapsed_seconds
                    // Maximum capacity is limited by night_dust_ratio (5 DUST per NIGHT = 5 * 10^15
                    // Specks per 10^6 Stars)
                    const NIGHT_DUST_RATIO: u128 = 5_000_000_000; // Max Specks per Star
                    let max_capacity = value_u128.saturating_mul(NIGHT_DUST_RATIO);
                    let generated_capacity = value_u128
                        .saturating_mul(GENERATION_DECAY_RATE)
                        .saturating_mul(elapsed_seconds);
                    current_capacity = generated_capacity.min(max_capacity);
                }
            }

            statuses.push(DustGenerationStatus {
                cardano_stake_key: stake_key.clone(),
                dust_address: is_registered.then_some(dust_address),
                is_registered,
                generation_rate,
                current_capacity,
                night_balance,
            });
        }

        Ok(statuses)
    }

    #[trace]
    async fn get_dust_merkle_root(
        &self,
        tree_type: DustMerkleTreeType,
        timestamp: u64,
    ) -> Result<Option<DustMerkleRoot>, sqlx::Error> {
        let query = match tree_type {
            DustMerkleTreeType::Commitment => indoc! {"
                SELECT dc.root
                FROM dust_commitment_tree dc
                INNER JOIN blocks b ON b.height = dc.block_height
                WHERE b.timestamp <= $1
                ORDER BY dc.block_height DESC
                LIMIT 1
            "},

            DustMerkleTreeType::Generation => indoc! {"
                SELECT dg.root
                FROM dust_generation_tree dg
                INNER JOIN blocks b ON b.height = dg.block_height
                WHERE b.timestamp <= $1
                ORDER BY dg.block_height DESC
                LIMIT 1
            "},
        };

        let root = sqlx::query_as::<_, (DustMerkleRoot,)>(query)
            .bind(timestamp as i64)
            .fetch_optional(&*self.pool)
            .await?
            .map(|(x,)| x);

        Ok(root)
    }

    fn get_dust_generations(
        &self,
        dust_address: &DustAddress,
        from_generation_index: u64,
        // Used to resume from specific merkle tree position when streaming.
        from_merkle_index: u64,
        only_active: bool,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<DustGenerationEvent, sqlx::Error>> + Send {
        let batch_size = batch_size.get() as i64;
        let mut last_index = from_generation_index;
        let mut last_merkle_index = from_merkle_index;

        try_stream! {
            loop {
                // Query generation info.
                let query = if only_active {
                    indoc! {r#"
                        SELECT 
                            id, night_utxo_hash, value, owner, nonce, 
                            ctime, dtime, merkle_index
                        FROM dust_generation_info
                        WHERE owner = $1
                        AND merkle_index >= $2
                        AND dtime IS NULL
                        ORDER BY merkle_index
                        LIMIT $3
                    "#}
                } else {
                    indoc! {r#"
                        SELECT 
                            id, night_utxo_hash, value, owner, nonce, 
                            ctime, dtime, merkle_index
                        FROM dust_generation_info
                        WHERE owner = $1
                        AND merkle_index >= $2
                        ORDER BY merkle_index
                        LIMIT $3
                    "#}
                };

                let rows = sqlx::query_as::<_, DustGenerationInfoRow>(query)
                    .bind(dust_address.as_ref())
                    .bind(last_index as i64)
                    .bind(batch_size)
                    .fetch_all(&*self.pool)
                    .await?;

                if rows.is_empty() {
                    break;
                }

                for row in rows {
                    last_index = row.id + 1;
                    yield DustGenerationEvent::Info(row.into());
                }

                // Query merkle tree updates in the same range.
                let merkle_query = indoc! {"
                    SELECT merkle_index, root, block_height, tree_data
                    FROM dust_generation_tree
                    WHERE merkle_index >= $1
                    ORDER BY merkle_index
                    LIMIT $2
                "};

                let merkle_rows = sqlx::query_as::<_, DustGenerationTreeRow>(merkle_query)
                    .bind(last_merkle_index as i64)
                    .bind(batch_size)
                    .fetch_all(&*self.pool)
                    .await?;

                for row in merkle_rows {
                    last_merkle_index = row.merkle_index + 1;
                    // Extract merkle path from Json wrapper.
                    let merkle_path = (!row.tree_data.0.is_empty()).then_some(row.tree_data.0);

                    yield DustGenerationEvent::MerkleUpdate(DustGenerationMerkleUpdate {
                        index: row.merkle_index,
                        collapsed_update: row.root.clone(),
                        block_height: row.block_height,
                        merkle_path,
                    });
                }
            }
        }
    }

    #[trace]
    fn get_dust_nullifier_transactions(
        &self,
        prefixes: &[DustPrefix],
        min_prefix_length: u32,
        from_block: u32,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<DustNullifierTransactionEvent, sqlx::Error>> + Send {
        let batch_size = batch_size.get() as i64;
        let min_prefix_length = min_prefix_length as usize;
        let mut current_block = from_block as i64;

        try_stream! {
            loop {
                // Filter prefixes that meet minimum length requirement.
                let valid_prefixes = prefixes
                    .iter()
                    .filter(|prefix| prefix.as_ref().len() >= min_prefix_length)
                    .collect::<Vec<_>>();

                if valid_prefixes.is_empty() {
                    break;
                }

                // Build conditions with parameter placeholders.
                let conditions = valid_prefixes
                    .iter()
                    .enumerate()
                    .map(|(i, prefix)| {
                        let param_num = 3 + i;
                        let hex_len = prefix.as_ref().len() * 2;

                        #[cfg(feature = "cloud")]
                        {
                            format!(
                                "substring(encode(nullifier, 'hex'), 1, {hex_len}) = encode(${param_num}, 'hex')"
                            )
                        }

                        #[cfg(feature = "standalone")]
                        {
                            format!(
                                "substr(hex(nullifier), 1, {hex_len}) = hex(${param_num})"
                            )
                        }
                    })
                    .collect::<Vec<_>>();

                let where_clause = conditions.join(" OR ");

                // Use CTE with two-step query:.
                // 1. First SELECT finds distinct transactions matching our criteria.
                // 2. Second SELECT fetches all nullifiers for those transactions.
                let query = format!(
                    indoc! {"
                        WITH matched_transactions AS (
                            SELECT DISTINCT t.id, t.hash, b.height
                            FROM dust_utxos du
                            INNER JOIN transactions t ON t.id = du.spent_at_transaction_id
                            INNER JOIN blocks b ON b.id = t.block_id
                            WHERE du.nullifier IS NOT NULL
                            AND ({})
                            AND b.height >= $1
                            ORDER BY b.height
                            LIMIT $2
                        )
                        SELECT mt.hash, mt.height, du.nullifier
                        FROM matched_transactions mt
                        INNER JOIN dust_utxos du ON du.spent_at_transaction_id = mt.id
                        WHERE du.nullifier IS NOT NULL
                        ORDER BY mt.height, mt.hash
                    "},
                    where_clause
                );

                // Build query with parameter bindings.
                let mut transaction_query =
                    sqlx::query_as::<_, (TransactionHash, i64, DustNullifier)>(&query)
                        .bind(current_block)
                        .bind(batch_size);

                // Bind binary prefix parameters.
                for prefix in &valid_prefixes {
                    transaction_query = transaction_query.bind(prefix.as_ref());
                }

                let rows = transaction_query.fetch_all(&*self.pool).await?;

                if rows.is_empty() {
                    break;
                }

                // Group by transaction using itertools' chunk_by (not group_by which is deprecated).
                // in 0.14), then collect to avoid Send issues.
                let grouped = rows
                    .into_iter()
                    .chunk_by(|(hash, height, _)| (*hash, *height as u32))
                    .into_iter()
                    .map(|((hash, height), group)| {
                        let nullifiers = group.collect::<Vec<_>>();
                        ((hash, height), nullifiers)
                    })
                    .collect::<Vec<_>>();

                let mut max_height = current_block;
                for ((transaction_hash, block_height), nullifiers) in grouped {
                    max_height = max_height.max(block_height as i64);

                    // Check which prefixes match the nullifiers for this transaction.
                    let mut matching_prefixes = Vec::new();
                    for (_, _, nullifier) in nullifiers {
                        for prefix in &valid_prefixes {
                            if nullifier.as_ref().starts_with(prefix.as_ref()) {
                                matching_prefixes.push((*prefix).clone());
                            }
                        }
                    }

                    yield DustNullifierTransactionEvent::Transaction(DustNullifierTransaction {
                        transaction_hash,
                        block_height,
                        matching_nullifier_prefixes: matching_prefixes,
                    });
                }

                // Update current block for next iteration.
                if max_height > current_block {
                    current_block = max_height + 1;
                }
            }
        }
    }

    #[trace]
    fn get_dust_commitments(
        &self,
        commitment_prefixes: &[DustPrefix],
        start_index: u64,
        min_prefix_length: u32,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<DustCommitmentEvent, sqlx::Error>> + Send {
        let batch_size = batch_size.get() as i64;
        let min_prefix_length = min_prefix_length as usize;
        let mut current_index = start_index as i64;

        try_stream! {
            loop {
                // Filter prefixes that meet minimum length requirement.
                let valid_prefixes = commitment_prefixes
                    .iter()
                    .filter(|prefix| prefix.as_ref().len() >= min_prefix_length)
                    .collect::<Vec<_>>();

                if valid_prefixes.is_empty() {
                    break;
                }

                // Build conditions with parameter placeholders.
                let conditions = valid_prefixes
                    .iter()
                    .enumerate()
                    .map(|(i, prefix)| {
                        let param_num = 3 + i;
                        let hex_len = prefix.as_ref().len() * 2;

                        #[cfg(feature = "cloud")]
                        {
                            format!(
                                "substring(encode(commitment, 'hex'), 1, {hex_len}) = encode(${param_num}, 'hex')"
                            )
                        }

                        #[cfg(feature = "standalone")]
                        {
                            format!(
                                "substr(hex(commitment), 1, {hex_len}) = hex(${param_num})"
                            )
                        }
                    })
                    .collect::<Vec<_>>();

                let where_clause = conditions.join(" OR ");

                // Query commitments.
                let query = format!(
                    indoc! {"
                        SELECT 
                            id, commitment, nullifier, initial_value, 
                            owner, nonce, ctime, spent_at_transaction_id
                        FROM dust_utxos
                        WHERE id >= $1
                        AND ({})
                        ORDER BY id
                        LIMIT $2
                    "},
                    where_clause
                );

                // Build query with parameter bindings.
                let mut commitment_query = sqlx::query_as::<_, DustUtxosRow>(&query)
                    .bind(current_index)
                    .bind(batch_size);

                // Bind binary prefix parameters.
                for prefix in &valid_prefixes {
                    commitment_query = commitment_query.bind(prefix.as_ref());
                }

                let rows = commitment_query.fetch_all(&*self.pool).await?;

                if rows.is_empty() {
                    break;
                }

                for row in rows {
                    let spent_id = row.spent_at_transaction_id;
                    current_index = row.id as i64 + 1;

                    let mut commitment_info = DustCommitmentInfo::from(row);

                    // Get spent timestamp if spent.
                    if let Some(spent_id) = spent_id {
                        let spent_query = indoc! {"
                            SELECT b.timestamp
                            FROM transactions t
                            INNER JOIN blocks b ON b.id = t.block_id
                            WHERE t.id = $1
                        "};

                        let timestamp = sqlx::query_as::<_, (i64,)>(spent_query)
                            .bind(spent_id as i64)
                            .fetch_optional(&*self.pool)
                            .await?;

                        commitment_info.spent_at = timestamp.map(|(t,)| t as u64);
                    }

                    yield DustCommitmentEvent::Commitment(commitment_info);
                }

                // Query merkle tree updates in the same range.
                let merkle_query = indoc! {"
                    SELECT merkle_index, root, block_height, tree_data
                    FROM dust_commitment_tree
                    WHERE merkle_index >= $1
                    ORDER BY merkle_index
                    LIMIT $2
                "};

                let merkle_rows = sqlx::query_as::<_, DustCommitmentTreeRow>(merkle_query)
                    .bind(current_index)
                    .bind(batch_size)
                    .fetch_all(&*self.pool)
                    .await?;

                for row in merkle_rows {
                    let merkle_path = (!row.tree_data.0.is_empty()).then_some(row.tree_data.0);

                    yield DustCommitmentEvent::MerkleUpdate(DustCommitmentMerkleUpdate {
                        index: row.merkle_index,
                        collapsed_update: row.root,
                        block_height: row.block_height,
                        merkle_path,
                    });
                }
            }
        }
    }

    #[trace]
    fn get_registration_updates(
        &self,
        addresses: &[RegistrationAddress],
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<RegistrationUpdate, sqlx::Error>> + Send {
        let batch_size = batch_size.get() as i64;
        let mut last_id = 0;

        try_stream! {
            loop {
                // Build conditions based on address types.
                let conditions = addresses
                    .iter()
                    .map(|address| {
                        match address.address_type {
                            AddressType::CardanoStake => ("cardano_address", &address.value),

                            AddressType::Dust => ("dust_address", &address.value),

                            AddressType::Night => {
                                // Night addresses might map to DUST addresses through some.
                                // mechanism. For now, treat as DUST.
                                // address.
                                ("dust_address", &address.value)
                            }
                        }
                    })
                    .collect::<Vec<_>>();

                if conditions.is_empty() {
                    break;
                }

                let where_clause = conditions
                    .iter()
                    .enumerate()
                    .map(|(i, (col, _))| format!("{} = ${}", col, i + 2))
                    .join(" OR ");

                let query = format!(
                    indoc! {"
                        SELECT 
                            id, cardano_address, dust_address, is_valid, 
                            registered_at, removed_at
                        FROM cnight_registrations
                        WHERE id > $1
                        AND ({})
                        ORDER BY id
                        LIMIT ${}
                    "},
                    where_clause,
                    conditions.len() + 2
                );

                let mut registration_query =
                    sqlx::query_as::<_, CnightRegistrationsRow>(&query).bind(last_id as i64);
                for (_, bytes) in conditions {
                    registration_query = registration_query.bind(bytes);
                }
                registration_query = registration_query.bind(batch_size);

                let rows = registration_query.fetch_all(&*self.pool).await?;

                if rows.is_empty() {
                    break;
                }

                for row in rows {
                    last_id = row.id;
                    yield row.into();
                }
            }
        }
    }

    #[trace]
    async fn get_highest_generation_index_for_dust_address(
        &self,
        dust_address: &DustAddress,
    ) -> Result<Option<u64>, sqlx::Error> {
        let query = indoc! {"
            SELECT MAX(merkle_index)
            FROM dust_generation_info
            WHERE owner = $1
        "};

        let max = sqlx::query_as::<_, (i64,)>(query)
            .bind(dust_address.as_ref())
            .fetch_optional(&*self.pool)
            .await?
            .map(|(max,)| max as u64);

        Ok(max)
    }

    #[trace]
    async fn get_active_generation_count_for_dust_address(
        &self,
        dust_address: &DustAddress,
    ) -> Result<u32, sqlx::Error> {
        let query = indoc! {"
            SELECT COUNT(*)
            FROM dust_generation_info
            WHERE owner = $1
            AND dtime IS NULL
        "};

        let (count,) = sqlx::query_as::<_, (i64,)>(query)
            .bind(dust_address.as_ref())
            .fetch_one(&*self.pool)
            .await?;

        Ok(count as u32)
    }

    #[trace]
    async fn get_dust_events_by_transaction(
        &self,
        transaction_hash: TransactionHash,
    ) -> Result<Vec<DustEvent>, sqlx::Error> {
        #[derive(FromRow)]
        struct DustEventRow {
            transaction_hash: TransactionHash,
            logical_segment: i32,
            physical_segment: i32,
            event_data: Json<DustEventAttributes>,
        }

        let query = indoc! {"
            SELECT transaction_hash, logical_segment, physical_segment, event_data
            FROM dust_events
            WHERE transaction_hash = $1
            ORDER BY logical_segment, physical_segment
        "};

        let rows = sqlx::query_as::<_, DustEventRow>(query)
            .bind(transaction_hash.as_ref())
            .fetch_all(&*self.pool)
            .await?;

        Ok(rows
            .into_iter()
            .map(|row| DustEvent {
                transaction_hash: row.transaction_hash,
                logical_segment: row.logical_segment as u16,
                physical_segment: row.physical_segment as u16,
                event_details: row.event_data.0,
            })
            .collect())
    }

    #[trace]
    async fn get_recent_dust_events(
        &self,
        limit: u32,
        event_variant: Option<DustEventVariant>,
    ) -> Result<Vec<DustEvent>, sqlx::Error> {
        #[derive(FromRow)]
        struct DustEventRow {
            transaction_hash: TransactionHash,
            logical_segment: i32,
            physical_segment: i32,
            event_data: Json<DustEventAttributes>,
        }

        let query = if event_variant.is_some() {
            // Filter by event type.
            indoc! {"
                SELECT de.transaction_hash, de.logical_segment, de.physical_segment, de.event_data
                FROM dust_events de
                JOIN transactions t ON t.id = de.transaction_id
                WHERE de.event_type = $1
                ORDER BY t.id DESC, de.logical_segment, de.physical_segment
                LIMIT $2
            "}
        } else {
            // Get all event types.
            indoc! {"
                SELECT de.transaction_hash, de.logical_segment, de.physical_segment, de.event_data
                FROM dust_events de
                JOIN transactions t ON t.id = de.transaction_id
                ORDER BY t.id DESC, de.logical_segment, de.physical_segment
                LIMIT $1
            "}
        };

        let rows = if let Some(event_variant) = event_variant {
            sqlx::query_as::<_, DustEventRow>(query)
                .bind(event_variant)
                .bind(limit as i64)
                .fetch_all(&*self.pool)
                .await?
        } else {
            sqlx::query_as::<_, DustEventRow>(query)
                .bind(limit as i64)
                .fetch_all(&*self.pool)
                .await?
        };

        Ok(rows
            .into_iter()
            .map(|row| DustEvent {
                transaction_hash: row.transaction_hash,
                logical_segment: row.logical_segment as u16,
                physical_segment: row.physical_segment as u16,
                event_details: row.event_data.0,
            })
            .collect())
    }
}

/// Row type for dust generation info queries.
#[derive(Debug, Clone, FromRow)]
struct DustGenerationInfoRow {
    #[sqlx(try_from = "i64")]
    id: u64,

    night_utxo_hash: NightUtxoHash,

    #[sqlx(try_from = "U128BeBytes")]
    value: u128,

    owner: DustOwner,

    nonce: DustNonce,

    #[sqlx(try_from = "i64")]
    ctime: u64,

    #[sqlx(try_from = "SqlxOption<i64>")]
    dtime: Option<u64>,

    #[sqlx(try_from = "i64")]
    merkle_index: u64,
}

impl From<DustGenerationInfoRow> for DustGenerationInfo {
    fn from(row: DustGenerationInfoRow) -> Self {
        let DustGenerationInfoRow {
            night_utxo_hash,
            value,
            owner,
            nonce,
            ctime,
            dtime,
            merkle_index,
            ..
        } = row;

        Self {
            night_utxo_hash,
            value,
            owner,
            nonce,
            ctime,
            dtime,
            merkle_index,
        }
    }
}

/// Row type for dust_utxos table queries.
#[derive(Debug, Clone, FromRow)]
struct DustUtxosRow {
    #[sqlx(try_from = "i64")]
    id: u64,

    commitment: DustCommitment,

    nullifier: Option<DustNullifier>,

    #[sqlx(rename = "initial_value", try_from = "U128BeBytes")]
    value: u128,

    owner: DustOwner,

    nonce: DustNonce,

    #[sqlx(try_from = "i64")]
    ctime: u64,

    #[sqlx(try_from = "SqlxOption<i64>")]
    spent_at_transaction_id: Option<u64>,
}

/// Row type for dust_commitment_tree table queries.
#[derive(Debug, Clone, FromRow)]
struct DustCommitmentTreeRow {
    #[sqlx(try_from = "i64")]
    block_height: u32,

    #[sqlx(try_from = "i64")]
    merkle_index: u64,

    root: DustMerkleUpdate, // This is actually the collapsed update data, not a root hash.

    tree_data: Json<Vec<DustMerklePathEntry>>, // Merkle path data stored as JSON.
}

/// Row type for dust_generation_tree table queries.
#[derive(Debug, Clone, FromRow)]
struct DustGenerationTreeRow {
    #[sqlx(try_from = "i64")]
    block_height: u32,

    #[sqlx(try_from = "i64")]
    merkle_index: u64,

    root: DustMerkleUpdate, // This is actually the collapsed update data, not a root hash.

    tree_data: Json<Vec<DustMerklePathEntry>>, // Merkle path data stored as JSON.
}

impl From<DustUtxosRow> for DustCommitmentInfo {
    fn from(row: DustUtxosRow) -> Self {
        let DustUtxosRow {
            commitment,
            nullifier,
            value,
            owner,
            nonce,
            ctime,
            ..
        } = row;

        Self {
            commitment,
            nullifier,
            value,
            owner,
            nonce,
            created_at: ctime,
            spent_at: None, // This needs to be handled separately with an additional query.
        }
    }
}

/// Row type for cnight_registrations table queries.
#[derive(Debug, Clone, FromRow)]
struct CnightRegistrationsRow {
    #[sqlx(try_from = "i64")]
    id: u64,

    cardano_address: CardanoStakeKey,

    dust_address: DustAddress,

    is_valid: bool,

    #[sqlx(try_from = "i64")]
    registered_at: u64,

    #[sqlx(try_from = "SqlxOption<i64>")]
    removed_at: Option<u64>,
}

impl From<CnightRegistrationsRow> for RegistrationUpdate {
    fn from(row: CnightRegistrationsRow) -> Self {
        let CnightRegistrationsRow {
            cardano_address,
            dust_address,
            is_valid,
            registered_at,
            removed_at,
            ..
        } = row;

        Self {
            cardano_stake_key: cardano_address,
            dust_address,
            is_active: is_valid && removed_at.is_none(),
            registered_at,
            removed_at,
        }
    }
}
