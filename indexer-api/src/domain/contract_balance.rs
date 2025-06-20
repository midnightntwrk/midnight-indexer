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

//! Domain types for contract unshielded token balances.

use crate::domain::HexEncoded;
use indexer_common::{domain::RawTokenType, infra::sqlx::U128BeBytes};
use sqlx::FromRow;

/// Represents a token balance held by a contract at the domain level.
/// This type is used internally by the storage layer and converted to
/// API-specific types (GraphQL) when exposed through the API.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct ContractBalance {
    /// Token type identifier.
    #[cfg_attr(feature = "standalone", sqlx(try_from = "&'a [u8]"))]
    pub token_type: RawTokenType,
    /// Balance amount (big-endian bytes in DB -> u128 here).
    #[sqlx(try_from = "U128BeBytes")]
    pub amount: u128,
}

impl ContractBalance {
    /// Convert to the API (GraphQL) representation.
    pub fn to_api(self) -> crate::infra::api::v1::contract_balance::ContractBalance {
        crate::infra::api::v1::contract_balance::ContractBalance {
            token_type: HexEncoded(const_hex::encode(self.token_type.0)),
            amount: self.amount.to_string(),
        }
    }
}
