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

use indexer_common::domain::{CardanoStakeKey, DustAddress, DustUtxoId};

/// Domain representation of DUST registration events from the NativeTokenObservation pallet.
#[derive(Debug, Clone, PartialEq)]
pub enum DustRegistrationEvent {
    /// Cardano address registered with DUST address.
    Registration {
        cardano_address: CardanoStakeKey,
        dust_address: DustAddress,
    },

    /// Cardano address deregistered from DUST address.
    Deregistration {
        cardano_address: CardanoStakeKey,
        dust_address: DustAddress,
    },

    /// UTXO mapping added for registration.
    MappingAdded {
        cardano_address: CardanoStakeKey,
        dust_address: DustAddress,
        utxo_id: DustUtxoId,
    },

    /// UTXO mapping removed from registration.
    MappingRemoved {
        cardano_address: CardanoStakeKey,
        dust_address: DustAddress,
        utxo_id: DustUtxoId,
    },
}
