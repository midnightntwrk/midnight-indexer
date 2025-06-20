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

//! GraphQL types for contract unshielded token balances.

use crate::domain::HexEncoded;
use async_graphql::SimpleObject;

/// Represents a token balance held by a contract.
/// This type is exposed through the GraphQL API to allow clients to query
/// unshielded token balances for any contract action (Deploy, Call, Update).
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct ContractBalance {
    /// Token type identifier, hex-encoded for GraphQL compatibility.
    pub token_type: HexEncoded,
    /// Balance amount as string to support u128 values.
    pub amount: String,
}

#[cfg(test)]
mod tests {
    use crate::{domain::HexEncoded, infra::api::v1::contract_balance::ContractBalance};

    #[test]
    fn test_contract_balance_equality() {
        let balance1 = ContractBalance {
            token_type: HexEncoded::try_from("1234".to_string()).unwrap(),
            amount: "1000".to_string(),
        };
        let balance2 = ContractBalance {
            token_type: HexEncoded::try_from("1234".to_string()).unwrap(),
            amount: "1000".to_string(),
        };
        let balance3 = ContractBalance {
            token_type: HexEncoded::try_from("5678".to_string()).unwrap(),
            amount: "1000".to_string(),
        };

        assert_eq!(balance1, balance2);
        assert_ne!(balance1, balance3);
    }

    #[test]
    fn test_contract_balance_debug() {
        let balance = ContractBalance {
            token_type: HexEncoded::try_from("1234567890abcdef".to_string()).unwrap(),
            amount: "123456789".to_string(),
        };

        let debug_str = format!("{:?}", balance);
        assert!(debug_str.contains("1234567890abcdef"));
        assert!(debug_str.contains("123456789"));
    }
}
