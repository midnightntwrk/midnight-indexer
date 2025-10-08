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
    domain::{dust::DustGenerationStatus, storage::dust::DustStorage},
    infra::storage::Storage,
};
use fastrace::trace;
use indexer_common::{domain::CardanoStakeKey, infra::sqlx::U128BeBytes};
use indoc::indoc;

impl DustStorage for Storage {
    #[trace]
    async fn get_dust_generation_status(
        &self,
        cardano_stake_keys: &[CardanoStakeKey],
    ) -> Result<Vec<DustGenerationStatus>, sqlx::Error> {
        let mut statuses = vec![];

        for stake_key in cardano_stake_keys {
            // Query registration info.
            let registration_query = indoc! {"
                SELECT dust_address, valid
                FROM cnight_registrations
                WHERE cardano_address = $1 AND removed_at IS NULL
                ORDER BY registered_at DESC
                LIMIT 1
            "};

            let result = sqlx::query_as::<_, (Vec<u8>, bool)>(registration_query)
                .bind(stake_key.as_ref())
                .fetch_optional(&*self.pool)
                .await?;

            let (dust_address, registered) = match result {
                Some((addr, valid)) => {
                    let address_array: [u8; 32] = addr
                        .try_into()
                        .map_err(|_| sqlx::Error::Decode("Invalid DUST address length".into()))?;
                    (
                        indexer_common::domain::DustAddress::from(address_array),
                        valid,
                    )
                }
                None => (indexer_common::domain::DustAddress::from([0u8; 32]), false),
            };

            let mut generation_rate = 0u128;
            let mut current_capacity = 0u128;
            let mut night_balance = 0u128;

            // Query active generation info if registered.
            if registered {
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
                    // - Therefore: generation_rate = Stars * 8,267 Specks/second.
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

                    // Current capacity = Stars * generation_decay_rate * elapsed_seconds.
                    // Maximum capacity is limited by night_dust_ratio (5 DUST per NIGHT = 5 * 10^15
                    // Specks per 10^6 Stars).
                    const NIGHT_DUST_RATIO: u128 = 5_000_000_000; // Max Specks per Star.
                    let max_capacity = value_u128.saturating_mul(NIGHT_DUST_RATIO);
                    let generated_capacity = value_u128
                        .saturating_mul(GENERATION_DECAY_RATE)
                        .saturating_mul(elapsed_seconds);
                    current_capacity = generated_capacity.min(max_capacity);
                }
            }

            statuses.push(DustGenerationStatus {
                cardano_stake_key: stake_key.to_owned(),
                dust_address: registered.then_some(dust_address),
                registered,
                night_balance,
                generation_rate,
                current_capacity,
            });
        }

        Ok(statuses)
    }
}
