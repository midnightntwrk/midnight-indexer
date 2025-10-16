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

//! GraphQL API types for DUST operations.
//!
//! NOTE: This is a minimal cherry-pick for dustGenerationStatus query only.
//! Other DUST-related types will be added when the full DUST system is integrated.

use crate::{
    domain,
    infra::api::v3::{AsBytesExt, HexEncoded},
};
use async_graphql::SimpleObject;

/// DUST generation status for a specific Cardano stake key.
#[derive(Debug, Clone, SimpleObject)]
pub struct DustGenerationStatus {
    /// The hex-encoded Cardano stake key.
    pub cardano_stake_key: HexEncoded,

    /// The hex-encoded associated DUST address if registered.
    pub dust_address: Option<HexEncoded>,

    /// Whether this stake key is registered.
    pub registered: bool,

    /// NIGHT balance backing generation.
    pub night_balance: String,

    /// Generation rate in Specks per second.
    pub generation_rate: String,

    /// Current DUST capacity.
    pub current_capacity: String,
}

impl From<domain::DustGenerationStatus> for DustGenerationStatus {
    fn from(status: domain::DustGenerationStatus) -> Self {
        Self {
            cardano_stake_key: status.cardano_stake_key.hex_encode(),
            dust_address: status.dust_address.map(|addr| addr.hex_encode()),
            registered: status.registered,
            night_balance: status.night_balance.to_string(),
            generation_rate: status.generation_rate.to_string(),
            current_capacity: status.current_capacity.to_string(),
        }
    }
}
