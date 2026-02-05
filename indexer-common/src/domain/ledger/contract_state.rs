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

use crate::domain::{
    ContractBalance, LedgerVersion, ProtocolVersion, TokenType,
    ledger::{Error, TaggedSerializableV7_0_0Ext},
};
use fastrace::trace;
use midnight_coin_structure_v7_0_0::coin::TokenType as TokenTypeV7_0_0;
use midnight_onchain_runtime_v7_0_0::state::ContractState as ContractStateV7_0_0;
use midnight_serialize_v7_0_0::tagged_deserialize as tagged_deserialize_v7_0_0;
use midnight_storage_v7_0_0::{DefaultDB as DefaultDBV7_0_0, arena::Sp as SpV7_0_0};

/// Facade for `ContractState` from `midnight_ledger` across supported (protocol) versions.
#[derive(Debug, Clone)]
pub enum ContractState {
    V7_0_0(ContractStateV7_0_0<DefaultDBV7_0_0>),
}

impl ContractState {
    /// Deserialize the given serialized contract state using the given protocol version.
    #[trace(properties = { "protocol_version": "{protocol_version}" })]
    pub fn deserialize(
        contract_state: impl AsRef<[u8]>,
        protocol_version: ProtocolVersion,
    ) -> Result<Self, Error> {
        let contract_state = match protocol_version.ledger_version()? {
            LedgerVersion::V7 => {
                let contract_state = tagged_deserialize_v7_0_0(&mut contract_state.as_ref())
                    .map_err(|error| Error::Deserialize("ContractStateV7_0_0", error))?;
                Self::V7_0_0(contract_state)
            }
        };

        Ok(contract_state)
    }

    /// Get the token balances for this contract.
    pub fn balances(&self) -> Result<Vec<ContractBalance>, Error> {
        match self {
            Self::V7_0_0(contract_state) => {
                contract_state
                    .balance
                    .iter()
                    .filter_map(|entry| {
                        let (token_type_sp, amount_sp) = SpV7_0_0::into_inner(entry)?;
                        let token_type = SpV7_0_0::into_inner(token_type_sp)?;
                        let amount = SpV7_0_0::into_inner(amount_sp)?;

                        (amount > 0).then_some((token_type, amount))
                    })
                    .map(|(token_type, amount)| {
                        match token_type {
                            // For unshielded tokens extract the type directly.
                            TokenTypeV7_0_0::Unshielded(unshielded) => Ok(ContractBalance {
                                token_type: unshielded.0.0.into(),
                                amount,
                            }),

                            // For other tokens we serialize the type.
                            _ => {
                                let token_type = token_type
                                    .tagged_serialize_v7_0_0()
                                    .map_err(|error| Error::Serialize("TokenTypeV7_0_0", error))?;

                                let token_type = TokenType::try_from(token_type.as_ref())
                                    .map_err(Error::ByteArrayLen)?;

                                Ok(ContractBalance { token_type, amount })
                            }
                        }
                    })
                    .collect()
            }
        }
    }
}
