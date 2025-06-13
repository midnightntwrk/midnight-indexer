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

//! Contract unshielded token balance extraction functionality.
//! This module provides functionality for extracting unshielded token balances
//! from contract state during chain indexing. The extracted balances are stored
//! in the database for efficient querying by the API layer.
//!
//! ## NetworkId Requirement
//! Contract state deserialization requires the correct NetworkId because the midnight
//! serialization framework uses network-specific formatting. The NetworkId is only
//! needed during the extraction process - it is not stored in the database.
use indexer_common::{
    domain::{ByteVec, NetworkId, RawTokenType},
    serialize::SerializableExt,
};
use midnight_coin_structure::coin::TokenType;
use midnight_onchain_runtime::state::ContractState;
use midnight_serialize::deserialize;
use midnight_storage::{DefaultDB, arena::Sp, storage::HashMap as LedgerHashMap};
use std::io;
use thiserror::Error;

/// Represents a token balance extracted from contract state.
/// This is the internal representation used during indexing, before storing
/// to the database. The API layer will read this data and convert it to
/// GraphQL-compatible types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractBalance {
    /// Token type identifier.
    pub token_type: RawTokenType,

    /// Balance amount as u128.
    pub amount: u128,
}

/// Errors that can occur during contract balance extraction.
#[derive(Debug, Error)]
pub enum ContractBalanceError {
    #[error("cannot deserialize contract state or balance map")]
    Deserialization,

    #[error("cannot serialize token type")]
    TokenSerialization(#[from] io::Error),
}

impl ContractBalance {
    /// Extract unshielded token balances from contract state.
    /// This function is called during chain indexing to extract balance information
    /// from contract state and prepare it for storage in the database.
    ///
    /// # Arguments
    /// * `state` - Raw contract state as stored in the blockchain.
    /// * `network_id` - Network context required for proper deserialization.
    ///
    /// # Returns
    /// Vector of `ExtractedBalance` objects representing non-zero token balances.
    /// Returns error if deserialization fails.
    ///
    /// # Implementation Notes
    /// This function implements a multi-strategy approach:
    /// 1. Try deserializing the full ContractState.
    /// 2. Fallback to deserializing just the balance HashMap.
    /// 3. Return error if both strategies fail.
    pub fn extract(
        state: &ByteVec,
        network_id: NetworkId,
    ) -> Result<Vec<Self>, ContractBalanceError> {
        if let Ok(contract_state) =
            deserialize::<ContractState<DefaultDB>, _>(&mut state.as_ref(), network_id.into())
        {
            return extract_from_contract_state(&contract_state, network_id);
        }

        if let Ok(balance_map) = deserialize::<LedgerHashMap<TokenType, u128, DefaultDB>, _>(
            &mut state.as_ref(),
            network_id.into(),
        ) {
            return extract_from_balance_map(&balance_map, network_id);
        }

        Err(ContractBalanceError::Deserialization)
    }
}

/// Extract balance information from a successfully deserialized ContractState.
/// This function processes the balance field from the contract state and converts
/// it to the internal ExtractedBalance format.
fn extract_from_contract_state(
    contract_state: &ContractState<DefaultDB>,
    network_id: NetworkId,
) -> Result<Vec<ContractBalance>, ContractBalanceError> {
    contract_state
        .balance
        .iter()
        .filter_map(|entry| {
            let entry_tuple = Sp::into_inner(entry)?;
            let (token_type_sp, amount_sp) = entry_tuple;
            let token_type = Sp::into_inner(token_type_sp)?;
            let amount = Sp::into_inner(amount_sp)?;

            if amount > 0 {
                // For unshielded tokens, extract the hash directly
                match token_type {
                    TokenType::Unshielded(unshielded) => Some(Ok(ContractBalance {
                        token_type: RawTokenType::from(unshielded.0.0),
                        amount,
                    })),

                    _ => {
                        // For other token types, we need to serialize.
                        match token_type.serialize(network_id) {
                            Ok(token_type) => match RawTokenType::try_from(token_type) {
                                Ok(token_type) => Some(Ok(ContractBalance { token_type, amount })),

                                _ => Some(Err(ContractBalanceError::TokenSerialization(
                                    io::Error::new(
                                        io::ErrorKind::InvalidData,
                                        "token type serialization produced wrong length",
                                    ),
                                ))),
                            },

                            Err(e) => Some(Err(ContractBalanceError::TokenSerialization(e))),
                        }
                    }
                }
            } else {
                None
            }
        })
        .collect()
}

/// Extract balance information from a successfully deserialized balance HashMap.
/// This is a fallback function for cases where we can only deserialize the balance
/// portion of the contract state.
fn extract_from_balance_map(
    balance_map: &LedgerHashMap<TokenType, u128, DefaultDB>,
    network_id: NetworkId,
) -> Result<Vec<ContractBalance>, ContractBalanceError> {
    balance_map
        .iter()
        .filter_map(|entry| {
            let entry_tuple = Sp::into_inner(entry)?;
            let (token_type_sp, amount_sp) = entry_tuple;
            let token_type = Sp::into_inner(token_type_sp)?;
            let amount = Sp::into_inner(amount_sp)?;

            if amount > 0 {
                // For unshielded tokens, extract the hash directly
                match token_type {
                    TokenType::Unshielded(unshielded) => Some(Ok(ContractBalance {
                        token_type: RawTokenType::from(unshielded.0.0),
                        amount,
                    })),

                    _ => {
                        // For other token types, we need to serialize
                        match token_type.serialize(network_id) {
                            Ok(token_type) => match RawTokenType::try_from(token_type) {
                                Ok(token_type) => Some(Ok(ContractBalance { token_type, amount })),

                                _ => Some(Err(ContractBalanceError::TokenSerialization(
                                    io::Error::new(
                                        io::ErrorKind::InvalidData,
                                        "token type serialization produced wrong length",
                                    ),
                                ))),
                            },

                            Err(e) => Some(Err(ContractBalanceError::TokenSerialization(e))),
                        }
                    }
                }
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::domain::ContractBalance;
    use indexer_common::{
        domain::{ByteVec, NetworkId, RawTokenType},
        serialize::SerializableExt,
    };
    use midnight_base_crypto::hash::HashOutput;
    use midnight_coin_structure::coin::{TokenType, UnshieldedTokenType};
    use midnight_onchain_runtime::state::ContractState;
    use midnight_storage::{DefaultDB, arena::Sp};

    #[test]
    fn test_extract_contract_balances_empty_state() {
        let empty_state = ByteVec(vec![]);
        let result = ContractBalance::extract(&empty_state, NetworkId::Undeployed);
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_contract_balances_non_empty_state() {
        let non_empty_state = ByteVec(vec![1, 2, 3, 4]);
        let result = ContractBalance::extract(&non_empty_state, NetworkId::Undeployed);
        // Should return error for invalid data
        assert!(result.is_err());
    }

    #[test]
    fn test_serialize_token_type() {
        let token_type = TokenType::Unshielded(UnshieldedTokenType(HashOutput([1u8; 32])));

        let result = token_type.serialize(NetworkId::Undeployed);

        if let Ok(bytes) = result {
            assert!(!bytes.is_empty());
        }
    }

    #[test]
    fn test_extracted_balance_equality() {
        let balance1 = ContractBalance {
            token_type: RawTokenType::from([1; 32]),
            amount: 1000,
        };
        let balance2 = ContractBalance {
            token_type: RawTokenType::from([1; 32]),
            amount: 1000,
        };
        let balance3 = ContractBalance {
            token_type: RawTokenType::from([2; 32]),
            amount: 1000,
        };

        assert_eq!(balance1, balance2);
        assert_ne!(balance1, balance3);
    }

    #[test]
    fn test_balance_extraction_pattern() {
        // Test the balance extraction pattern using storage pointers
        let contract_state = ContractState::<DefaultDB>::default();

        // Extract balances using the storage pointer pattern
        let result: Vec<(TokenType, u128)> = contract_state
            .balance
            .iter()
            .filter_map(|entry| {
                let entry_tuple = Sp::into_inner(entry)?;
                let (token_type_sp, amount_sp) = entry_tuple;
                let token_type = Sp::into_inner(token_type_sp)?;
                let amount = Sp::into_inner(amount_sp)?;
                Some((token_type, amount))
            })
            .collect();

        assert_eq!(result.len(), 0);

        // Test extracting just amounts using the same pattern
        let amounts: Vec<u128> = contract_state
            .balance
            .iter()
            .filter_map(|entry| {
                let entry_tuple = Sp::into_inner(entry)?;
                let (_, amount_sp) = entry_tuple;
                Sp::into_inner(amount_sp)
            })
            .collect();

        assert_eq!(amounts.len(), 0);
    }
}
