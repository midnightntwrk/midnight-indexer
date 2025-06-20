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

use crate::domain::ContractBalance;
use indexer_common::domain::{
    ContractActionVariant, ContractAddress, ContractEntryPoint, ContractState, RawLedgerState,
};
use serde::Serialize;

/// A contract action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractAction {
    pub address: ContractAddress,
    pub state: ContractState,
    pub attributes: ContractAttributes,
    pub zswap_state: RawLedgerState,
    pub extracted_balances: Vec<ContractBalance>,
}

/// Attributes for a specific [ContractAction].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ContractAttributes {
    Deploy,
    Call { entry_point: ContractEntryPoint },
    Update,
}

impl From<&ContractAttributes> for ContractActionVariant {
    fn from(attributes: &ContractAttributes) -> Self {
        match attributes {
            ContractAttributes::Deploy => Self::Deploy,
            ContractAttributes::Call { .. } => Self::Call,
            ContractAttributes::Update => Self::Update,
        }
    }
}
