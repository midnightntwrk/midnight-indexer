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

use indexer_common::{domain::ByteArray, infra::sqlx::SqlxOption};
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
