// This file is part of midnight-indexer.
// Copyright (C) 2025-2026 Midnight Foundation
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
use tokio::sync::RwLock;

#[derive(Debug, Default)]
pub struct LedgerStateCache(RwLock<Option<LedgerState>>);

impl LedgerStateCache {
    /// Create a collapsed update from the given start index to the given end index for the given
    /// protocol version.
    pub async fn collapsed_update(
        &self,
        start_index: u64,
        end_index: u64,
        storage: &impl Storage,
        protocol_version: ProtocolVersion,
    ) -> Result<MerkleTreeCollapsedUpdate, LedgerStateCacheError> {
        // Acquire a read lock.
        let mut ledger_state_read = self.0.read().await;

        // Check if the current ledger state is stale and needs to be updated.
        let first_free = ledger_state_read
            .as_ref()
            .map(|s| s.zswap_first_free())
            .unwrap_or_default();
        if end_index >= first_free {
            // Release the read lock and acquire a write lock.
            drop(ledger_state_read);
            let mut ledger_state_write = self.0.write().await;

            // Check if the ledger state has been updated in the meantime.
            let first_free = ledger_state_write
                .as_ref()
                .map(|s| s.zswap_first_free())
                .unwrap_or_default();
            if end_index >= first_free {
                debug!(end_index, first_free; "outdated ledger state, loading from storage");

                let Some((protocol_version, ledger_state_key)) =
                    storage.get_highest_ledger_state().await?
                else {
                    return Err(LedgerStateCacheError::NotFound);
                };

                let ledger_state =
                    LedgerState::load(&ledger_state_key, protocol_version.ledger_version())?;
                *ledger_state_write = Some(ledger_state);
            }

            ledger_state_read = ledger_state_write.downgrade();
        }

        debug!(start_index, end_index; "creating collapsed update");

        let collapsed_update = ledger_state_read
            .as_ref()
            .expect("ledger_state is some")
            .collapsed_update(start_index, end_index, protocol_version)?;

        Ok(collapsed_update)
    }
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

    /// Produce a collapsed Merkle Tree from this ledger state.
    pub fn collapsed_update(
        &self,
        start_index: u64,
        end_index: u64,
        protocol_version: ProtocolVersion,
    ) -> Result<MerkleTreeCollapsedUpdate, ledger::Error> {
        let update = self.0.collapsed_update(start_index, end_index)?;

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
