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
            AddressType, DustCommitmentEvent, DustCommitmentInfo, DustCommitmentMerkleUpdate,
            DustCommitmentProgress, DustGenerationEvent, DustGenerationInfo,
            DustGenerationMerkleUpdate, DustGenerationProgress, DustGenerationStatus,
            DustMerkleTreeType, DustNullifierTransaction, DustNullifierTransactionEvent,
            DustNullifierTransactionProgress, DustSystemState, RegistrationAddress,
            RegistrationUpdate, RegistrationUpdateEvent, RegistrationUpdateProgress,
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
    infra::sqlx::{QueryableByteArray, SqlxOption, U128BeBytes},
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

        let commitment_tree_root = sqlx::query_as::<_, QueryableByteArray<32>>(query)
            .fetch_optional(&*self.pool)
            .await?
            .map(|bytes| bytes.into())
            .unwrap_or_default();

        // Get latest generation tree root.
        let query = indoc! {"
            SELECT root
            FROM dust_generation_tree
            ORDER BY block_height DESC
            LIMIT 1
        "};

        let generation_tree_root = sqlx::query_as::<_, QueryableByteArray<32>>(query)
            .fetch_optional(&*self.pool)
            .await?
            .map(|bytes| bytes.into())
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
        cardano_stake_keys: &[indexer_common::domain::CardanoStakeKey],
    ) -> Result<Vec<DustGenerationStatus>, sqlx::Error> {
        // Convert stake keys to bytea format for querying.
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

            let registration_info = sqlx::query_as::<_, (Vec<u8>, bool)>(registration_query)
                .bind(stake_key.as_ref())
                .fetch_optional(&*self.pool)
                .await?;

            let (dust_address, is_registered): (Option<DustAddress>, bool) = match registration_info
            {
                Some((addr, valid)) => (addr.as_slice().try_into().ok(), valid),
                None => (None, false),
            };

            // Query active generation info if registered
            let mut generation_rate = 0u128;
            let mut current_capacity = 0u128;
            let mut night_balance = 0u128;

            if let Some(ref dust_addr) = dust_address {
                // Get active generation info
                let generation_query = indoc! {"
                    SELECT value
                    FROM dust_generation_info
                    WHERE owner = $1 AND dtime IS NULL
                    ORDER BY ctime DESC
                    LIMIT 1
                "};

                let value = sqlx::query_as::<_, (U128BeBytes,)>(generation_query)
                    .bind(dust_addr.as_ref())
                    .fetch_optional(&*self.pool)
                    .await?;

                if let Some((value,)) = value {
                    let value_u128 = value.into();
                    night_balance = value_u128;
                    // Simplified generation rate calculation (1 Speck per NIGHT per second)
                    generation_rate = value_u128;
                    // Capacity could be calculated based on time since ctime
                    current_capacity = 0; // TODO: Calculate based on elapsed time
                }
            }

            statuses.push(DustGenerationStatus {
                cardano_stake_key: stake_key.clone(),
                dust_address,
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
        timestamp: i32,
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

        let root = sqlx::query_as::<_, QueryableByteArray<32>>(query)
            .bind(timestamp as i64)
            .fetch_optional(&*self.pool)
            .await?;

        Ok(root.map(|r| r.into()))
    }

    async fn get_dust_generations(
        &self,
        dust_address: &indexer_common::domain::DustAddress,
        from_generation_index: i64,
        from_merkle_index: i64,
        only_active: bool,
        batch_size: NonZeroU32,
    ) -> Result<impl Stream<Item = Result<DustGenerationEvent, sqlx::Error>> + Send, sqlx::Error>
    {
        let batch_size = batch_size.get() as i64;

        let stream = try_stream! {
            let mut last_index = from_generation_index;
            let mut last_merkle_index = from_merkle_index;
            let mut has_more = true;

            while has_more {
                // Query generation info
                let query = if only_active {
                    indoc! {r#"
                        SELECT 
                            id, night_utxo_hash, value, owner, nonce, 
                            ctime, dtime, "index" as merkle_index
                        FROM dust_generation_info
                        WHERE owner = $1 AND "index" >= $2 AND dtime IS NULL
                        ORDER BY "index"
                        LIMIT $3
                    "#}
                } else {
                    indoc! {r#"
                        SELECT 
                            id, night_utxo_hash, value, owner, nonce, 
                            ctime, dtime, "index" as merkle_index
                        FROM dust_generation_info
                        WHERE owner = $1 AND "index" >= $2
                        ORDER BY "index"
                        LIMIT $3
                    "#}
                };

                let rows: Vec<DustGenerationRow> =
                    sqlx::query_as(query)
                        .bind(dust_address.as_ref())
                        .bind(last_index)
                        .bind(batch_size)
                        .fetch_all(&*self.pool)
                        .await?;

                if rows.is_empty() {
                    has_more = false;
                } else {
                    for row in rows {
                        yield DustGenerationEvent::Info(DustGenerationInfo {
                            night_utxo_hash: row.night_utxo_hash,
                            value: row.value,
                            owner: row.owner,
                            nonce: row.nonce,
                            ctime: row.ctime as u32,
                            dtime: row.dtime.map(|d| d as u32),
                            merkle_index: row.index as u32,
                        });

                        last_index = row.id as i64 + 1;
                        last_merkle_index = row.index as i64 + 1;
                    }

                    // Query merkle updates for this batch
                    let merkle_query = indoc! {r#"
                        SELECT "index", root, block_height
                        FROM dust_generation_tree
                        WHERE "index" >= $1
                        ORDER BY "index"
                        LIMIT $2
                    "#};

                    let merkle_rows: Vec<MerkleTreeRow> = sqlx::query_as(merkle_query)
                        .bind(last_merkle_index)
                        .bind(batch_size)
                        .fetch_all(&*self.pool)
                        .await?;

                    for row in merkle_rows {
                        yield DustGenerationEvent::MerkleUpdate(DustGenerationMerkleUpdate {
                            index: row.index as u32,
                            collapsed_update: row.root.into(),
                            block_height: row.block_height as u32,
                        });
                    }

                    // Send progress update
                    let active_count_query = indoc! {"
                        SELECT COUNT(*)
                        FROM dust_generation_info
                        WHERE owner = $1 AND dtime IS NULL
                    "};

                    let (active_count,) = sqlx::query_as::<_, (i64,)>(active_count_query)
                        .bind(dust_address.as_ref())
                        .fetch_one(&*self.pool)
                        .await?;

                    yield DustGenerationEvent::Progress(DustGenerationProgress {
                        highest_index: last_merkle_index as u32,
                        active_generations: active_count as u32,
                    });
                }
            }
        };

        Ok(stream)
    }

    #[trace]
    async fn get_dust_nullifier_transactions(
        &self,
        prefixes: &[DustPrefix],
        min_prefix_length: i32,
        from_block: i32,
        batch_size: NonZeroU32,
    ) -> Result<
        impl Stream<Item = Result<DustNullifierTransactionEvent, sqlx::Error>> + Send,
        sqlx::Error,
    > {
        let batch_size = batch_size.get() as i64;
        let min_prefix_length = min_prefix_length as usize;

        let stream = try_stream! {
            let mut current_block = from_block as i64;
            let mut has_more = true;

            while has_more {
                // Build prefix conditions for the query
                let mut conditions = Vec::new();
                for prefix in prefixes {
                    if prefix.as_ref().len() >= min_prefix_length {
                        let hex_prefix = prefix.hex_encode().to_string();
                        #[cfg(feature = "cloud")]
                        conditions.push(format!("substring(nullifier::text, 1, {}) = '\\\\x{}'::text",
                            hex_prefix.len(), hex_prefix));
                        #[cfg(feature = "standalone")]
                        conditions.push(format!("substr(hex(nullifier), 1, {}) = '{}'",
                            hex_prefix.len(), hex_prefix));
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

                let rows: Vec<(Vec<u8>, i64, i64)> = sqlx::query_as(&query)
                    .bind(current_block)
                    .bind(batch_size)
                    .fetch_all(&*self.pool)
                    .await?;

                if rows.is_empty() {
                    has_more = false;
                } else {
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
                            let nullifier: indexer_common::domain::DustNullifier = nullifier_bytes.as_slice().try_into().unwrap();
                            let nullifier_hex = nullifier.hex_encode().to_string();
                            for prefix in prefixes {
                                let hex_prefix = prefix.hex_encode().to_string();
                                if nullifier_hex.starts_with(&hex_prefix) && prefix.as_ref().len() >= min_prefix_length {
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

                    // Send progress update
                    let matched_count = rows.len() as u32;
                    yield DustNullifierTransactionEvent::Progress(DustNullifierTransactionProgress {
                        highest_block: (current_block as u32).saturating_sub(1),
                        matched_count,
                    });
                }
            }
        };

        Ok(stream)
    }

    #[trace]
    async fn get_dust_commitments(
        &self,
        commitment_prefixes: &[DustPrefix],
        start_index: i32,
        min_prefix_length: i32,
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

                let rows: Vec<DustUtxoRow> =
                    sqlx::query_as(&query)
                        .bind(current_index)
                        .bind(batch_size)
                        .fetch_all(&*self.pool)
                        .await?;

                if rows.is_empty() {
                    has_more = false;
                } else {
                    let mut commitment_count = 0;

                    for row in rows {
                        // Get spent timestamp if spent
                        let spent_at = if row.spent_at_transaction_id.is_some() {
                            let spent_query = indoc! {"
                                SELECT b.timestamp
                                FROM transactions t
                                INNER JOIN blocks b ON b.id = t.block_id
                                WHERE t.id = $1
                            "};

                            let timestamp = sqlx::query_as::<_, (i64,)>(spent_query)
                                .bind(row.spent_at_transaction_id.map(|id| id as i64))
                                .fetch_optional(&*self.pool)
                                .await?;

                            timestamp.map(|(t,)| t as u32)
                        } else {
                            None
                        };

                        yield DustCommitmentEvent::Commitment(DustCommitmentInfo {
                            commitment: row.commitment,
                            nullifier: row.nullifier,
                            value: row.value,
                            owner: row.owner,
                            nonce: row.nonce,
                            created_at: row.ctime as u32,
                            spent_at,
                        });

                        commitment_count += 1;
                        current_index = row.id as i64 + 1;
                    }

                    // Query merkle updates
                    let merkle_query = indoc! {r#"
                        SELECT "index", root, block_height
                        FROM dust_commitment_tree
                        WHERE "index" >= $1
                        ORDER BY "index"
                        LIMIT $2
                    "#};

                    let merkle_rows: Vec<MerkleTreeRow> = sqlx::query_as(merkle_query)
                        .bind(current_index)
                        .bind(batch_size)
                        .fetch_all(&*self.pool)
                        .await?;

                    for row in merkle_rows {
                        yield DustCommitmentEvent::MerkleUpdate(DustCommitmentMerkleUpdate {
                            index: row.index as u32,
                            collapsed_update: row.root.into(),
                            block_height: row.block_height as u32,
                        });
                    }

                    // Send progress update
                    yield DustCommitmentEvent::Progress(DustCommitmentProgress {
                        highest_index: current_index as u32,
                        commitment_count,
                    });
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

                let mut query_builder = sqlx::query_as::<_, RegistrationRow>(&query)
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
                    let update_count = rows.len() as u32;
                    let mut latest_timestamp = 0i64;

                    for row in rows {
                        yield RegistrationUpdateEvent::Update(RegistrationUpdate {
                            cardano_stake_key: row.cardano_address,
                            dust_address: row.dust_address,
                            is_active: row.is_valid && row.removed_at.is_none(),
                            registered_at: row.registered_at as u32,
                            removed_at: row.removed_at.map(|t| t as u32),
                        });

                        last_id = row.id as i64;
                        latest_timestamp = latest_timestamp.max(row.registered_at as i64);
                        if let Some(removed) = row.removed_at {
                            latest_timestamp = latest_timestamp.max(removed as i64);
                        }
                    }

                    // Send progress update
                    yield RegistrationUpdateEvent::Progress(RegistrationUpdateProgress {
                        latest_timestamp: latest_timestamp as u32,
                        update_count,
                    });
                }
            }
        };

        Ok(stream)
    }
}

/// Row type for dust generation info queries.
#[derive(Debug, Clone, FromRow)]
struct DustGenerationRow {
    #[sqlx(try_from = "i64")]
    id: u64,

    #[cfg_attr(feature = "standalone", sqlx(try_from = "&'a [u8]"))]
    night_utxo_hash: NightUtxoHash,

    #[sqlx(try_from = "U128BeBytes")]
    value: u128,

    #[cfg_attr(feature = "standalone", sqlx(try_from = "&'a [u8]"))]
    owner: DustOwner,

    #[cfg_attr(feature = "standalone", sqlx(try_from = "&'a [u8]"))]
    nonce: DustNonce,

    #[sqlx(try_from = "i64")]
    ctime: u64,

    #[sqlx(try_from = "SqlxOption<i64>")]
    dtime: Option<u64>,

    #[sqlx(rename = "merkle_index", try_from = "i64")]
    index: u64,
}

/// Row type for dust utxo queries.
#[derive(Debug, Clone, FromRow)]
struct DustUtxoRow {
    #[sqlx(try_from = "i64")]
    id: u64,

    #[cfg_attr(feature = "standalone", sqlx(try_from = "&'a [u8]"))]
    commitment: indexer_common::domain::DustCommitment,

    #[cfg_attr(feature = "standalone", sqlx(try_from = "SqlxOption<&'a [u8]>"))]
    nullifier: Option<indexer_common::domain::DustNullifier>,

    #[sqlx(rename = "initial_value", try_from = "U128BeBytes")]
    value: u128,

    #[cfg_attr(feature = "standalone", sqlx(try_from = "&'a [u8]"))]
    owner: DustOwner,

    #[cfg_attr(feature = "standalone", sqlx(try_from = "&'a [u8]"))]
    nonce: DustNonce,

    #[sqlx(try_from = "i64")]
    ctime: u64,

    #[sqlx(try_from = "SqlxOption<i64>")]
    spent_at_transaction_id: Option<u64>,
}

/// Row type for merkle tree queries.
#[derive(Debug, Clone, FromRow)]
struct MerkleTreeRow {
    #[sqlx(try_from = "i64")]
    index: u64,

    root: Vec<u8>,

    #[sqlx(try_from = "i64")]
    block_height: u64,
}

/// Row type for registration queries.
#[derive(Debug, Clone, FromRow)]
struct RegistrationRow {
    #[sqlx(try_from = "i64")]
    id: u64,

    #[cfg_attr(feature = "standalone", sqlx(try_from = "&'a [u8]"))]
    cardano_address: CardanoStakeKey,

    #[cfg_attr(feature = "standalone", sqlx(try_from = "&'a [u8]"))]
    dust_address: DustAddress,

    is_valid: bool,

    #[sqlx(try_from = "i64")]
    registered_at: u64,

    #[sqlx(try_from = "SqlxOption<i64>")]
    removed_at: Option<u64>,
}
