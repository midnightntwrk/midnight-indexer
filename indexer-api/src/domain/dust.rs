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

use indexer_common::domain::{CardanoRewardAddress, DustPublicKey};
use serde::{Deserialize, Serialize};

/// DUST generation status for a specific Cardano reward address.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DustGenerationStatus {
    /// Cardano reward address.
    pub cardano_reward_address: CardanoRewardAddress,

    /// Associated DUST address (DUST public key) if registered.
    pub dust_address: Option<DustPublicKey>,

    /// Whether this reward address is registered.
    pub registered: bool,

    /// NIGHT balance backing generation.
    pub night_balance: u128,

    /// Generation rate in Specks per second.
    pub generation_rate: u128,

    /// Maximum DUST capacity in SPECK.
    pub max_capacity: u128,

    /// Current generated DUST capacity in Specks.
    pub current_capacity: u128,
}
