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

use indexer_common::domain::{
    ContractAttributes, ContractBalance, RawContractAddress, RawContractState, RawZswapState,
};

/// A contract action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractAction {
    pub address: RawContractAddress,
    pub state: RawContractState,
    pub attributes: ContractAttributes,
    pub zswap_state: RawZswapState,
    pub extracted_balances: Vec<ContractBalance>,
}

impl From<indexer_common::domain::ContractAction> for ContractAction {
    fn from(contract_action: indexer_common::domain::ContractAction) -> Self {
        Self {
            address: contract_action.address,
            state: contract_action.state,
            attributes: contract_action.attributes,
            zswap_state: Default::default(),
            extracted_balances: contract_action.extracted_balances,
        }
    }
}
