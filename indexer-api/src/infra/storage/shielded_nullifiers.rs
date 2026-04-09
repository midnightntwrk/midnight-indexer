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
        shielded_nullifier::ShieldedNullifierTransaction,
        storage::shielded_nullifiers::ShieldedNullifiersStorage,
    },
    infra::storage::Storage,
};
use async_stream::try_stream;
use futures::Stream;
use indexer_common::domain::ByteVec;
use indoc::indoc;
use std::num::NonZeroU32;

impl ShieldedNullifiersStorage for Storage {
    async fn get_shielded_nullifier_transactions(
        &self,
        nullifier_prefixes: &[Vec<u8>],
        from_block: u64,
        to_block: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<ShieldedNullifierTransaction, sqlx::Error>> + Send {
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
                        SELECT zn.id, zn.nullifier, t.id, b.height, b.hash
                        FROM zswap_nullifiers zn
                        JOIN transactions t ON t.id = zn.transaction_id
                        JOIN blocks b ON b.id = zn.block_id
                        WHERE zn.nullifier >= $1 AND zn.nullifier < $2
                        AND b.height >= $3
                        AND b.height <= $4
                        AND zn.id > $5
                        ORDER BY zn.id
                        LIMIT $6
                    "};

                    let rows = sqlx::query_as::<_, (i64, ByteVec, i64, i64, ByteVec)>(query)
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

                    for (_, nullifier, transaction_id, block_height, block_hash) in rows {
                        yield ShieldedNullifierTransaction {
                            nullifier,
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
