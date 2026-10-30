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
    infra::storage::{Storage, prefix_upper_bound},
};
use async_stream::try_stream;
use futures::Stream;
use indexer_common::domain::{ByteVec, TransactionHash};
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
            for prefix in &nullifier_prefixes {
                let upper_bound = prefix_upper_bound(prefix);

                let mut cursor = 0i64;

                loop {
                    let mut query = sqlx::QueryBuilder::new(indoc! {"
                        SELECT zn.id, zn.nullifier, t.id, t.hash, b.height, b.hash
                        FROM zswap_nullifiers zn
                        JOIN transactions t ON t.id = zn.transaction_id
                        JOIN blocks b ON b.id = zn.block_id
                        WHERE zn.nullifier >=
                    "});
                    query.push_bind(prefix);
                    if let Some(upper_bound) = &upper_bound {
                        query.push(" AND zn.nullifier < ").push_bind(upper_bound);
                    }
                    query
                        .push(" AND b.height >= ").push_bind(from_block as i64)
                        .push(" AND b.height <= ").push_bind(to_block.min(i64::MAX as u64) as i64)
                        .push(" AND zn.id > ").push_bind(cursor)
                        .push(" ORDER BY zn.id LIMIT ").push_bind(batch_size.get() as i64);

                    let rows = query
                        .build_query_as::<(i64, ByteVec, i64, TransactionHash, i64, ByteVec)>()
                        .fetch_all(&*pool)
                        .await?;

                    match rows.last() {
                        Some(last) => cursor = last.0,
                        None => break,
                    }

                    for (_, nullifier, transaction_id, transaction_hash, block_height, block_hash) in rows {
                        yield ShieldedNullifierTransaction {
                            transaction_id: transaction_id as u64,
                            transaction_hash,
                            block_hash,
                            block_height: block_height as u32,
                            nullifier,
                        };
                    }
                }
            }
        }
    }
}
