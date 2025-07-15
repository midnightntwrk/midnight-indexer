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

use indexer_common::{
    domain::{
        ByteArray, ByteVec, DustCommitment, DustNonce, DustNullifier, DustOwner, NightUtxoHash,
        NightUtxoNonce, TransactionHash, TransactionResultWithDustEvents,
    },
    infra::sqlx::SqlxOption,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DustProcessingError {
    #[error("Database error during DUST processing")]
    Database(#[from] sqlx::Error),

    #[error("Invalid DUST event data: {0}")]
    InvalidEventData(String),

    #[error("DUST generation info not found for index {0}")]
    GenerationInfoNotFound(u64),
}

/// Qualified DUST output information.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct QualifiedDustOutput {
    /// Initial value of DUST UTXO.
    pub initial_value: u128,

    /// Owner's DUST public key.
    #[cfg_attr(feature = "standalone", sqlx(try_from = "&'a [u8]"))]
    pub owner: ByteArray<32>,

    /// Nonce for this DUST UTXO.
    #[cfg_attr(feature = "standalone", sqlx(try_from = "&'a [u8]"))]
    pub nonce: ByteArray<32>,

    /// Sequence number.
    #[sqlx(try_from = "i64")]
    pub seq: u32,

    /// Creation time.
    #[sqlx(try_from = "i64")]
    pub ctime: u64,

    /// Backing Night UTXO nonce.
    #[cfg_attr(feature = "standalone", sqlx(try_from = "&'a [u8]"))]
    pub backing_night: ByteArray<32>,

    /// Merkle tree index.
    #[sqlx(try_from = "i64")]
    pub mt_index: u64,
}

/// DUST generation information.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct DustGenerationInfo {
    /// Hash of the backing Night UTXO.
    #[cfg_attr(feature = "standalone", sqlx(try_from = "&'a [u8]"))]
    pub night_utxo_hash: ByteArray<32>,

    /// Value of backing Night UTXO.
    pub value: u128,

    /// Owner's DUST public key.
    #[cfg_attr(feature = "standalone", sqlx(try_from = "&'a [u8]"))]
    pub owner: ByteArray<32>,

    /// Initial nonce.
    #[cfg_attr(feature = "standalone", sqlx(try_from = "&'a [u8]"))]
    pub nonce: ByteArray<32>,

    /// Creation time.
    #[sqlx(try_from = "i64")]
    pub ctime: u64,

    /// Decay time (when Night is spent).
    #[sqlx(try_from = "i64")]
    pub dtime: u64,
}

/// DUST UTXO state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct DustUtxo {
    /// DUST commitment.
    #[cfg_attr(feature = "standalone", sqlx(try_from = "&'a [u8]"))]
    pub commitment: ByteArray<32>,

    /// DUST nullifier (when spent).
    #[cfg_attr(
        feature = "standalone",
        sqlx(try_from = "indexer_common::infra::sqlx::SqlxOption<&'a [u8]>")
    )]
    pub nullifier: Option<ByteArray<32>>,

    /// Initial value.
    pub initial_value: u128,

    /// Owner's DUST public key.
    #[cfg_attr(feature = "standalone", sqlx(try_from = "&'a [u8]"))]
    pub owner: ByteArray<32>,

    /// UTXO nonce.
    #[cfg_attr(feature = "standalone", sqlx(try_from = "&'a [u8]"))]
    pub nonce: ByteArray<32>,

    /// Sequence number.
    #[sqlx(try_from = "i64")]
    pub seq: u32,

    /// Creation time.
    #[sqlx(try_from = "i64")]
    pub ctime: u64,

    /// Reference to generation info.
    #[sqlx(try_from = "SqlxOption<i64>")]
    pub generation_info_id: Option<u64>,

    /// Transaction where this was spent.
    #[sqlx(try_from = "SqlxOption<i64>")]
    pub spent_at_transaction_id: Option<u64>,
}

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
        output: QualifiedDustOutputEvent,
        /// Generation information.
        generation: DustGenerationInfoEvent,
        /// Merkle tree index for generation.
        generation_index: u64,
    },

    /// DUST generation time update (when backing Night is spent).
    DustGenerationDtimeUpdate {
        /// Updated generation information.
        generation: DustGenerationInfoEvent,
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

/// Qualified DUST output information from events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualifiedDustOutputEvent {
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

/// DUST generation information from events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DustGenerationInfoEvent {
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

/// DUST UTXO state from events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DustUtxoEvent {
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

// Conversion from event types to storage types
impl From<QualifiedDustOutputEvent> for QualifiedDustOutput {
    fn from(event: QualifiedDustOutputEvent) -> Self {
        Self {
            initial_value: event.initial_value,
            owner: event.owner,
            nonce: event.nonce,
            seq: event.seq,
            ctime: event.ctime,
            backing_night: event.backing_night,
            mt_index: event.mt_index,
        }
    }
}

impl From<DustGenerationInfoEvent> for DustGenerationInfo {
    fn from(event: DustGenerationInfoEvent) -> Self {
        Self {
            night_utxo_hash: event.night_utxo_hash,
            value: event.value,
            owner: event.owner,
            nonce: event.nonce,
            ctime: event.ctime,
            dtime: event.dtime,
        }
    }
}

/// Type alias for transaction result with DUST events.
pub type DustTransactionResult = TransactionResultWithDustEvents<DustEvent>;
