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

pub use crate::domain::{DustCommitment, DustNullifier};

use crate::domain::{DustNonce, DustOwner, NightUtxoHash, NightUtxoNonce, TransactionHash};
use serde::{Deserialize, Serialize};

/// DUST event for the indexer domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DustEvent {
    pub transaction_hash: TransactionHash,
    pub logical_segment: u16,
    pub physical_segment: u16,
    pub event_details: DustEventAttributes,
}

/// DUST event details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DustEventAttributes {
    /// Initial DUST UTXO creation.
    DustInitialUtxo {
        /// Qualified DUST output.
        output: QualifiedDustOutput,
        /// Generation information.
        generation_info: DustGenerationInfo,
        /// Merkle tree index for generation.
        generation_index: u64,
    },

    /// DUST generation time update (when backing Night is spent).
    DustGenerationDtimeUpdate {
        /// Updated generation information.
        generation_info: DustGenerationInfo,
        /// Merkle tree index for generation.
        generation_index: u64,
        /// Merkle tree path for this update.
        merkle_path: Vec<DustMerklePathEntry>,
    },

    /// DUST spend processed.
    DustSpendProcessed {
        /// DUST commitment.
        commitment: DustCommitment,
        /// Commitment merkle tree index.
        commitment_index: u64,
        /// DUST nullifier.
        nullifier: DustNullifier,
        /// Fee amount paid.
        v_fee: u128,
        /// Timestamp of spend.
        time: u64,
        /// DUST parameters.
        params: DustParameters,
    },
}

/// Qualified DUST output information.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualifiedDustOutput {
    /// Initial value of DUST UTXO.
    pub initial_value: u128,

    /// Owner's DUST public key.
    pub owner: DustOwner,

    /// Nonce for this DUST UTXO.
    pub nonce: DustNonce,

    /// Sequence number.
    pub seq: u32,

    /// Creation time.
    pub ctime: u64,

    /// Backing Night UTXO nonce.
    pub backing_night: NightUtxoNonce,

    /// Merkle tree index.
    pub mt_index: u64,
}

/// Merkle tree path entry for DUST trees.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DustMerklePathEntry {
    /// The hash of the sibling at this level (if available).
    pub sibling_hash: Option<Vec<u8>>,
    /// Whether the path goes left at this level.
    pub goes_left: bool,
}

/// DUST generation information.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DustGenerationInfo {
    /// Hash of the backing Night UTXO.
    pub night_utxo_hash: NightUtxoHash,

    /// Value of backing Night UTXO.
    pub value: u128,

    /// Owner's DUST public key.
    pub owner: DustOwner,

    /// Initial nonce.
    pub nonce: DustNonce,

    /// Creation time.
    pub ctime: u64,

    /// Decay time (when Night is spent).
    pub dtime: u64,
}

/// DUST parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DustParameters {
    /// Night to DUST ratio.
    pub night_dust_ratio: u64,

    /// Generation decay rate.
    pub generation_decay_rate: u32,

    /// DUST grace period in seconds.
    pub dust_grace_period: u64,
}

impl Default for DustParameters {
    fn default() -> Self {
        // Initial DUST parameters from the ledger.
        Self {
            // 5 DUST per NIGHT.
            night_dust_ratio: 5_000_000_000,
            // Works out to a generation time of approximately 1 week.
            generation_decay_rate: 8_267,
            // 3 hours in seconds.
            dust_grace_period: 3 * 60 * 60,
        }
    }
}

/// DUST event type for database storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "DUST_EVENT_TYPE", rename_all = "PascalCase")]
pub enum DustEventVariant {
    /// Initial DUST UTXO creation.
    DustInitialUtxo,

    /// DUST generation time update.
    DustGenerationDtimeUpdate,

    /// DUST spend processed.
    DustSpendProcessed,
}

impl From<&DustEventAttributes> for DustEventVariant {
    fn from(details: &DustEventAttributes) -> Self {
        match details {
            DustEventAttributes::DustInitialUtxo { .. } => Self::DustInitialUtxo,
            DustEventAttributes::DustGenerationDtimeUpdate { .. } => {
                Self::DustGenerationDtimeUpdate
            }
            DustEventAttributes::DustSpendProcessed { .. } => Self::DustSpendProcessed,
        }
    }
}
