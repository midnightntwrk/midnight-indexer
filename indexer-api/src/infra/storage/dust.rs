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
use indexer_common::{
    domain::{ByteVec, CardanoRewardAddress, dust::DustParameters},
    infra::sqlx::U128BeBytes,
};
use indoc::indoc;

impl DustStorage for Storage {
    #[trace]
    async fn get_dust_generation_status(
        &self,
        cardano_reward_addresses: &[CardanoRewardAddress],
    ) -> Result<Vec<DustGenerationStatus>, sqlx::Error> {
        let dust_params = DustParameters::default();
        let mut statuses = vec![];

        for reward_address in cardano_reward_addresses {
            // Query registration info.
            let registration_query = indoc! {"
                SELECT dust_address, valid
                FROM cnight_registrations
                WHERE cardano_address = $1 AND removed_at IS NULL
                ORDER BY registered_at DESC
                LIMIT 1
            "};

            let (dust_address, registered) =
                sqlx::query_as::<_, (Vec<u8>, bool)>(registration_query)
                    .bind(reward_address.as_ref())
                    .fetch_optional(&*self.pool)
                    .await?
                    .unwrap_or_default();
            let dust_address = ByteVec::from(dust_address);

            let mut generation_rate = 0u128;
            let mut max_capacity = 0u128;
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
                    let value = u128::from(value);
                    night_balance = value;

                    // DUST generation rate = Stars * generation_decay_rate Specks/second.
                    generation_rate =
                        value.saturating_mul(dust_params.generation_decay_rate as u128);

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
                    // Convert from milliseconds to seconds.
                    let elapsed_seconds = ((current_timestamp - ctime).max(0) as u128) / 1000;

                    // Maximum capacity (static cap) = Stars * night_dust_ratio
                    // (5 DUST per NIGHT = 5 * 10^15 Specks per 10^6 Stars).
                    max_capacity = value.saturating_mul(dust_params.night_dust_ratio as u128);

                    // Current capacity (time-dependent) = Stars * generation_decay_rate *
                    // elapsed_seconds. Capped at max_capacity.
                    let generated_capacity = value
                        .saturating_mul(dust_params.generation_decay_rate as u128)
                        .saturating_mul(elapsed_seconds);
                    current_capacity = generated_capacity.min(max_capacity);
                }
            }

            statuses.push(DustGenerationStatus {
                cardano_reward_address: reward_address.to_owned(),
                dust_address: registered.then_some(dust_address),
                registered,
                night_balance,
                generation_rate,
                max_capacity,
                current_capacity,
            });
        }

        Ok(statuses)
    }
}
