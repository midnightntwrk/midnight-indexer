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
    domain::{
        DustCommitmentEvent, DustGenerationEvent, DustGenerationStatus,
        DustNullifierTransactionEvent, DustSystemState, RegistrationUpdateEvent,
        storage::dust::DustStorage,
    },
    infra::storage::Storage,
};
use async_stream::try_stream;
#[cfg(feature = "cloud")]
use fastrace::trace;
use futures::Stream;
use indexer_common::{
    domain::{AddressType, DustMerkleTreeType},
    stream::flatten_chunks,
};
use indoc::indoc;
use std::num::NonZeroU32;

impl DustStorage for Storage {
    #[cfg_attr(feature = "cloud", trace)]
    async fn get_current_dust_state(&self) -> Result<DustSystemState, sqlx::Error> {
        // Get the latest block height and timestamp
        let query = indoc! {"
            SELECT height, timestamp
            FROM blocks
            ORDER BY height DESC
            LIMIT 1
        "};

        let (block_height, timestamp) = sqlx::query_as::<_, (i64, i64)>(query)
            .fetch_optional(&*self.pool)
            .await?
            .unwrap_or((0, 0));

        // Get commitment tree root (placeholder implementation)
        let commitment_tree_root =
            "0000000000000000000000000000000000000000000000000000000000000000".to_owned();

        // Get generation tree root (placeholder implementation)
        let generation_tree_root =
            "0000000000000000000000000000000000000000000000000000000000000000".to_owned();

        // Get total registrations
        let query = indoc! {"
            SELECT COUNT(*)
            FROM cnight_registrations
            WHERE is_valid = TRUE
        "};

        let (total_registrations,) = sqlx::query_as::<_, (i64,)>(query)
            .fetch_one(&*self.pool)
            .await?;

        Ok(DustSystemState {
            commitment_tree_root,
            generation_tree_root,
            block_height: block_height as u32,
            timestamp,
            total_registrations: total_registrations as u32,
        })
    }

    #[cfg_attr(feature = "cloud", trace)]
    async fn get_dust_generation_status_batch(
        &self,
        cardano_stake_keys: &[String],
    ) -> Result<Vec<DustGenerationStatus>, sqlx::Error> {
        let mut results = Vec::new();

        for stake_key in cardano_stake_keys {
            let query = indoc! {"
                SELECT dust_address, is_valid
                FROM cnight_registrations
                WHERE cardano_address = $1
                ORDER BY registered_at DESC
                LIMIT 1
            "};

            let registration = sqlx::query_as::<_, (Vec<u8>, bool)>(query)
                .bind(stake_key.as_bytes())
                .fetch_optional(&*self.pool)
                .await?;

            let (dust_address, is_registered) = match registration {
                Some((dust_addr, is_valid)) => (Some(const_hex::encode(dust_addr)), is_valid),
                None => (None, false),
            };

            // Placeholder values for generation rate, capacity, and night balance
            let generation_rate = "0".to_owned();
            let current_capacity = "0".to_owned();
            let night_balance = "0".to_owned();

            results.push(DustGenerationStatus {
                cardano_stake_key: stake_key.clone(),
                dust_address,
                is_registered,
                generation_rate,
                current_capacity,
                night_balance,
            });
        }

        Ok(results)
    }

    #[cfg_attr(feature = "cloud", trace)]
    async fn get_dust_merkle_root_at_timestamp(
        &self,
        tree_type: DustMerkleTreeType,
        timestamp: i64,
    ) -> Result<Option<Vec<u8>>, sqlx::Error> {
        let table_name = match tree_type {
            DustMerkleTreeType::Commitment => "dust_commitment_tree",
            DustMerkleTreeType::Generation => "dust_generation_tree",
        };

        let query = format!(
            "SELECT root FROM {table_name} WHERE block_height <= (SELECT height FROM blocks WHERE timestamp <= $1 ORDER BY height DESC LIMIT 1) ORDER BY block_height DESC LIMIT 1"
        );

        let root = sqlx::query_scalar(&query)
            .bind(timestamp)
            .fetch_optional(&*self.pool)
            .await?;

        Ok(root)
    }

    fn get_dust_generations(
        &self,
        dust_address: &str,
        from_generation_index: i64,
        _from_merkle_index: i64,
        only_active: bool,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<DustGenerationEvent, sqlx::Error>> {
        let dust_address = dust_address.to_owned();
        let pool = self.pool.clone();

        let chunks = try_stream! {
            let mut generation_index = from_generation_index;

            loop {
                let query = if only_active {
                    indoc! {"
                        SELECT night_utxo_hash, value, owner, nonce, ctime, dtime, merkle_index
                        FROM dust_generation_info
                        WHERE owner = $1
                        AND merkle_index >= $2
                        AND dtime IS NULL
                        ORDER BY merkle_index
                        LIMIT $3
                    "}
                } else {
                    indoc! {"
                        SELECT night_utxo_hash, value, owner, nonce, ctime, dtime, merkle_index
                        FROM dust_generation_info
                        WHERE owner = $1
                        AND merkle_index >= $2
                        ORDER BY merkle_index
                        LIMIT $3
                    "}
                };

                let dust_address_bytes = const_hex::decode(&dust_address)
                    .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;

                let rows = sqlx::query_as::<_, (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, i64, Option<i64>, i64)>(query)
                    .bind(&dust_address_bytes)
                    .bind(generation_index)
                    .bind(batch_size.get() as i64)
                    .fetch_all(&*pool)
                    .await?;

                if rows.is_empty() {
                    break;
                }

                let events: Vec<DustGenerationEvent> = rows
                    .into_iter()
                    .map(|(night_utxo_hash, value, owner, nonce, ctime, dtime, merkle_index)| {
                        let night_utxo_hash = indexer_common::domain::NightUtxoHash::try_from(night_utxo_hash)
                            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
                        let owner = indexer_common::domain::DustOwner::try_from(owner)
                            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
                        let nonce = indexer_common::domain::DustNonce::try_from(nonce)
                            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
                        let value = u128::from_be_bytes(
                            value.try_into().map_err(|_| sqlx::Error::Decode(Box::new(
                                std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid value bytes")
                            )))?
                        );

                        generation_index = merkle_index + 1;

                        Ok(DustGenerationEvent::DustGenerationInfo(
                            crate::domain::DustGenerationInfo {
                                night_utxo_hash,
                                value,
                                owner,
                                nonce,
                                ctime,
                                dtime,
                                merkle_index: merkle_index as u64,
                            }
                        ))
                    })
                    .collect::<Result<Vec<_>, sqlx::Error>>()?;

                yield events;
            }
        };

        flatten_chunks(chunks)
    }

    fn get_dust_nullifier_transactions(
        &self,
        prefixes: &[String],
        _min_prefix_length: usize,
        from_block: i64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<DustNullifierTransactionEvent, sqlx::Error>> {
        let prefixes = prefixes.to_owned();
        let pool = self.pool.clone();

        let chunks = try_stream! {
            let mut block_height = from_block;

            loop {
                let query = indoc! {"
                    SELECT t.id, t.hash, t.protocol_version, t.transaction_result, t.raw, t.merkle_tree_root, t.start_index, t.end_index, t.paid_fees, t.estimated_fees, t.block_id
                    FROM transactions t
                    JOIN blocks b ON t.block_id = b.id
                    WHERE b.height >= $1
                    ORDER BY b.height, t.id
                    LIMIT $2
                "};

                let transactions = sqlx::query_as::<_, (i64, Vec<u8>, i64, sqlx::types::JsonValue, Vec<u8>, Vec<u8>, i64, i64, Option<Vec<u8>>, Option<Vec<u8>>, i64)>(query)
                    .bind(block_height)
                    .bind(batch_size.get() as i64)
                    .fetch_all(&*pool)
                    .await?;

                if transactions.is_empty() {
                    break;
                }

                let events: Vec<DustNullifierTransactionEvent> = transactions
                    .into_iter()
                    .filter_map(|(_id, _hash, _protocol_version, _transaction_result, raw, _merkle_tree_root, _start_index, _end_index, _paid_fees, _estimated_fees, block_id)| {
                        // Check if transaction contains any matching nullifier prefixes
                        let matching_prefixes: Vec<String> = prefixes.iter()
                            .filter(|prefix| {
                                // Simple prefix matching on raw transaction data
                                let prefix_bytes = const_hex::decode(prefix).unwrap_or_default();
                                raw.windows(prefix_bytes.len()).any(|window| window == prefix_bytes)
                            })
                            .cloned()
                            .collect();

                        if matching_prefixes.is_empty() {
                            None
                        } else {
                            block_height = block_id + 1;

                            // This is a placeholder - in real implementation, we'd build a proper Transaction object
                            Some(DustNullifierTransactionEvent::DustNullifierTransactionProgress(
                                crate::domain::DustNullifierTransactionProgress {
                                    highest_block: block_id as u32,
                                    matched_count: matching_prefixes.len() as u32,
                                }
                            ))
                        }
                    })
                    .collect();

                yield events;
            }
        };

        flatten_chunks(chunks)
    }

    fn get_dust_commitments(
        &self,
        commitment_prefixes: &[String],
        _min_prefix_length: usize,
        start_index: i64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<DustCommitmentEvent, sqlx::Error>> {
        let commitment_prefixes = commitment_prefixes.to_owned();
        let pool = self.pool.clone();

        let chunks = try_stream! {
            let mut commitment_index = start_index;

            loop {
                let query = indoc! {"
                    SELECT commitment, nullifier, initial_value, owner, nonce, ctime, spent_at_transaction_id
                    FROM dust_utxos
                    WHERE id >= $1
                    ORDER BY id
                    LIMIT $2
                "};

                let commitments = sqlx::query_as::<_, (Vec<u8>, Option<Vec<u8>>, Vec<u8>, Vec<u8>, Vec<u8>, i64, Option<i64>)>(query)
                    .bind(commitment_index)
                    .bind(batch_size.get() as i64)
                    .fetch_all(&*pool)
                    .await?;

                if commitments.is_empty() {
                    break;
                }

                let events: Vec<DustCommitmentEvent> = commitments
                    .into_iter()
                    .filter_map(|(commitment, nullifier, initial_value, owner, nonce, ctime, spent_at_transaction_id)| {
                        // Check if commitment matches any prefix
                        let commitment_hex = const_hex::encode(&commitment);
                        let matches_prefix = commitment_prefixes.iter()
                            .any(|prefix| commitment_hex.starts_with(prefix));

                        if matches_prefix {
                            commitment_index += 1;

                            let commitment = indexer_common::domain::DustCommitment::try_from(commitment).ok()?;
                            let owner = indexer_common::domain::DustOwner::try_from(owner).ok()?;
                            let nonce = indexer_common::domain::DustNonce::try_from(nonce).ok()?;
                            let nullifier = nullifier.and_then(|n| indexer_common::domain::DustNullifier::try_from(n).ok());
                            let initial_value = u128::from_be_bytes(
                                initial_value.try_into().ok()?
                            );

                            Some(DustCommitmentEvent::DustCommitment(
                                crate::domain::DustCommitment {
                                    commitment,
                                    nullifier,
                                    value: initial_value,
                                    owner,
                                    nonce,
                                    created_at: ctime,
                                    spent_at: spent_at_transaction_id.map(|_| ctime), // Placeholder
                                }
                            ))
                        } else {
                            None
                        }
                    })
                    .collect();

                yield events;
            }
        };

        flatten_chunks(chunks)
    }

    fn get_registration_updates(
        &self,
        addresses: &[(AddressType, String)],
        from_timestamp: i64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<RegistrationUpdateEvent, sqlx::Error>> {
        let addresses = addresses.to_owned();
        let pool = self.pool.clone();

        let chunks = try_stream! {
            let mut timestamp = from_timestamp;

            loop {
                let query = indoc! {"
                    SELECT cardano_address, dust_address, is_valid, registered_at, removed_at
                    FROM cnight_registrations
                    WHERE registered_at >= $1
                    ORDER BY registered_at
                    LIMIT $2
                "};

                let registrations = sqlx::query_as::<_, (Vec<u8>, Vec<u8>, bool, i64, Option<i64>)>(query)
                    .bind(timestamp)
                    .bind(batch_size.get() as i64)
                    .fetch_all(&*pool)
                    .await?;

                if registrations.is_empty() {
                    break;
                }

                let events: Vec<RegistrationUpdateEvent> = registrations
                    .into_iter()
                    .filter_map(|(cardano_address, dust_address, is_valid, registered_at, _removed_at)| {
                        let cardano_address_hex = const_hex::encode(&cardano_address);
                        let dust_address_hex = const_hex::encode(&dust_address);

                        // Check if any of the requested addresses match this registration
                        let matches = addresses.iter().any(|(addr_type, addr_value)| {
                            match addr_type {
                                AddressType::CardanoStake => addr_value == &cardano_address_hex,
                                AddressType::Dust => addr_value == &dust_address_hex,
                                AddressType::Night => false, // Not implemented yet
                            }
                        });

                        if matches {
                            timestamp = registered_at + 1;

                            Some(RegistrationUpdateEvent::RegistrationUpdate(
                                crate::domain::RegistrationUpdate {
                                    address_type: AddressType::CardanoStake,
                                    address_value: cardano_address_hex.clone(),
                                    related_addresses: crate::domain::RelatedAddresses {
                                        night_address: None,
                                        dust_address: Some(dust_address_hex),
                                        cardano_stake_key: Some(cardano_address_hex),
                                    },
                                    is_active: is_valid,
                                    timestamp: registered_at,
                                }
                            ))
                        } else {
                            None
                        }
                    })
                    .collect();

                yield events;
            }
        };

        flatten_chunks(chunks)
    }
}
