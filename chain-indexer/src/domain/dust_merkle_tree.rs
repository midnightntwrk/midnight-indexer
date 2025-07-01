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

use crate::domain::storage::Storage;
use indexer_common::domain::ByteVec;
use log::{debug, info};
use thiserror::Error;

/// Errors that can occur during Merkle tree operations.
#[derive(Debug, Error)]
pub enum MerkleTreeError {
    #[error("merkle tree index out of bounds")]
    IndexOutOfBounds,

    #[error("merkle tree state corrupted")]
    StateCorrupted,

    #[error("database error")]
    Database(#[from] sqlx::Error),
}

/// Type of Merkle tree for DUST.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MerkleTreeType {
    /// Commitment tree for DUST UTXOs.
    Commitment,
    /// Generation tree for Night UTXOs generating DUST.
    Generation,
}

/// Manages DUST Merkle trees for privacy-preserving operations.
pub struct DustMerkleTreeManager<S: Storage> {
    storage: S,
    batch_size: usize,
}

impl<S: Storage> DustMerkleTreeManager<S> {
    /// Create a new Merkle tree manager.
    pub fn new(storage: S, batch_size: usize) -> Self {
        Self {
            storage,
            batch_size,
        }
    }

    /// Update commitment tree with new commitments.
    ///
    /// Assumes ledger provides commitments with correct indices.
    pub async fn update_commitment_tree(
        &self,
        commitments: Vec<(u64, ByteVec)>,
        block_height: u32,
        root: ByteVec,
    ) -> Result<(), MerkleTreeError> {
        if commitments.is_empty() {
            return Ok(());
        }

        info!(
            block_height,
            commitments_count = commitments.len();
            "updating DUST commitment tree"
        );

        // Process in batches for performance.
        for chunk in commitments.chunks(self.batch_size) {
            self.process_commitment_batch(chunk, block_height).await?;
        }

        // Update tree root.
        self.storage
            .update_merkle_tree_state(MerkleTreeType::Commitment, block_height, root.as_ref(), &[])
            .await?;

        Ok(())
    }

    /// Update generation tree with new generation info.
    ///
    /// Assumes ledger provides generation info with correct indices.
    pub async fn update_generation_tree(
        &self,
        generations: Vec<(u64, ByteVec)>,
        block_height: u32,
        root: ByteVec,
    ) -> Result<(), MerkleTreeError> {
        if generations.is_empty() {
            return Ok(());
        }

        info!(
            block_height,
            generations_count = generations.len();
            "updating DUST generation tree"
        );

        // Process in batches for performance.
        for chunk in generations.chunks(self.batch_size) {
            self.process_generation_batch(chunk, block_height).await?;
        }

        // Update tree root.
        self.storage
            .update_merkle_tree_state(MerkleTreeType::Generation, block_height, root.as_ref(), &[])
            .await?;

        Ok(())
    }

    /// Get collapsed Merkle tree update for synchronization.
    ///
    /// Assumes ledger provides collapsed tree format compatible with wallets.
    pub async fn get_collapsed_update(
        &self,
        tree_type: MerkleTreeType,
        start_index: u64,
        end_index: u64,
    ) -> Result<ByteVec, MerkleTreeError> {
        if start_index > end_index {
            return Err(MerkleTreeError::IndexOutOfBounds);
        }

        debug!(
            tree_type:? = tree_type,
            start_index,
            end_index;
            "generating collapsed Merkle tree update"
        );

        // TODO(sean): Implement actual collapsed tree generation once ledger API provides it.
        // TEMPORARY: Mock placeholder - will be replaced with real collapsed tree data from ledger-5.0.0-alpha.3+.
        // This mock return value will be deleted once we have a node image with proper DUST support.
        Ok(ByteVec::from(vec![0u8; 32]))
    }

    async fn process_commitment_batch(
        &self,
        commitments: &[(u64, ByteVec)],
        block_height: u32,
    ) -> Result<(), MerkleTreeError> {
        // TODO(sean): Implement actual commitment storage once schema is finalized.
        debug!(
            block_height,
            batch_size = commitments.len();
            "processing commitment batch"
        );
        Ok(())
    }

    async fn process_generation_batch(
        &self,
        generations: &[(u64, ByteVec)],
        block_height: u32,
    ) -> Result<(), MerkleTreeError> {
        // TODO(sean): Implement actual generation storage once schema is finalized.
        debug!(
            block_height,
            batch_size = generations.len();
            "processing generation batch"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::storage::tests::NoopStorage;

    #[tokio::test]
    async fn test_empty_updates() {
        let storage = NoopStorage;
        let manager = DustMerkleTreeManager::new(storage, 1000);

        // Empty updates should succeed.
        let result = manager
            .update_commitment_tree(vec![], 100, ByteVec::from(vec![0u8; 32]))
            .await;
        assert!(result.is_ok());
    }
}
