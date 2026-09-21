// This file is part of midnight-indexer.
// Copyright (C) Midnight Foundation
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
    ContractAttributes, ContractBalance, SerializedContractAddress, SerializedContractStateKey,
    SerializedZswapStateKey,
};

/// A contract action.
///
/// The states are held as ledger-arena keys rather than serialized blobs: the arena already stores
/// them content-addressed and structurally shared, so a key costs tens of bytes where the blob cost
/// hundreds of kilobytes and grew quadratically in the number of actions per contract.
///
/// Both keys are optional because a failed action has no contract state to reference — today that
/// is represented as an empty `state` blob — and because a state that could not be captured must
/// read back as absent rather than as some other contract's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractAction {
    pub address: SerializedContractAddress,

    /// Physical segment containing this action.
    pub segment: u16,

    /// Whether this Call also has work in guaranteed logical segment 0.
    pub has_guaranteed_transcript: bool,

    pub state_key: Option<SerializedContractStateKey>,
    pub zswap_state_key: Option<SerializedZswapStateKey>,
    pub extracted_balances: Vec<ContractBalance>,
    pub attributes: ContractAttributes,
}

impl From<indexer_common::domain::ContractAction> for ContractAction {
    fn from(contract_action: indexer_common::domain::ContractAction) -> Self {
        Self {
            address: contract_action.address,
            segment: contract_action.segment,
            has_guaranteed_transcript: contract_action.has_guaranteed_transcript,
            state_key: Default::default(),
            zswap_state_key: Default::default(),
            extracted_balances: Default::default(),
            attributes: contract_action.attributes,
        }
    }
}
