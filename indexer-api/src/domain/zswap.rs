// This file is part of midnight-indexer.
// Copyright (C) 2025 Midnight Foundation
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
use indexer_common::{
    domain::{NetworkId, ProtocolVersion, ZswapStateStorage},
    error::BoxError,
    serialize::SerializableExt,
};
use log::debug;
use midnight_ledger::transient_crypto::merkle_tree;
use std::{io, sync::Arc};
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Debug, Default, Clone)]
pub struct ZswapStateCache(Arc<RwLock<ZswapState>>);

impl ZswapStateCache {
    pub async fn collapsed_update(
        &self,
        start_index: u64,
        end_index: u64,
        network_id: NetworkId,
        protocol_version: ProtocolVersion,
        zswap_state_storage: &impl ZswapStateStorage,
    ) -> Result<MerkleTreeCollapsedUpdate, ZswapStateCacheError> {
        // Acquire a read lock
        let mut zswap_state_read = self.0.read().await;

        // Check if the current zswap state is stale and needs to be updated.
        if end_index >= zswap_state_read.first_free() {
            debug!(
                end_index,
                first_free = zswap_state_read.first_free;
                "zswap state is stale"
            );

            // Release the read lock.
            drop(zswap_state_read);

            // Acquire a write lock.
            let mut zswap_state_write = self.0.write().await;
            debug!("acquired write lock");

            // Check if the state has been updated in the meantime.
            if end_index >= zswap_state_write.first_free() {
                debug!(
                    end_index,
                    first_free = zswap_state_write.first_free;
                    "zswap state is still stale, loading"
                );

                let zswap_state = zswap_state_storage
                    .load_zswap_state()
                    .await
                    .map_err(|error| ZswapStateCacheError::Load(error.into()))?
                    .map(|(zswap_state, _)| zswap_state);

                debug!("zswap state loaded");

                match zswap_state {
                    Some(zswap_state) => {
                        let zswap_state = indexer_common::domain::ZswapState::deserialize(
                            zswap_state,
                            network_id,
                        )?
                        .into();

                        *zswap_state_write = zswap_state;
                    }

                    None => return Err(ZswapStateCacheError::NotFound),
                }
            }

            zswap_state_read = zswap_state_write.downgrade();
        }

        debug!(start_index, end_index; "creating collapsed update");

        let collapsed_update = zswap_state_read.collapsed_update(
            protocol_version,
            start_index,
            end_index,
            network_id,
        )?;

        Ok(collapsed_update)
    }
}

#[derive(Debug, Error)]
pub enum ZswapStateCacheError {
    #[error("cannot load zswap state")]
    Load(#[source] BoxError),

    #[error("no zswap state stored")]
    NotFound,

    #[error("cannot deserialize zswap state")]
    Serialize(#[from] io::Error),

    #[error(transparent)]
    TrimMerkleTree(#[from] ZswapStateError),
}

/// Wrapper around ZswapState from indexer_common.
#[derive(Debug, Clone, Default, From, Deref)]
pub struct ZswapState(indexer_common::domain::ZswapState);

impl ZswapState {
    pub fn first_free(&self) -> u64 {
        self.first_free
    }

    /// Produce a collapsed Merkle Tree from this [ZswapState].
    pub fn collapsed_update(
        &self,
        protocol_version: ProtocolVersion,
        start_index: u64,
        end_index: u64,
        network_id: NetworkId,
    ) -> Result<MerkleTreeCollapsedUpdate, ZswapStateError> {
        let update =
            merkle_tree::MerkleTreeCollapsedUpdate::new(&self.coin_coms, start_index, end_index)?
                .serialize(network_id)?;

        Ok(MerkleTreeCollapsedUpdate {
            protocol_version,
            start_index,
            end_index,
            update,
        })
    }
}

#[derive(Debug, Error)]
pub enum ZswapStateError {
    #[error("cannot create merkle-tree collapsed update")]
    MerkleTree(#[from] merkle_tree::InvalidUpdate),

    #[error("cannot serialize merkle-tree collapsed update")]
    Serialize(#[from] io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MerkleTreeCollapsedUpdate {
    pub protocol_version: ProtocolVersion,
    pub start_index: u64,
    pub end_index: u64,
    pub update: Vec<u8>,
}
