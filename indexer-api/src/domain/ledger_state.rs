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

use crate::domain::storage::Storage;
use derive_more::derive::{Deref, From};
use indexer_common::domain::{
    ByteVec, LedgerVersion, ProtocolVersion, SerializedLedgerStateKey, ledger,
};
use log::debug;
use thiserror::Error;
use tokio::sync::{RwLock, RwLockReadGuard};

/// Cache for the latest ledger state, together with the height of the block it belongs to.
#[derive(Debug, Default)]
pub struct LedgerStateCache(RwLock<Option<(LedgerState, u32)>>);

impl LedgerStateCache {
    /// Create a zswap state Merkle tree collapsed update. Only load the ledger state if it is
    /// stale.
    pub async fn make_zswap_collapsed_update(
        &self,
        start_index: u64,
        end_index: u64,
        storage: &impl Storage,
        protocol_version: ProtocolVersion,
    ) -> Result<MerkleTreeCollapsedUpdate, LedgerStateCacheError> {
        let ledger_state = self
            .load_if_stale(end_index, storage, |s| s.zswap_first_free())
            .await?;

        debug!(start_index, end_index; "creating zswap collapsed update");

        let update = ledger_state
            .as_ref()
            .map(|(s, _)| s)
            .expect("ledger state should exist after loading")
            .make_zswap_collapsed_update(start_index, end_index, protocol_version)?;

        Ok(update)
    }

    /// Create a collapsed update for the dust generations tree.
    pub async fn dust_generations_collapsed_update(
        &self,
        start_index: u64,
        end_index: u64,
        storage: &impl Storage,
        protocol_version: ProtocolVersion,
    ) -> Result<MerkleTreeCollapsedUpdate, LedgerStateCacheError> {
        let ledger_state = self
            .load_if_stale(end_index, storage, |s| s.dust_generations_first_free())
            .await?;

        debug!(start_index, end_index; "creating dust generations collapsed update");

        let collapsed_update = ledger_state
            .as_ref()
            .map(|(s, _)| s)
            .expect("ledger state should exist after loading")
            .dust_generations_collapsed_update(start_index, end_index, protocol_version)?;

        Ok(collapsed_update)
    }

    /// Create a collapsed update for the dust commitments tree.
    pub async fn dust_commitments_collapsed_update(
        &self,
        start_index: u64,
        end_index: u64,
        storage: &impl Storage,
        protocol_version: ProtocolVersion,
    ) -> Result<MerkleTreeCollapsedUpdate, LedgerStateCacheError> {
        let ledger_state = self
            .load_if_stale(end_index, storage, |s| s.dust_commitments_first_free())
            .await?;

        debug!(start_index, end_index; "creating dust commitments collapsed update");

        let collapsed_update = ledger_state
            .as_ref()
            .map(|(s, _)| s)
            .expect("ledger state should exist after loading")
            .dust_commitments_collapsed_update(start_index, end_index, protocol_version)?;

        Ok(collapsed_update)
    }

    /// Return a read guard on the cached ledger state, first replacing it with the latest ledger
    /// state from storage if `end_index` is at or beyond the first free index of the relevant tree.
    async fn load_if_stale(
        &self,
        end_index: u64,
        storage: &impl Storage,
        first_free: impl Fn(&LedgerState) -> u64,
    ) -> Result<RwLockReadGuard<'_, Option<(LedgerState, u32)>>, LedgerStateCacheError> {
        let is_stale = |cached: &Option<(LedgerState, u32)>| {
            let first_free = cached
                .as_ref()
                .map(|(s, _)| first_free(s))
                .unwrap_or_default();
            end_index >= first_free
        };

        // Acquire a read lock and check if the cached ledger state is stale.
        let ledger_state_read = self.0.read().await;
        if !is_stale(&ledger_state_read) {
            return Ok(ledger_state_read);
        }

        // Release the read lock, acquire a write lock and check again, because the ledger state
        // could have been updated in the meantime.
        drop(ledger_state_read);
        let mut ledger_state_write = self.0.write().await;
        if is_stale(&ledger_state_write) {
            debug!(end_index; "outdated ledger state, loading from storage");

            let Some((latest_height, protocol_version, latest_key)) =
                storage.get_highest_ledger_state().await?
            else {
                return Err(LedgerStateCacheError::NotFound);
            };

            let cached_height = ledger_state_write.as_ref().map(|(_, height)| *height);
            if needs_reload(cached_height, latest_height) {
                let ledger_state =
                    LedgerState::load(&latest_key, protocol_version.ledger_version())?;
                *ledger_state_write = Some((ledger_state, latest_height));
            }
        }

        Ok(ledger_state_write.downgrade())
    }
}

/// Whether a cached ledger state of the block at `cached_height` must be reloaded to serve the
/// latest ledger state of the block at `height`. Once the first free index check has found
/// the cached ledger state stale, it is always replaced with the latest one.
fn needs_reload(_cached_height: Option<u32>, _height: u32) -> bool {
    true
}

#[derive(Debug, Error)]
pub enum LedgerStateCacheError {
    #[error("cannot load ledger state")]
    Load(#[from] sqlx::Error),

    #[error("no ledger state stored")]
    NotFound,

    #[error(transparent)]
    Ledger(#[from] ledger::Error),
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
    use crate::domain::ledger_state::needs_reload;

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
}
