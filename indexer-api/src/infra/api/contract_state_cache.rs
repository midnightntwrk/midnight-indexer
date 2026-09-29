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

//! Cross-request cache for contract states resolved out of the ledger arena.
//!
//! Serving `state` now means loading a DAG of arena nodes and re-serializing it, which is far more
//! expensive than reading one column. async-graphql's `DataLoader` deduplicates only *within* a
//! request, so this is the layer that stops the same contract state being rebuilt for every
//! request that asks for it. Both layers are wanted: the loader batches, this one persists.
//!
//! Entries are keyed by arena key, which is content-addressed and therefore immutable — the same
//! key can never denote different bytes. Expiry is purely a memory policy, so it is a
//! time-to-*idle*: a state that keeps being asked for is never evicted for age.

use super::{ApiError, ApiResult, OptionExt, ResultExt};
use indexer_common::domain::{
    SerializedContractState, SerializedContractStateKey, SerializedZswapState,
    SerializedZswapStateKey,
    ledger::{ContractZswapState, LedgerDbContractState, LedgerState},
};
use log::debug;
use metrics::{Counter, Histogram, counter, histogram};
use moka::future::Cache;
use serde::Deserialize;
use std::{sync::Arc, time::Duration};
use tokio::sync::Semaphore;

/// Configuration for the [ContractStateCache].
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct ContractStateCacheConfig {
    /// Total size of cached state bytes, applied independently to the contract state and zswap
    /// state caches. Byte-denominated rather than entry-denominated because entries differ in size
    /// by orders of magnitude — a contract state can be around a megabyte — which makes an entry
    /// count meaningless as a memory bound.
    #[serde(with = "byte_unit_serde")]
    max_capacity: u64,

    /// How long an unused entry is kept. Time-to-idle, not time-to-live: the cached value cannot go
    /// stale, because arena keys are content-addressed, so this only bounds memory.
    #[serde(with = "humantime_serde")]
    time_to_idle: Duration,

    /// How many arena loads may run at once. Each load blocks its worker thread — storage-core's
    /// node reads are synchronous under `block_in_place` — and in cloud deployments draws on the
    /// same connection pool as ordinary GraphQL queries, so this keeps an arena-heavy request from
    /// starving them of both.
    max_concurrent_loads: usize,
}

impl ContractStateCacheConfig {
    /// Concurrent arena loads this cache admits. Read by the ledger-query bound, which shares the
    /// same connection pool and blocking threads and so has to budget against it rather than
    /// alongside it (issue #595).
    pub fn max_concurrent_loads(&self) -> usize {
        self.max_concurrent_loads
    }
}

/// Per-process cache of contract and zswap states resolved from the ledger arena, keyed by arena
/// key.
#[derive(Clone)]
pub struct ContractStateCache {
    /// Bytes rather than hex: half the footprint of the hex string, and reusable by any caller
    /// regardless of how it wants to encode. Hex-encoding per request costs microseconds against a
    /// multi-round-trip arena load.
    contract_states: Cache<SerializedContractStateKey, Arc<SerializedContractState>>,

    zswap_states: Cache<SerializedZswapStateKey, Arc<SerializedZswapState>>,

    loads: Arc<Semaphore>,

    metrics: Arc<Metrics>,
}

impl ContractStateCache {
    pub fn new(config: ContractStateCacheConfig) -> Self {
        let ContractStateCacheConfig {
            max_capacity,
            time_to_idle,
            max_concurrent_loads,
        } = config;

        let contract_states = Cache::builder()
            .max_capacity(max_capacity)
            .weigher(|_, state: &Arc<SerializedContractState>| weight(state.len()))
            .time_to_idle(time_to_idle)
            .build();
        let zswap_states = Cache::builder()
            .max_capacity(max_capacity)
            .weigher(|_, state: &Arc<SerializedZswapState>| weight(state.len()))
            .time_to_idle(time_to_idle)
            .build();

        Self {
            contract_states,
            zswap_states,
            loads: Arc::new(Semaphore::new(max_concurrent_loads)),
            metrics: Arc::new(Metrics::default()),
        }
    }

    /// The serialized contract state at the given arena key, from the cache if possible. Concurrent
    /// requests for the same key collapse into a single arena load.
    pub async fn contract_state(
        &self,
        key: &SerializedContractStateKey,
    ) -> ApiResult<Arc<SerializedContractState>> {
        if let Some(state) = self.contract_states.get(key).await {
            self.metrics.hits.increment(1);
            return Ok(state);
        }

        self.contract_states
            .try_get_with(key.to_owned(), async {
                self.metrics.misses.increment(1);
                let _permit = self.permit().await?;

                // Loading a node that is no longer in the ledger DB panics inside storage-core
                // rather than erroring, so establish that it is there first. Unlike a ledger state
                // this is not expected to age out: contract state roots are never unpersisted.
                LedgerState::contract_state_loadable(key)
                    .map_err_into_server_error(|| format!("check contract state {key}"))?
                    .then_some(())
                    .some_or_server_error(|| {
                        format!("contract state {key} is no longer in the ledger DB")
                    })?;

                let started = std::time::Instant::now();
                // Prefetch: re-serializing touches every node in the DAG, and `pre_fetch` issues
                // one batched query per DAG level where faulting nodes in individually is one
                // round trip per node.
                let state = LedgerDbContractState::load_prefetched(key)
                    .and_then(|contract_state| contract_state.serialize())
                    .map_err_into_server_error(|| format!("load contract state {key}"))?;
                self.metrics
                    .load_duration
                    .record(started.elapsed().as_secs_f64());
                debug!(key:%, len = state.len(); "loaded contract state from the ledger arena");

                Ok::<_, ApiError>(Arc::new(state))
            })
            .await
            .map_err(|error| (*error).clone())
    }

    /// The serialized zswap state at the given arena key, from the cache if possible.
    pub async fn zswap_state(
        &self,
        key: &SerializedZswapStateKey,
    ) -> ApiResult<Arc<SerializedZswapState>> {
        if let Some(state) = self.zswap_states.get(key).await {
            self.metrics.hits.increment(1);
            return Ok(state);
        }

        self.zswap_states
            .try_get_with(key.to_owned(), async {
                self.metrics.misses.increment(1);
                let _permit = self.permit().await?;

                LedgerState::contract_zswap_state_loadable(key)
                    .map_err_into_server_error(|| format!("check contract zswap state {key}"))?
                    .then_some(())
                    .some_or_server_error(|| {
                        format!("contract zswap state {key} is no longer in the ledger DB")
                    })?;

                let started = std::time::Instant::now();
                let state = ContractZswapState::load_prefetched(key)
                    .and_then(|zswap_state| zswap_state.serialize())
                    .map_err_into_server_error(|| format!("load contract zswap state {key}"))?;
                self.metrics
                    .load_duration
                    .record(started.elapsed().as_secs_f64());

                Ok::<_, ApiError>(Arc::new(state))
            })
            .await
            .map_err(|error| (*error).clone())
    }

    async fn permit(&self) -> ApiResult<tokio::sync::SemaphorePermit<'_>> {
        self.loads
            .acquire()
            .await
            .map_err_into_server_error(|| "acquire contract state load permit")
    }
}

/// State-bytes metrics for the cache. `Arc`ed so cloning the cache does not re-register them.
struct Metrics {
    hits: Counter,
    misses: Counter,
    load_duration: Histogram,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            hits: counter!("indexer_contract_state_cache_hits"),
            misses: counter!("indexer_contract_state_cache_misses"),
            load_duration: histogram!("indexer_contract_state_load_duration_seconds"),
        }
    }
}

/// moka weights are `u32`; a state larger than 4 GiB is not representable and not possible, so
/// saturating is fine.
fn weight(len: usize) -> u32 {
    len.try_into().unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use crate::infra::api::contract_state_cache::{ContractStateCache, ContractStateCacheConfig};
    use indexer_common::domain::SerializedContractStateKey;
    use std::time::Duration;

    fn config() -> ContractStateCacheConfig {
        ContractStateCacheConfig {
            max_capacity: 16 * 1024 * 1024,
            time_to_idle: Duration::from_secs(60),
            max_concurrent_loads: 4,
        }
    }

    /// A key with an unrecognised tag must be refused while still just bytes, before anything
    /// touches the arena — `get_lazy` would hand back a lazy pointer and panic only on use.
    #[tokio::test]
    async fn unloadable_key_errors_instead_of_panicking() {
        let cache = ContractStateCache::new(config());
        let key = SerializedContractStateKey::from(b"not a contract state key".to_vec());

        assert!(cache.contract_state(&key).await.is_err());
    }

    /// Failed loads must not be cached: moka's `try_get_with` only stores successes, so a later
    /// attempt for the same key must run the loader again rather than returning a stale error.
    #[tokio::test]
    async fn failed_loads_are_not_cached() {
        let cache = ContractStateCache::new(config());
        let key = SerializedContractStateKey::from(b"not a contract state key".to_vec());

        assert!(cache.contract_state(&key).await.is_err());
        assert!(cache.contract_state(&key).await.is_err());

        cache.contract_states.run_pending_tasks().await;
        assert_eq!(cache.contract_states.entry_count(), 0);
    }
}
