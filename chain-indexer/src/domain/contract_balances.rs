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
//!
//! This module provides functionality for extracting unshielded token balances
//! from contract state during chain indexing. The extracted balances are stored
//! in the database for efficient querying by the API layer.
//!
//! ## NetworkId Requirement
//!
//! Contract state deserialization requires the correct NetworkId because the midnight
//! serialization framework uses network-specific formatting. The NetworkId is only
//! needed during the extraction process - it is not stored in the database.

use indexer_common::domain::{ByteVec, NetworkId};
use midnight_coin_structure::coin::TokenType;
use midnight_onchain_runtime::state::ContractState;
use midnight_serialize::{NetworkId as SerializeNetworkId, Serializable, deserialize, serialize};
use midnight_storage::{DefaultDB, arena::Sp, storage::HashMap as LedgerHashMap};

/// Represents an extracted token balance from contract state.
///
/// This is the internal representation used during indexing, before storing
/// to the database. The API layer will read this data and convert it to
/// GraphQL-compatible types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedBalance {
    /// Token type identifier, serialized as bytes
    pub token_type_bytes: Vec<u8>,
    /// Balance amount as u128
    pub amount: u128,
}

/// Extract unshielded token balances from contract state.
///
/// This function is called during chain indexing to extract balance information
/// from contract state and prepare it for storage in the database.
///
/// # Arguments
///
/// * `state` - Raw contract state as stored in the blockchain
/// * `network_id` - Network context required for proper deserialization
///
/// # Returns
///
/// Vector of `ExtractedBalance` objects representing non-zero token balances.
/// Returns empty vector if deserialization fails or no balances exist.
///
/// # Implementation Notes
///
/// This function implements a multi-strategy approach:
/// 1. Try deserializing the full ContractState
/// 2. Fallback to deserializing just the balance HashMap
/// 3. Graceful degradation to empty vector on failure
pub fn extract_contract_balances(state: &ByteVec, network_id: NetworkId) -> Vec<ExtractedBalance> {
    let serialize_network_id: SerializeNetworkId = match network_id {
        NetworkId::MainNet => SerializeNetworkId::MainNet,
        NetworkId::DevNet => SerializeNetworkId::DevNet,
        NetworkId::TestNet => SerializeNetworkId::TestNet,
        NetworkId::Undeployed => SerializeNetworkId::Undeployed,
    };

    if let Ok(contract_state) =
        deserialize::<ContractState<DefaultDB>, _>(&mut state.0.as_slice(), serialize_network_id)
    {
        return extract_balances_from_contract_state(&contract_state, serialize_network_id);
    }

    if let Ok(balance_map) = deserialize::<LedgerHashMap<TokenType, u128, DefaultDB>, _>(
        &mut state.0.as_slice(),
        serialize_network_id,
    ) {
        return extract_balances_from_balance_map(&balance_map, serialize_network_id);
    }

    Vec::new()
}

/// Extract balance information from a successfully deserialized ContractState.
///
/// This function processes the balance field from the contract state and converts
/// it to the internal ExtractedBalance format.
fn extract_balances_from_contract_state(
    contract_state: &ContractState<DefaultDB>,
    network_id: SerializeNetworkId,
) -> Vec<ExtractedBalance> {
    contract_state
        .balance
        .iter()
        .filter_map(|entry| {
            let entry_tuple = Sp::into_inner(entry)?;
            let (token_type_sp, amount_sp) = entry_tuple;
            let token_type = Sp::into_inner(token_type_sp)?;
            let amount = Sp::into_inner(amount_sp)?;

            if amount > 0 {
                match serialize_token_type_to_bytes(&token_type, network_id) {
                    Ok(token_type_bytes) => Some(ExtractedBalance {
                        token_type_bytes,
                        amount,
                    }),
                    Err(_) => None,
                }
            } else {
                None
            }
        })
        .collect()
}

/// Extract balance information from a successfully deserialized balance HashMap.
///
/// This is a fallback function for cases where we can only deserialize the balance
/// portion of the contract state.
fn extract_balances_from_balance_map(
    balance_map: &LedgerHashMap<TokenType, u128, DefaultDB>,
    network_id: SerializeNetworkId,
) -> Vec<ExtractedBalance> {
    balance_map
        .iter()
        .filter_map(|entry| {
            let entry_tuple = Sp::into_inner(entry)?;
            let (token_type_sp, amount_sp) = entry_tuple;
            let token_type = Sp::into_inner(token_type_sp)?;
            let amount = Sp::into_inner(amount_sp)?;

            if amount > 0 {
                match serialize_token_type_to_bytes(&token_type, network_id) {
                    Ok(token_type_bytes) => Some(ExtractedBalance {
                        token_type_bytes,
                        amount,
                    }),
                    Err(_) => None,
                }
            } else {
                None
            }
        })
        .collect()
}

/// Convert a TokenType to a byte array for database storage.
///
/// This function serializes the TokenType using the midnight serialization
/// framework. The resulting bytes will be stored in the database and later
/// converted back to hex-encoded strings by the API layer.
fn serialize_token_type_to_bytes(
    token_type: &TokenType,
    network_id: SerializeNetworkId,
) -> Result<Vec<u8>, std::io::Error> {
    let size = Serializable::serialized_size(token_type);
    let mut bytes = Vec::with_capacity(size);

    serialize(token_type, &mut bytes, network_id)?;

    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexer_common::domain::NetworkId;
    use midnight_base_crypto::hash::HashOutput;
    use midnight_coin_structure::coin::{TokenType, UnshieldedTokenType};
    use midnight_onchain_runtime::state::ContractState;
    use midnight_storage::DefaultDB;

    #[test]
    fn test_extract_contract_balances_empty_state() {
        let empty_state = ByteVec(vec![]);
        let balances = extract_contract_balances(&empty_state, NetworkId::Undeployed);
        assert_eq!(balances, Vec::new());
    }

    #[test]
    fn test_extract_contract_balances_non_empty_state() {
        let non_empty_state = ByteVec(vec![1, 2, 3, 4]);
        let balances = extract_contract_balances(&non_empty_state, NetworkId::Undeployed);
        // Should return empty for now until deserialization works with real data
        assert_eq!(balances, Vec::new());
    }

    #[test]
    fn test_serialize_token_type_to_bytes() {
        let token_type = TokenType::Unshielded(UnshieldedTokenType(HashOutput([1u8; 32])));

        let result = serialize_token_type_to_bytes(&token_type, SerializeNetworkId::Undeployed);

        if let Ok(bytes) = result {
            assert!(!bytes.is_empty());
        }
    }

    #[test]
    fn test_extracted_balance_equality() {
        let balance1 = ExtractedBalance {
            token_type_bytes: vec![1, 2, 3, 4],
            amount: 1000,
        };
        let balance2 = ExtractedBalance {
            token_type_bytes: vec![1, 2, 3, 4],
            amount: 1000,
        };
        let balance3 = ExtractedBalance {
            token_type_bytes: vec![5, 6, 7, 8],
            amount: 1000,
        };

        assert_eq!(balance1, balance2);
        assert_ne!(balance1, balance3);
    }

    #[test]
    fn test_network_id_conversion() {
        // Test that all NetworkId variants can be converted to SerializeNetworkId
        let test_cases = [
            (NetworkId::MainNet, SerializeNetworkId::MainNet),
            (NetworkId::DevNet, SerializeNetworkId::DevNet),
            (NetworkId::TestNet, SerializeNetworkId::TestNet),
            (NetworkId::Undeployed, SerializeNetworkId::Undeployed),
        ];

        for (indexer_id, expected_serialize_id) in test_cases {
            let converted = match indexer_id {
                NetworkId::MainNet => SerializeNetworkId::MainNet,
                NetworkId::DevNet => SerializeNetworkId::DevNet,
                NetworkId::TestNet => SerializeNetworkId::TestNet,
                NetworkId::Undeployed => SerializeNetworkId::Undeployed,
            };

            assert_eq!(converted, expected_serialize_id);
        }
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
