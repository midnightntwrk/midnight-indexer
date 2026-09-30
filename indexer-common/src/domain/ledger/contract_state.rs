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

use crate::domain::{
    ContractBalance, TokenType,
    ledger::{Error, TaggedSerializableExt},
};
use midnight_coin_structure_v2::coin::TokenType as MidnightTokenType;
use midnight_onchain_runtime_v3::state::ContractState as ContractStateV3;
use midnight_storage_core_v1::{arena::Sp, db::DB};

/// Facade for `ContractState` from `midnight_ledger` across supported (protocol) versions.
///
/// Holds the arena pointer, so field reads force only the nodes they touch.
pub enum ContractState<D: DB> {
    V3(Sp<ContractStateV3<D>, D>),
}

impl<D: DB> ContractState<D> {
    /// Get the token balances for this contract.
    pub fn balances(&self) -> Result<Vec<ContractBalance>, Error> {
        match self {
            Self::V3(contract_state) => {
                contract_state
                    .balance
                    .iter()
                    .filter_map(|entry| {
                        // Read via deref: `Sp::into_inner` returns `None` for lazy or shared
                        // entries, silently dropping all balances.
                        let (token_type, amount) = &*entry;
                        let (token_type, amount) = (**token_type, **amount);

                        (amount > 0).then_some((token_type, amount))
                    })
                    .map(|(token_type, amount)| {
                        match token_type {
                            // For unshielded tokens extract the type directly.
                            MidnightTokenType::Unshielded(unshielded) => Ok(ContractBalance {
                                token_type: unshielded.0.0.into(),
                                amount,
                            }),

                            // For other tokens we serialize the type.
                            _ => {
                                let token_type = token_type
                                    .tagged_serialize()
                                    .map_err(|error| Error::Serialize("TokenTypeV8", error))?;

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

#[cfg(test)]
mod tests {
    use crate::domain::{
        ByteArray, TokenType,
        ledger::{ContractState, TaggedSerializableExt},
    };
    use midnight_base_crypto_v1::hash::HashOutput;
    use midnight_coin_structure_v2::coin::{TokenType as MidnightTokenType, UnshieldedTokenType};
    use midnight_onchain_runtime_v3::state::ContractState as ContractStateV3;
    use midnight_serialize_v1::tagged_deserialize;
    use midnight_storage_core_v1::{DefaultDB, arena::Sp};

    #[test]
    fn test_balances_v8() {
        let mut contract_state = ContractStateV3::<DefaultDB>::default();
        contract_state.balance = contract_state.balance.insert(
            MidnightTokenType::Unshielded(UnshieldedTokenType(HashOutput(TOKEN_TYPE.0))),
            AMOUNT,
        );
        let contract_state = contract_state
            .tagged_serialize()
            .expect("contract state can be serialized");

        let contract_state =
            tagged_deserialize::<ContractStateV3<DefaultDB>>(&mut contract_state.as_ref())
                .expect("contract state can be deserialized");
        let balances = ContractState::V3(Sp::new(contract_state))
            .balances()
            .expect("balances can be extracted");

        assert_eq!(balances.len(), 1);
        assert_eq!(balances[0].token_type, TOKEN_TYPE);
        assert_eq!(balances[0].amount, AMOUNT);
    }

    const TOKEN_TYPE: TokenType = ByteArray([7; 32]);
    const AMOUNT: u128 = 1_000_000;
}
