// This file is part of midnight-indexer.
// Copyright (C) Midnight Foundation
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
        dust::{DustGenerationEntry, DustGenerations, DustNullifierTransaction, DustRegistration},
        storage::dust_generations::DustGenerationsStorage,
    },
    infra::storage::Storage,
};
use async_stream::try_stream;
use fastrace::trace;
use futures::Stream;
use indexer_common::{
    domain::{ByteVec, CardanoRewardAddress, LedgerVersion, TimestampMs, TimestampSecs, ledger},
    infra::sqlx::U128BeBytes,
};
use indoc::indoc;
use std::num::NonZeroU32;

impl DustGenerationsStorage for Storage {
    #[trace]
    async fn get_dust_generations(
        &self,
        cardano_reward_addresses: &[CardanoRewardAddress],
        ledger_version: LedgerVersion,
    ) -> Result<Vec<DustGenerations>, sqlx::Error> {
        let dust_params = ledger::dust_parameters(ledger_version)
            .expect("DUST parameters should be available for supported protocol version");
        let generation_decay_rate = dust_params.generation_decay_rate as u128;
        let night_dust_ratio = dust_params.night_dust_ratio as u128;

        let current_time_query = indoc! {"
            SELECT timestamp
            FROM blocks
            ORDER BY height DESC
            LIMIT 1
        "};

        let now = sqlx::query_as::<_, (i64,)>(current_time_query)
            .fetch_optional(&*self.pool)
            .await?
            .map(|(t,)| TimestampMs(t as u64));

        let mut results = Vec::with_capacity(cardano_reward_addresses.len());

        for reward_address in cardano_reward_addresses {
            let registration_query = indoc! {"
                SELECT dust_address, valid, utxo_tx_hash, utxo_output_index
                FROM cnight_registrations
                WHERE cardano_stake_key = $1
                AND removed_at IS NULL
                ORDER BY registered_at DESC
            "};

            let registrations = sqlx::query_as::<_, (ByteVec, bool, Option<Vec<u8>>, Option<i64>)>(
                registration_query,
            )
            .bind(reward_address.as_ref())
            .fetch_all(&*self.pool)
            .await?;

            let mut registration_data = Vec::with_capacity(registrations.len());

            for (dust_address, valid, utxo_tx_hash, utxo_output_index) in registrations {
                let generation_query = indoc! {"
                    SELECT value, ctime
                    FROM dust_generation_info
                    WHERE owner = $1
                    AND dtime IS NULL
                "};

                let generations = sqlx::query_as::<_, (U128BeBytes, i64)>(generation_query)
                    .bind(dust_address.as_ref())
                    .fetch_all(&*self.pool)
                    .await?;

                let mut night_balance = 0u128;
                let mut generation_rate = 0u128;
                let mut max_capacity = 0u128;
                let mut current_capacity = 0u128;

                for (value, ctime_raw) in &generations {
                    let value = u128::from(*value);
                    let ctime = TimestampSecs(*ctime_raw as u64);

                    night_balance = night_balance.saturating_add(value);
                    generation_rate =
                        generation_rate.saturating_add(value.saturating_mul(generation_decay_rate));
                    let gen_max = value.saturating_mul(night_dust_ratio);
                    max_capacity = max_capacity.saturating_add(gen_max);

                    if let Some(now) = now {
                        let elapsed_seconds = now.elapsed_seconds_since(ctime.to_ms());
                        let gen_capacity = value
                            .saturating_mul(generation_decay_rate)
                            .saturating_mul(elapsed_seconds as u128)
                            .min(gen_max);
                        current_capacity = current_capacity.saturating_add(gen_capacity);
                    }
                }

                registration_data.push(DustRegistration {
                    dust_address,
                    valid,
                    night_balance,
                    generation_rate,
                    max_capacity,
                    current_capacity,
                    utxo_tx_hash,
                    utxo_output_index: utxo_output_index.map(|i| i as u32),
                });
            }

            results.push(DustGenerations {
                cardano_reward_address: reward_address.to_owned(),
                registrations: registration_data,
            });
        }

        Ok(results)
    }

    async fn get_dust_generation_entries(
        &self,
        dust_address: &[u8],
        mut start_index: u64,
        end_index: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<DustGenerationEntry, sqlx::Error>> + Send {
        let pool = self.pool.clone();
        let dust_address = dust_address.to_vec();

        try_stream! {
            loop {
                let query = indoc! {"
                    SELECT merkle_index, owner, value, nonce, ctime, transaction_id
                    FROM dust_generation_info
                    WHERE owner = $1
                    AND merkle_index >= $2
                    AND merkle_index <= $3
                    ORDER BY merkle_index
                    LIMIT $4
                "};

                let entries = sqlx::query_as::<_, (i64, ByteVec, U128BeBytes, ByteVec, i64, i64)>(
                    query,
                )
                .bind(&dust_address[..])
                .bind(start_index as i64)
                .bind(end_index as i64)
                .bind(batch_size.get() as i64)
                .fetch_all(&*pool)
                .await?;

                if entries.is_empty() {
                    break;
                }

                for (merkle_index, owner, value, nonce, ctime, transaction_id) in &entries {
                    yield DustGenerationEntry {
                        merkle_index: *merkle_index as u64,
                        owner: owner.clone(),
                        value: u128::from(*value),
                        nonce: nonce.clone(),
                        ctime: *ctime as u64,
                        transaction_id: *transaction_id as u64,
                    };
                }

                let last_index = entries.last().unwrap().0 as u64;
                if last_index >= end_index {
                    break;
                }
                start_index = last_index + 1;
            }
        }
    }

    async fn get_dust_nullifier_transactions(
        &self,
        nullifier_prefixes: &[Vec<u8>],
        from_block: u64,
        to_block: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<DustNullifierTransaction, sqlx::Error>> + Send {
        let pool = self.pool.clone();
        let nullifier_prefixes = nullifier_prefixes.to_vec();

        try_stream! {
            let conditions = nullifier_prefixes
                .iter()
                .map(|prefix| {
                    let mut next_prefix = prefix.clone();
                    if let Some(last) = next_prefix.last_mut() {
                        if *last < 255 {
                            *last += 1;
                        } else {
                            next_prefix.push(0);
                        }
                    }
                    (prefix.clone(), next_prefix)
                })
                .collect::<Vec<_>>();

            let mut cursors = vec![0i64; conditions.len()];

            loop {
                let mut found_any = false;

                for (i, (prefix, next_prefix)) in conditions.iter().enumerate() {
                    let query = indoc! {"
                        SELECT dn.id, dn.nullifier, dn.commitment, t.id, b.height, b.hash
                        FROM dust_nullifiers dn
                        JOIN transactions t ON t.id = dn.transaction_id
                        JOIN blocks b ON b.id = dn.block_id
                        WHERE dn.nullifier >= $1 AND dn.nullifier < $2
                        AND b.height >= $3
                        AND b.height <= $4
                        AND dn.id > $5
                        ORDER BY dn.id
                        LIMIT $6
                    "};

                    let rows = sqlx::query_as::<_, (i64, ByteVec, ByteVec, i64, i64, ByteVec)>(query)
                        .bind(&prefix[..])
                        .bind(&next_prefix[..])
                        .bind(from_block as i64)
                        .bind(to_block as i64)
                        .bind(cursors[i])
                        .bind(batch_size.get() as i64)
                        .fetch_all(&*pool)
                        .await?;

                    if let Some(last) = rows.last() {
                        cursors[i] = last.0;
                        found_any = true;
                    }

                    for (_, nullifier, commitment, transaction_id, block_height, block_hash) in rows {
                        yield DustNullifierTransaction {
                            nullifier,
                            commitment,
                            transaction_id: transaction_id as u64,
                            block_height: block_height as u32,
                            block_hash,
                        };
                    }
                }

                if !found_any {
                    break;
                }
            }
        }
    }
}
