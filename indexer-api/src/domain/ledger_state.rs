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

use derive_more::derive::{Deref, From};
use indexer_common::domain::{
    ByteVec, LedgerVersion, ProtocolVersion, SerializedLedgerStateKey, ledger,
};
use log::debug;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Cache for the latest ledger state, together with the height of the block it belongs to.
///
/// Generic over the cached value only to unit-test the reloading without a real [LedgerState].
#[derive(Debug)]
pub struct LedgerStateCache<T = LedgerState>(RwLock<Option<(Arc<T>, u32)>>);

impl<T> Default for LedgerStateCache<T> {
    fn default() -> Self {
        Self(RwLock::default())
    }
}

impl LedgerStateCache {
    /// Return a ledger state at least as new as the one of the block at `height`: the cached one,
    /// first replaced with the one stored under `key` if that is newer.
    ///
    /// Freshness is decided by `height`, because a different `key` does not tell which ledger
    /// state is newer. The cache never moves back, so a caller that read `height` before a new
    /// block was indexed gets the newer ledger state, which still covers its range. This assumes
    /// the tip height never decreases, so indexer-api must be stopped for a reindex.
    pub async fn at_least(
        &self,
        height: u32,
        key: &SerializedLedgerStateKey,
        ledger_version: LedgerVersion,
    ) -> Result<Arc<LedgerState>, ledger::Error> {
        self.load_if_stale(height, || LedgerState::load(key, ledger_version))
            .await
    }
}

impl<T> LedgerStateCache<T> {
    /// Return the cached value, first replacing it with the one returned by `load` if `height` is
    /// newer than the height of the cached value. If `load` fails, the cached value is kept and the
    /// error is returned.
    ///
    /// The lock is only held to check and replace the cached value, and a cached value that is
    /// current only needs the read lock, so callers use the returned value without blocking each
    /// other or a concurrent reload. A replaced value is dropped once its last caller is done.
    async fn load_if_stale<E, F: FnOnce() -> Result<T, E>>(
        &self,
        height: u32,
        load: F,
    ) -> Result<Arc<T>, E> {
        let current = |cached: &Option<(Arc<T>, u32)>| {
            cached
                .as_ref()
                .filter(|(_, cached_height)| !needs_reload(Some(*cached_height), height))
                .map(|(value, _)| value.clone())
        };

        // Acquire a read lock and return the cached value if it is current.
        if let Some(value) = current(&*self.0.read().await) {
            return Ok(value);
        }

        // Acquire a write lock and check again, because the cached value could have been replaced
        // in the meantime.
        let mut cached = self.0.write().await;
        if let Some(value) = current(&cached) {
            return Ok(value);
        }

        debug!(height; "outdated ledger state, loading from storage");
        let value = Arc::new(load()?);
        *cached = Some((value.clone(), height));

        Ok(value)
    }
}

/// Whether a cached ledger state of the block at `cached_height` must be reloaded to serve the
/// ledger state of the block at `height`; see [LedgerStateCache::at_least].
///
/// Tree sizes are not enough to decide this, because some updates, e.g. a NIGHT spend changing a
/// dust generation leaf, modify a tree in place without changing its first free index.
fn needs_reload(cached_height: Option<u32>, height: u32) -> bool {
    cached_height.is_none_or(|cached_height| height > cached_height)
}

/// Wrapper around LedgerState from indexer_common.
#[derive(Debug, Clone, From, Deref)]
pub struct LedgerState(ledger::LedgerState);

impl LedgerState {
    pub fn load(
        key: &SerializedLedgerStateKey,
        ledger_version: LedgerVersion,
    ) -> Result<Self, indexer_common::domain::ledger::Error> {
        indexer_common::domain::ledger::LedgerState::load(key, ledger_version).map(Into::into)
    }

    /// Whether the root node of a previously persisted ledger state is still present in the
    /// ledger DB, i.e. the state is loadable and has not been garbage collected.
    pub fn root_loadable(
        key: &SerializedLedgerStateKey,
        ledger_version: LedgerVersion,
    ) -> Result<bool, indexer_common::domain::ledger::Error> {
        indexer_common::domain::ledger::LedgerState::root_loadable(key, ledger_version)
    }

    /// Create a zswap state Merkle tree collapsed update.
    pub fn make_zswap_collapsed_update(
        &self,
        start_index: u64,
        end_index: u64,
        protocol_version: ProtocolVersion,
    ) -> Result<MerkleTreeCollapsedUpdate, ledger::Error> {
        debug!(start_index, end_index; "creating zswap collapsed update");

        let update = self.0.make_zswap_collapsed_update(start_index, end_index)?;

        Ok(MerkleTreeCollapsedUpdate {
            start_index,
            end_index,
            update,
            protocol_version,
        })
    }

    /// Produce a collapsed Merkle Tree for the dust generations tree.
    pub fn dust_generations_collapsed_update(
        &self,
        start_index: u64,
        end_index: u64,
        protocol_version: ProtocolVersion,
    ) -> Result<MerkleTreeCollapsedUpdate, ledger::Error> {
        debug!(start_index, end_index; "creating dust generations collapsed update");

        let update = self
            .0
            .dust_generations_collapsed_update(start_index, end_index)?;

        Ok(MerkleTreeCollapsedUpdate {
            start_index,
            end_index,
            update,
            protocol_version,
        })
    }

    /// Produce a collapsed Merkle Tree for the dust commitments tree.
    pub fn dust_commitments_collapsed_update(
        &self,
        start_index: u64,
        end_index: u64,
        protocol_version: ProtocolVersion,
    ) -> Result<MerkleTreeCollapsedUpdate, ledger::Error> {
        debug!(start_index, end_index; "creating dust commitments collapsed update");

        let update = self
            .0
            .dust_commitments_collapsed_update(start_index, end_index)?;

        Ok(MerkleTreeCollapsedUpdate {
            start_index,
            end_index,
            update,
            protocol_version,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MerkleTreeCollapsedUpdate {
    pub start_index: u64,
    pub end_index: u64,
    pub update: ByteVec,
    pub protocol_version: ProtocolVersion,
}

#[cfg(test)]
mod tests {
    use crate::domain::ledger_state::{LedgerStateCache, needs_reload};
    use std::{cell::Cell, convert::Infallible, sync::Arc};
    use tokio::sync::RwLock;

    #[test]
    fn test_needs_reload_nothing_cached() {
        assert!(needs_reload(None, 42));
    }

    #[test]
    fn test_needs_reload_newer() {
        // A newer ledger state exists, e.g. after a NIGHT spend updated a dust generation leaf in
        // place without changing the first free index of the tree.
        assert!(needs_reload(Some(42), 43));
    }

    #[test]
    fn test_needs_reload_same() {
        // The cached ledger state is the latest one.
        assert!(!needs_reload(Some(42), 42));
    }

    #[test]
    fn test_needs_reload_older() {
        // The caller fetched the latest ledger state before a new block arrived and the cache was
        // meanwhile reloaded with the newer one; it must not move back.
        assert!(!needs_reload(Some(43), 42));
    }

    fn cache_with(value: &'static str, height: u32) -> LedgerStateCache<&'static str> {
        LedgerStateCache(RwLock::new(Some((Arc::new(value), height))))
    }

    #[tokio::test]
    async fn test_load_if_stale_nothing_cached() {
        let cache = LedgerStateCache::<&str>::default();
        let loads = Cell::new(0);

        let cached = cache
            .load_if_stale(42, || {
                loads.set(loads.get() + 1);
                Ok::<_, Infallible>("state-42")
            })
            .await
            .expect("load succeeds");

        assert_eq!(*cached, "state-42");
        assert_eq!(*cache.0.read().await, Some((Arc::new("state-42"), 42)));
        assert_eq!(loads.get(), 1);
    }

    #[tokio::test]
    async fn test_load_if_stale_same_or_older() {
        let cache = cache_with("state-42", 42);
        let loads = Cell::new(0);

        for height in [42, 41] {
            let cached = cache
                .load_if_stale(height, || {
                    loads.set(loads.get() + 1);
                    Ok::<_, Infallible>("unexpected")
                })
                .await
                .expect("load succeeds");

            assert_eq!(*cached, "state-42");
            assert_eq!(*cache.0.read().await, Some((Arc::new("state-42"), 42)));
        }
        assert_eq!(loads.get(), 0);
    }

    #[tokio::test]
    async fn test_load_if_stale_newer() {
        let cache = cache_with("state-42", 42);
        let loads = Cell::new(0);

        let cached = cache
            .load_if_stale(43, || {
                loads.set(loads.get() + 1);
                Ok::<_, Infallible>("state-43")
            })
            .await
            .expect("load succeeds");

        assert_eq!(*cached, "state-43");
        assert_eq!(*cache.0.read().await, Some((Arc::new("state-43"), 43)));
        assert_eq!(loads.get(), 1);
    }

    #[tokio::test]
    async fn test_load_if_stale_error_keeps_cached() {
        let cache = cache_with("state-42", 42);

        let result = cache.load_if_stale(43, || Err("cannot load")).await;
        assert_eq!(result.err(), Some("cannot load"));

        // The previous value is still cached and the failed height is retried on the next request.
        let cached = cache
            .load_if_stale(42, || Ok::<_, &str>("unexpected"))
            .await
            .expect("load succeeds");
        assert_eq!(*cached, "state-42");
        assert_eq!(*cache.0.read().await, Some((Arc::new("state-42"), 42)));

        let cached = cache
            .load_if_stale(43, || Ok::<_, &str>("state-43"))
            .await
            .expect("load succeeds");
        assert_eq!(*cached, "state-43");
        assert_eq!(*cache.0.read().await, Some((Arc::new("state-43"), 43)));
    }
}
