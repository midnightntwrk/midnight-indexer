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
    domain::storage::Storage,
    infra::api::{
        ApiResult, ContextExt, ResultExt,
        v4::{HexEncodable, HexEncoded},
    },
};
use async_graphql::{Context, SimpleObject, Subscription};
use async_stream::try_stream;
use futures::{Stream, TryStreamExt};
use indexer_common::domain::{BlockIndexed, Subscriber};
use log::{debug, warn};
use std::{marker::PhantomData, pin::pin};

pub struct ShieldedNullifierTransactionsSubscription<S, B> {
    _s: PhantomData<S>,
    _b: PhantomData<B>,
}

impl<S, B> Default for ShieldedNullifierTransactionsSubscription<S, B> {
    fn default() -> Self {
        Self {
            _s: PhantomData,
            _b: PhantomData,
        }
    }
}

/// A transaction containing a shielded (zswap) nullifier match with block context.
#[derive(Debug, Clone, SimpleObject)]
pub struct ShieldedNullifierTransaction {
    /// The hex-encoded matched nullifier.
    pub nullifier: HexEncoded,
    /// The transaction ID (use to query full transaction via `transaction` query).
    pub transaction_id: u64,
    /// The block height containing this transaction.
    pub block_height: u32,
    /// The hex-encoded block hash (use to query block with ledger parameters).
    pub block_hash: HexEncoded,
}

#[Subscription]
impl<S, B> ShieldedNullifierTransactionsSubscription<S, B>
where
    S: Storage,
    B: Subscriber,
{
    /// Subscribe to transactions containing shielded (zswap) nullifiers matching the provided
    /// prefixes. Returns transaction and block references for wallet to fetch full data.
    /// If `toBlock` is specified, the subscription finishes after reaching that block.
    async fn shielded_nullifier_transactions<'a>(
        &self,
        cx: &'a Context<'a>,
        nullifier_prefixes: Vec<HexEncoded>,
        from_block: Option<u64>,
        to_block: Option<u64>,
    ) -> impl Stream<Item = ApiResult<ShieldedNullifierTransaction>> {
        let storage = cx.get_storage::<S>();
        let subscriber = cx.get_subscriber::<B>();
        let batch_size = cx
            .get_subscription_config()
            .shielded_nullifier_transactions
            .batch_size;

        let block_indexed_stream = subscriber.subscribe::<BlockIndexed>();

        try_stream! {
            let prefix_bytes = nullifier_prefixes
                .iter()
                .map(|p| const_hex::decode(p.as_ref()))
                .collect::<Result<Vec<_>, _>>()
                .map_err_into_client_error(|| "invalid hex-encoded nullifier prefix")?;

            let from = from_block.unwrap_or(0);
            let to = to_block.unwrap_or(i64::MAX as u64);

            debug!("streaming existing shielded nullifier transactions");

            let entries = storage
                .get_shielded_nullifier_transactions(&prefix_bytes, from, to, batch_size)
                .await;
            let mut entries = pin!(entries);
            while let Some(entry) = entries
                .try_next()
                .await
                .map_err_into_server_error(|| "get next shielded nullifier transaction")?
            {
                yield ShieldedNullifierTransaction {
                    nullifier: entry.nullifier.hex_encode(),
                    transaction_id: entry.transaction_id,
                    block_height: entry.block_height,
                    block_hash: entry.block_hash.hex_encode(),
                };
            }

            if let Some(to) = to_block {
                let latest_block = storage
                    .get_latest_block()
                    .await
                    .map_err_into_server_error(|| "get latest block")?;

                if let Some(block) = latest_block
                    && block.height as u64 >= to
                {
                    return;
                }
            }

            debug!("streaming live shielded nullifier transactions");
            let mut block_indexed_stream = pin!(block_indexed_stream);
            while block_indexed_stream
                .try_next()
                .await
                .map_err_into_server_error(|| "get next BlockIndexed event")?
                .is_some()
            {
                let entries = storage
                    .get_shielded_nullifier_transactions(&prefix_bytes, from, to, batch_size)
                    .await;
                let mut entries = pin!(entries);
                while let Some(entry) = entries
                    .try_next()
                    .await
                    .map_err_into_server_error(|| "get next shielded nullifier transaction")?
                {
                    yield ShieldedNullifierTransaction {
                        nullifier: entry.nullifier.hex_encode(),
                        transaction_id: entry.transaction_id,
                        block_height: entry.block_height,
                        block_hash: entry.block_hash.hex_encode(),
                    };
                }

                if let Some(to) = to_block {
                    let latest_block = storage
                        .get_latest_block()
                        .await
                        .map_err_into_server_error(|| "get latest block")?;

                    if let Some(block) = latest_block
                        && block.height as u64 >= to
                    {
                        return;
                    }
                }
            }

            warn!("stream of BlockIndexed events completed unexpectedly");
        }
    }
}
