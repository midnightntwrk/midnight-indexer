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

use crate::domain::{
    ByteVec, DustCommitment, DustNonce, DustNullifier, DustOwner, NightUtxoHash, NightUtxoNonce,
    TransactionHash,
};
use serde::{Deserialize, Serialize};

/// DUST event for the indexer domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DustEvent {
    pub transaction_hash: TransactionHash,
    pub logical_segment: u16,
    pub physical_segment: u16,
    pub event_details: DustEventDetails,
}

/// DUST event details.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DustEventDetails {
    /// Initial DUST UTXO creation.
    DustInitialUtxo {
        /// Qualified DUST output.
        output: QualifiedDustOutput,
        /// Generation information.
        generation: DustGenerationInfo,
        /// Merkle tree index for generation.
        generation_index: u64,
    },

    /// DUST generation time update (when backing Night is spent).
    DustGenerationDtimeUpdate {
        /// Updated generation information.
        generation: DustGenerationInfo,
        /// Merkle tree index for generation.
        generation_index: u64,
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

/// Registration mapping between Cardano address and DUST address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DustRegistration {
    /// Cardano address (Night holder).
    pub cardano_address: ByteVec,

    /// DUST address (where DUST is sent).
    pub dust_address: DustOwner,

    /// Whether this registration is currently valid (only one per Cardano address).
    pub is_valid: bool,

    /// When this registration was created.
    pub registered_at: u64,

    /// When this registration was removed (if applicable).
    pub removed_at: Option<u64>,
}

/// DUST UTXO state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DustUtxo {
    /// DUST commitment.
    pub commitment: DustCommitment,

    /// DUST nullifier (when spent).
    pub nullifier: Option<DustNullifier>,

    /// Initial value.
    pub initial_value: u128,

    /// Owner's DUST public key.
    pub owner: DustOwner,

    /// UTXO nonce.
    pub nonce: DustNonce,

    /// Sequence number.
    pub seq: u32,

    /// Creation time.
    pub ctime: u64,

    /// Reference to generation info.
    pub generation_info_id: Option<u64>,

    /// Transaction where this was spent.
    pub spent_at_transaction_id: Option<u64>,
}

/// DUST event type for database storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "DUST_EVENT_TYPE", rename_all = "PascalCase")]
pub enum DustEventType {
    /// Initial DUST UTXO creation.
    DustInitialUtxo,

    /// DUST generation time update.
    DustGenerationDtimeUpdate,

    /// DUST spend processed.
    DustSpendProcessed,
}

impl From<&DustEventDetails> for DustEventType {
    fn from(details: &DustEventDetails) -> Self {
        match details {
            DustEventDetails::DustInitialUtxo { .. } => Self::DustInitialUtxo,
            DustEventDetails::DustGenerationDtimeUpdate { .. } => Self::DustGenerationDtimeUpdate,
            DustEventDetails::DustSpendProcessed { .. } => Self::DustSpendProcessed,
        }
    }
}
