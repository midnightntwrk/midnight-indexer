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
    infra::api::v3::{
        CardanoNetwork, CardanoRewardAddress, HexEncodable, HexEncoded,
        encode_cardano_reward_address,
    },
};
use async_graphql::SimpleObject;

/// DUST generation status for a specific Cardano reward address.
#[derive(Debug, Clone, SimpleObject)]
pub struct DustGenerationStatus {
    /// The Bech32-encoded Cardano reward address (e.g., stake_test1... or stake1...).
    pub cardano_reward_address: CardanoRewardAddress,

    /// The hex-encoded associated DUST address if registered.
    pub dust_address: Option<HexEncoded>,

    /// Whether this reward address is registered.
    pub registered: bool,

    /// NIGHT balance backing generation in STAR.
    pub night_balance: String,

    /// DUST generation rate in SPECK per second.
    pub generation_rate: String,

    /// Maximum DUST capacity in SPECK.
    pub max_capacity: String,

    /// Current generated DUST capacity in SPECK.
    pub current_capacity: String,
}

impl From<domain::DustGenerationStatus> for DustGenerationStatus {
    fn from(status: domain::DustGenerationStatus) -> Self {
        // TODO: Make the cardano network configurable!
        let cardano_reward_address = CardanoRewardAddress(encode_cardano_reward_address(
            status.cardano_reward_address,
            CardanoNetwork::Testnet,
        ));

        Self {
            cardano_reward_address,
            dust_address: status.dust_address.map(|addr| addr.hex_encode()),
            registered: status.registered,
            night_balance: status.night_balance.to_string(),
            generation_rate: status.generation_rate.to_string(),
            max_capacity: status.max_capacity.to_string(),
            current_capacity: status.current_capacity.to_string(),
        }
    }
}
