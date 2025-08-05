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

use crate::{
    domain::{
        dust::{
            AddressType, DustCommitmentEvent, DustCommitmentInfo, DustGenerationEvent,
            DustGenerationInfo, DustGenerationStatus, DustMerkleTreeType, DustNullifierTransaction,
            DustNullifierTransactionEvent, DustSystemState, RegistrationAddress,
            RegistrationUpdate, RegistrationUpdateEvent,
        },
        storage::dust::DustStorage,
    },
    infra::{api::AsBytesExt, storage::Storage},
};
use async_stream::try_stream;
use fastrace::trace;
use futures::Stream;
use indexer_common::{
    domain::{
        CardanoStakeKey, DustAddress, DustMerkleRoot, DustNonce, DustOwner, DustPrefix,
        NightUtxoHash,
    },
    infra::sqlx::{SqlxOption, U128BeBytes},
};
use indoc::indoc;
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
            // Query registration info
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
                    SELECT value
                    FROM dust_generation_info
                    WHERE owner = $1 AND dtime IS NULL
                    ORDER BY ctime DESC
                    LIMIT 1
                "};

                let value = sqlx::query_as::<_, (U128BeBytes,)>(generation_query)
                    .bind(dust_address.as_ref())
                    .fetch_optional(&*self.pool)
                    .await?
                    .map(|(x,)| x);

                if let Some(value) = value {
                    let value_u128 = value.into();
                    night_balance = value_u128;
                    // Simplified generation rate calculation (1 Speck per NIGHT per second).
                    generation_rate = value_u128;
                    // Capacity could be calculated based on time since ctime.
                    current_capacity = 0; // TODO: Calculate based on elapsed time
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
        dust_address: &indexer_common::domain::DustAddress,
        from_generation_index: u64,
        _from_merkle_index: u64,
        only_active: bool,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<DustGenerationEvent, sqlx::Error>> + Send {
        let batch_size = batch_size.get() as i64;

        try_stream! {
            let mut last_index = from_generation_index;

            loop {
                // Query generation info
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

                // TODO: Merkle tree updates are disabled.
                // Schema mismatch: domain expects 'index' field but table only has 'id'.
                // Need schema update or domain model change to fix this.
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

        try_stream! {
            let mut current_block = from_block as i64;

            loop {
                // Build prefix conditions for the query
                let mut conditions = Vec::new();
                for prefix in prefixes {
                    if prefix.as_ref().len() >= min_prefix_length {
                        let hex_prefix = prefix.hex_encode().to_string();
                        #[cfg(feature = "cloud")]
                        conditions.push(format!(
                            "substring(nullifier::text, 1, {}) = '\\\\x{}'::text",
                            hex_prefix.len(),
                            hex_prefix
                        ));

                        #[cfg(feature = "standalone")]
                        conditions.push(format!(
                            "substr(hex(nullifier), 1, {}) = '{}'",
                            hex_prefix.len(),
                            hex_prefix
                        ));
                    }
                }

                if conditions.is_empty() {
                    break;
                }

                let where_clause = conditions.join(" OR ");

                // Query transactions with matching nullifiers
                let query = format!(
                    indoc! {"
                        SELECT DISTINCT t.hash, t.block_id, b.height
                        FROM dust_utxos du
                        INNER JOIN transactions t ON t.id = du.spent_at_transaction_id
                        INNER JOIN blocks b ON b.id = t.block_id
                        WHERE du.nullifier IS NOT NULL
                        AND ({})
                        AND b.height >= $1
                        ORDER BY b.height
                        LIMIT $2
                    "},
                    where_clause
                );

                let rows = sqlx::query_as::<_, (Vec<u8>, i64, i64)>(&query)
                    .bind(current_block)
                    .bind(batch_size)
                    .fetch_all(&*self.pool)
                    .await?;

                if rows.is_empty() {
                    break;
                }

                for (tx_hash, _block_id, block_height) in &rows {
                    // Find matching prefixes for this transaction
                    let nullifier_query = indoc! {"
                            SELECT nullifier
                            FROM dust_utxos
                            WHERE spent_at_transaction_id = (
                                SELECT id FROM transactions WHERE hash = $1
                            )
                            AND nullifier IS NOT NULL
                        "};

                    let nullifiers = sqlx::query_as::<_, (Vec<u8>,)>(nullifier_query)
                        .bind(tx_hash)
                        .fetch_all(&*self.pool)
                        .await?;

                    let mut matching_prefixes = Vec::new();
                    for (nullifier_bytes,) in nullifiers {
                        let nullifier: indexer_common::domain::DustNullifier =
                            nullifier_bytes.as_slice().try_into().unwrap();
                        let nullifier_hex = nullifier.hex_encode().to_string();
                        for prefix in prefixes {
                            let hex_prefix = prefix.hex_encode().to_string();
                            if nullifier_hex.starts_with(&hex_prefix)
                                && prefix.as_ref().len() >= min_prefix_length
                            {
                                matching_prefixes.push(prefix.clone());
                            }
                        }
                    }

                    yield DustNullifierTransactionEvent::Transaction(DustNullifierTransaction {
                        transaction_hash: tx_hash.as_slice().try_into().unwrap(),
                        block_height: *block_height as u32,
                        matching_nullifier_prefixes: matching_prefixes,
                    });
                }

                // Update current block for next iteration
                if let Some((_, _, last_height)) = rows.last() {
                    current_block = last_height + 1;
                }
            }
        }
    }

    #[trace]
    async fn get_dust_commitments(
        &self,
        commitment_prefixes: &[DustPrefix],
        start_index: u64,
        min_prefix_length: u32,
        batch_size: NonZeroU32,
    ) -> Result<impl Stream<Item = Result<DustCommitmentEvent, sqlx::Error>> + Send, sqlx::Error>
    {
        let batch_size = batch_size.get() as i64;
        let min_prefix_length = min_prefix_length as usize;

        let stream = try_stream! {
            let mut current_index = start_index as i64;
            let mut has_more = true;

            while has_more {
                // Build prefix conditions
                let mut conditions = Vec::new();
                for prefix in commitment_prefixes {
                    if prefix.as_ref().len() >= min_prefix_length {
                        let hex_prefix = prefix.hex_encode().to_string();
                        #[cfg(feature = "cloud")]
                        conditions.push(format!("substring(commitment::text, 1, {}) = '\\\\x{}'::text",
                            hex_prefix.len(), hex_prefix));
                        #[cfg(feature = "standalone")]
                        conditions.push(format!("substr(hex(commitment), 1, {}) = '{}'",
                            hex_prefix.len(), hex_prefix));
                    }
                }

                if conditions.is_empty() {
                    break;
                }

                let where_clause = conditions.join(" OR ");

                // Query commitments
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

                let rows: Vec<DustUtxosRow> =
                    sqlx::query_as(&query)
                        .bind(current_index)
                        .bind(batch_size)
                        .fetch_all(&*self.pool)
                        .await?;

                if rows.is_empty() {
                    has_more = false;
                } else {
                    for row in rows {
                        let spent_id = row.spent_at_transaction_id;
                        current_index = row.id as i64 + 1;

                        let mut commitment_info: DustCommitmentInfo = row.into();

                        // Get spent timestamp if spent
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

                    // TODO: Fix merkle update handling - table schema doesn't have index column
                    // The dust_commitment_tree table only has 'id' (database PK), not 'index' (merkle position)
                    // Merkle update functionality is disabled until schema is updated
                    //
                    // let merkle_query = indoc! {r#"
                    //     SELECT "index", root, block_height
                    //     FROM dust_commitment_tree
                    //     WHERE "index" >= $1
                    //     ORDER BY "index"
                    //     LIMIT $2
                    // "#};
                    //
                    // let merkle_rows: Vec<DustCommitmentTreeRow> = sqlx::query_as(merkle_query)
                    //     .bind(current_index)
                    //     .bind(batch_size)
                    //     .fetch_all(&*self.pool)
                    //     .await?;
                    //
                    // for row in merkle_rows {
                    //     yield DustCommitmentEvent::MerkleUpdate(DustCommitmentMerkleUpdate {
                    //         index: row.index as u32,
                    //         collapsed_update: row.root.into(),
                    //         block_height: row.block_height as u32,
                    //     });
                    // }

                }
            }
        };

        Ok(stream)
    }

    #[trace]
    async fn get_registration_updates(
        &self,
        addresses: &[RegistrationAddress],
        batch_size: NonZeroU32,
    ) -> Result<impl Stream<Item = Result<RegistrationUpdateEvent, sqlx::Error>> + Send, sqlx::Error>
    {
        let batch_size = batch_size.get() as i64;

        let stream = try_stream! {
            let mut last_id = 0i64;
            let mut has_more = true;

            while has_more {
                // Build conditions based on address types
                let mut conditions = Vec::new();

                for addr in addresses {
                    let addr_bytes = addr.value.as_ref();

                    match addr.address_type {
                        AddressType::CardanoStake => {
                            conditions.push(("cardano_address", addr_bytes));
                        }
                        AddressType::Dust => {
                            conditions.push(("dust_address", addr_bytes));
                        }
                        AddressType::Night => {
                            // Night addresses might map to DUST addresses through some mechanism
                            // For now, treat as DUST address
                            conditions.push(("dust_address", addr_bytes));
                        }
                    }
                }

                if conditions.is_empty() {
                    break;
                }

                // Build WHERE clause
                let where_parts: Vec<String> = conditions.iter()
                    .enumerate()
                    .map(|(i, (col, _))| format!("{} = ${}", col, i + 2))
                    .collect();
                let where_clause = where_parts.join(" OR ");

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

                let mut query_builder = sqlx::query_as::<_, CnightRegistrationsRow>(&query)
                    .bind(last_id);

                for (_, bytes) in &conditions {
                    query_builder = query_builder.bind(bytes);
                }

                query_builder = query_builder.bind(batch_size);

                let rows = query_builder
                    .fetch_all(&*self.pool)
                    .await?;

                if rows.is_empty() {
                    has_more = false;
                } else {
                    let _update_count = rows.len() as u32;
                    let mut latest_timestamp = 0i64;

                    for row in rows {
                        let row_id = row.id;
                        let row_registered_at = row.registered_at;
                        let row_removed_at = row.removed_at;

                        yield RegistrationUpdateEvent::Update(row.into());

                        last_id = row_id as i64;
                        latest_timestamp = latest_timestamp.max(row_registered_at as i64);
                        if let Some(removed) = row_removed_at {
                            latest_timestamp = latest_timestamp.max(removed as i64);
                        }
                    }

                }
            }
        };

        Ok(stream)
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
            .map(|(m,)| m as u64);

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

    commitment: indexer_common::domain::DustCommitment,

    nullifier: Option<indexer_common::domain::DustNullifier>,

    #[sqlx(rename = "initial_value", try_from = "U128BeBytes")]
    value: u128,

    owner: DustOwner,

    nonce: DustNonce,

    #[sqlx(try_from = "i64")]
    ctime: u64,

    #[sqlx(try_from = "SqlxOption<i64>")]
    spent_at_transaction_id: Option<u64>,
}

// TODO: Uncomment these when merkle tree functionality is re-enabled
// /// Row type for dust_commitment_tree table queries.
// #[derive(Debug, Clone, FromRow)]
// struct DustCommitmentTreeRow {
//     #[sqlx(try_from = "i64")]
//     id: u64,
//
//     #[sqlx(try_from = "i64")]
//     block_height: u32,
//
//     root: Vec<u8>,
//
//     tree_data: Vec<u8>,
// }
//
// /// Row type for dust_generation_tree table queries.
// #[derive(Debug, Clone, FromRow)]
// struct DustGenerationTreeRow {
//     #[sqlx(try_from = "i64")]
//     id: u64,
//
//     #[sqlx(try_from = "i64")]
//     block_height: u32,
//
//     root: Vec<u8>,
//
//     tree_data: Vec<u8>,
// }

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
            spent_at: None, // This needs to be handled separately with an additional query
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
