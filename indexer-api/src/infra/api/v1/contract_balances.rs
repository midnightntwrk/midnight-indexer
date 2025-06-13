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

//! Contract unshielded token balance functionality.
//!
//! This module provides functionality for exposing unshielded token balances
//! held by smart contracts through the GraphQL API. The balances are extracted
//! and stored during chain indexing by the chain-indexer component.
//!
//! # Architecture Notes
//!
//! From the midnight architecture specification:
//!
//! ```text
//! struct ContractState {
//!     data: StateValue,
//!     operations: Map<Bytes, ZkVerifierKey>,
//!     maintenance_authority: ContractMaintenanceAuthority,
//!     balance: Map<TokenType, u128>,  // <- This is extracted during indexing
//! }
//! ```
//!
//! ## Balance Rules
//!
//! - **Deploy**: Contracts must be deployed with zero balance (architecture requirement)
//! - **Call**: Can modify balances through `unshielded_inputs` and `unshielded_outputs`
//! - **Update**: Maintenance updates that may affect contract state and balances
//!
//! ## Implementation Status
//!
//! This module provides GraphQL API support for querying pre-processed balance data
//! that was extracted during chain indexing. The heavy deserialization work is done
//! by the chain-indexer component, and this API layer simply reads the stored balance data.

use crate::domain::HexEncoded;
use async_graphql::SimpleObject;

/// Represents a token balance held by a contract.
///
/// This type is exposed through the GraphQL API to allow clients to query
/// unshielded token balances for any contract action (Deploy, Call, Update).
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct UnshieldedBalance {
    /// Token type identifier, hex-encoded for GraphQL compatibility
    pub token_type: HexEncoded,
    /// Balance amount as string to support u128 values
    pub amount: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unshielded_balance_equality() {
        let balance1 = UnshieldedBalance {
            token_type: HexEncoded::try_from("1234".to_string()).unwrap(),
            amount: "1000".to_string(),
        };
        let balance2 = UnshieldedBalance {
            token_type: HexEncoded::try_from("1234".to_string()).unwrap(),
            amount: "1000".to_string(),
        };
        let balance3 = UnshieldedBalance {
            token_type: HexEncoded::try_from("5678".to_string()).unwrap(),
            amount: "1000".to_string(),
        };

        assert_eq!(balance1, balance2);
        assert_ne!(balance1, balance3);
    }

    #[test]
    fn test_unshielded_balance_debug() {
        let balance = UnshieldedBalance {
            token_type: HexEncoded::try_from("1234567890abcdef".to_string()).unwrap(),
            amount: "123456789".to_string(),
        };

        let debug_str = format!("{:?}", balance);
        assert!(debug_str.contains("1234567890abcdef"));
        assert!(debug_str.contains("123456789"));
    }
}
