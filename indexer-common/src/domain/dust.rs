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

use crate::domain::{DustOwner, NightUtxoHash, Nonce};
use serde::{Deserialize, Serialize};

/// Qualified DUST output information.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualifiedDustOutput {
    /// Initial value of DUST UTXO.
    pub initial_value: u128,

    /// Owner's DUST public key.
    pub owner: DustOwner,

    /// Nonce for this DUST UTXO.
    pub nonce: Nonce,

    /// Sequence number.
    pub seq: u32,

    /// Creation time.
    pub ctime: u64,

    /// Backing Night UTXO nonce.
    pub backing_night: Nonce,

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
    pub nonce: Nonce,

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

/// Initial DUST parameters as specified in the ledger specification.
/// These values are defined in midnight-ledger/spec/dust.md and determine the economic
/// properties of DUST generation and decay.
///
/// # Unit Conversions
/// - 1 Night = 10^6 Stars (atomic unit of Night).
/// - 1 Dust = 10^15 Specks (atomic unit of Dust).
///
/// # Parameter Explanations
///
/// ## night_dust_ratio = 5_000_000_000 Specks per Star
/// This represents the maximum DUST that can be generated per NIGHT:
/// - Target: 5 DUST per NIGHT.
/// - Calculation: (5 DUST × 10^15 Specks/DUST) / (10^6 Stars/NIGHT) = 5 × 10^9 Specks/Star.
/// ## generation_decay_rate = 8_267 Specks per Star per second
/// This rate produces an approximately 1-week generation time to reach maximum capacity:
/// - Time to max = night_dust_ratio / generation_decay_rate.
/// - = 5_000_000_000 / 8_267 seconds.
/// - = 604,760 seconds.
/// - ≈ 7.0002 days ≈ 1 week.
/// ## dust_grace_period = 10,800 seconds (3 hours)
/// Maximum time window allowed for DUST spends to prevent transactions from living indefinitely
/// while still accommodating network congestion.
impl Default for DustParameters {
    fn default() -> Self {
        Self {
            night_dust_ratio: 5_000_000_000,
            generation_decay_rate: 8_267,
            dust_grace_period: 3 * 60 * 60,
        }
    }
}
