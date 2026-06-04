// This file is part of midnight-indexer.
// Copyright (C) Midnight Foundation
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

//! Cross-subscriber cache for shielded and unshielded progress polling. Multiple
//! subscribers for the same wallet or address reuse one recent query result instead
//! of each hitting the database. Entries are invalidated by pub-sub events with a
//! time-to-live backstop in case pub-sub stalls.

use crate::infra::api::ApiResult;
use futures::{StreamExt, TryStreamExt, stream};
use indexer_common::domain::{
    BlockIndexed, Subscriber, UnshieldedAddress, UnshieldedUtxoIndexed, WalletIndexed,
};
use log::warn;
use moka::future::Cache;
use serde::Deserialize;
use std::{future::Future, time::Duration};
use uuid::Uuid;

/// The cached shielded progress: the highest, highest checked and highest relevant zswap
/// end indices.
type ShieldedIndices = (Option<u64>, Option<u64>, Option<u64>);

/// Configuration for the [`ProgressCache`].
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct ProgressCacheConfig {
    /// Maximum number of entries (the shielded and unshielded caches are bounded separately).
    max_capacity: u64,

    /// How long an entry is served before it is treated as stale, as a backstop to the
    /// event-driven invalidation.
    #[serde(with = "humantime_serde")]
    time_to_live: Duration,
}

/// Per-process cache of the most recent progress query result, keyed by wallet ID for
/// shielded subscriptions and by address for unshielded ones.
#[derive(Clone)]
pub struct ProgressCache {
    shielded: Cache<Uuid, ShieldedIndices>,
    unshielded: Cache<UnshieldedAddress, Option<u64>>,
}

impl ProgressCache {
    pub fn new(config: ProgressCacheConfig) -> Self {
        let shielded = Cache::builder()
            .max_capacity(config.max_capacity)
            .time_to_live(config.time_to_live)
            .build();
        let unshielded = Cache::builder()
            .max_capacity(config.max_capacity)
            .time_to_live(config.time_to_live)
            .build();

        Self {
            shielded,
            unshielded,
        }
    }

    /// Return the cached shielded indices for `wallet_id`, else run `query`, cache and return
    /// its result. Concurrent subscribers for the same wallet within the cache window share a
    /// single database hit.
    pub async fn shielded_indices(
        &self,
        wallet_id: Uuid,
        query: impl Future<Output = ApiResult<ShieldedIndices>>,
    ) -> ApiResult<ShieldedIndices> {
        if let Some(indices) = self.shielded.get(&wallet_id).await {
            return Ok(indices);
        }

        let indices = query.await?;
        self.shielded.insert(wallet_id, indices).await;

        Ok(indices)
    }

    /// Return the cached highest transaction ID for `address`, else run `query`, cache and
    /// return its result.
    pub async fn unshielded_highest_transaction_id(
        &self,
        address: UnshieldedAddress,
        query: impl Future<Output = ApiResult<Option<u64>>>,
    ) -> ApiResult<Option<u64>> {
        if let Some(highest_transaction_id) = self.unshielded.get(&address).await {
            return Ok(highest_transaction_id);
        }

        let highest_transaction_id = query.await?;
        self.unshielded
            .insert(address, highest_transaction_id)
            .await;

        Ok(highest_transaction_id)
    }

    /// Run the background task that invalidates cache entries on relevant pub-sub events: a
    /// `WalletIndexed` invalidates that wallet's shielded entry, an `UnshieldedUtxoIndexed`
    /// invalidates that address, and a `BlockIndexed` invalidates all shielded entries (the
    /// global highest end index advanced). Returns when the event streams end, after which the
    /// cache relies on its time-to-live.
    pub async fn run_invalidation<B>(self, subscriber: B)
    where
        B: Subscriber,
    {
        enum Invalidation {
            AllShielded,
            Shielded(Uuid),
            Unshielded(UnshieldedAddress),
        }

        let blocks = subscriber
            .subscribe::<BlockIndexed>()
            .map_ok(|_| Invalidation::AllShielded);
        let wallets = subscriber
            .subscribe::<WalletIndexed>()
            .map_ok(|event| Invalidation::Shielded(event.wallet_id));
        let utxos = subscriber
            .subscribe::<UnshieldedUtxoIndexed>()
            .map_ok(|event| Invalidation::Unshielded(event.address));

        let mut events = stream::select_all([blocks.boxed(), wallets.boxed(), utxos.boxed()]);
        while let Some(event) = events.next().await {
            match event {
                Ok(Invalidation::AllShielded) => self.shielded.invalidate_all(),
                Ok(Invalidation::Shielded(wallet_id)) => self.shielded.invalidate(&wallet_id).await,
                Ok(Invalidation::Unshielded(address)) => self.unshielded.invalidate(&address).await,

                Err(error) => {
                    warn!(error:%; "progress cache invalidation subscription failed");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::infra::api::{
        ApiError,
        progress_cache::{ProgressCache, ProgressCacheConfig},
    };
    use indexer_common::domain::UnshieldedAddress;
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };
    use uuid::Uuid;

    fn config() -> ProgressCacheConfig {
        ProgressCacheConfig {
            max_capacity: 100,
            time_to_live: Duration::from_secs(60),
        }
    }

    #[tokio::test]
    async fn shielded_dedups_then_requeries_after_invalidation() {
        let cache = ProgressCache::new(config());
        let wallet_id = Uuid::from_u128(1);
        let calls = AtomicUsize::new(0);
        let query = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok::<(Option<u64>, Option<u64>, Option<u64>), ApiError>((Some(1), Some(2), Some(3)))
        };

        let first = cache.shielded_indices(wallet_id, query()).await.unwrap();
        let second = cache.shielded_indices(wallet_id, query()).await.unwrap();
        assert_eq!(first, (Some(1), Some(2), Some(3)));
        assert_eq!(second, first);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "second read should hit the cache"
        );

        cache.shielded.invalidate(&wallet_id).await;
        cache.shielded_indices(wallet_id, query()).await.unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "read after invalidation should re-query"
        );
    }

    #[tokio::test]
    async fn unshielded_dedups_then_requeries_after_invalidation() {
        let cache = ProgressCache::new(config());
        let address = UnshieldedAddress::from([1u8; 32]);
        let calls = AtomicUsize::new(0);
        let query = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok::<Option<u64>, ApiError>(Some(7))
        };

        let first = cache
            .unshielded_highest_transaction_id(address, query())
            .await
            .unwrap();
        cache
            .unshielded_highest_transaction_id(address, query())
            .await
            .unwrap();
        assert_eq!(first, Some(7));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "second read should hit the cache"
        );

        cache.unshielded.invalidate(&address).await;
        cache
            .unshielded_highest_transaction_id(address, query())
            .await
            .unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "read after invalidation should re-query"
        );
    }
}
