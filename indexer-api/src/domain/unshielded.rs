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

use crate::domain::Transaction;
use async_graphql::{InputValueError, InputValueResult, Scalar, ScalarType, Value};
use indexer_common::{
    domain::{
        IntentHash, NetworkId, RawTokenType, UnknownNetworkIdError,
        UnshieldedAddress as CommonUnshieldedAddress,
    },
    infra::sqlx::{SqlxOption, U128BeBytes},
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use thiserror::Error;

/// Represents an unshielded UTXO at the API-domain level.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct UnshieldedUtxo {
    /// The unshielded address that owns this UTXO
    pub owner_address: CommonUnshieldedAddress,

    /// Type of token (e.g. NIGHT has all-zero bytes).
    #[cfg_attr(feature = "standalone", sqlx(try_from = "&'a [u8]"))]
    pub token_type: RawTokenType,

    /// Hash of the intent that created this UTXO.
    #[cfg_attr(feature = "standalone", sqlx(try_from = "&'a [u8]"))]
    pub intent_hash: IntentHash,

    /// Amount (big-endian bytes in DB -> u128 here).
    #[sqlx(try_from = "U128BeBytes")]
    pub value: u128,

    /// Matches ledger's u32 type but stored as BIGINT since u32 max exceeds PostgreSQL INT range.
    #[sqlx(try_from = "i64")]
    pub output_index: u32,

    /// Database ID of the transaction that created this UTXO.
    #[sqlx(try_from = "i64")]
    pub creating_transaction_id: u64,

    /// Database ID of the transaction that spent this UTXO, if any.
    #[sqlx(try_from = "SqlxOption<i64>")]
    pub spending_transaction_id: Option<u64>,

    /// Full transaction data for the creating transaction (populated by queries).
    #[sqlx(skip)]
    pub created_at_transaction: Option<Transaction>,

    /// Full transaction data for the spending transaction (populated by queries)
    #[sqlx(skip)]
    pub spent_at_transaction: Option<Transaction>,

    /// Network ID for Bech32m address formatting (Option to avoid requiring Default by Sqlx).
    #[sqlx(skip)]
    pub network_id: Option<NetworkId>,
}

/// GraphQL scalar wrapper
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnshieldedAddress(pub String);

impl UnshieldedAddress {
    /// Converts this API address into a domain address, validating the bech32m format and
    /// network ID.
    ///
    /// Format expectations:
    /// - For mainnet: "mn_addr" + bech32m data
    /// - For other networks: "mn_addr_" + network-id + bech32m data where network-id is one of:
    ///   "dev", "test", "undeployed"
    pub fn try_into_domain(
        self,
        network_id: NetworkId,
    ) -> Result<CommonUnshieldedAddress, UnshieldedAddressFormatError> {
        let (hrp, bytes) = bech32::decode(&self.0).map_err(UnshieldedAddressFormatError::Decode)?;
        let hrp = hrp.to_lowercase();

        let Some(n) = hrp.strip_prefix("mn_addr") else {
            return Err(UnshieldedAddressFormatError::InvalidHrp(hrp));
        };
        let n = n.strip_prefix("_").unwrap_or(n).try_into()?;
        if n != network_id {
            return Err(UnshieldedAddressFormatError::UnexpectedNetworkId(
                n, network_id,
            ));
        }

        Ok(CommonUnshieldedAddress::from(bytes))
    }
}

#[derive(Debug, Error)]
pub enum UnshieldedAddressFormatError {
    #[error("cannot bech32m-decode unshielded address: {0}")]
    Decode(#[from] bech32::DecodeError),

    #[error("invalid HRP: got '{0}', expected 'mn_addr' prefix")]
    InvalidHrp(String),

    #[error("network ID error: {0}")]
    TryFromStrForNetworkIdError(#[from] UnknownNetworkIdError),

    #[error("network ID mismatch: got {0}, expected {1}")]
    UnexpectedNetworkId(NetworkId, NetworkId),
}

#[Scalar]
impl ScalarType for UnshieldedAddress {
    fn parse(value: Value) -> InputValueResult<Self> {
        if let Value::String(s) = &value {
            let (hrp, _) = bech32::decode(s)
                .map_err(|e| InputValueError::custom(format!("invalid bech32m address: {}", e)))?;
            let hrp = hrp.to_lowercase();

            if !hrp.starts_with("mn_addr") {
                return Err(InputValueError::custom(format!("invalid HRP: {}", hrp)));
            }

            Ok(UnshieldedAddress(s.clone()))
        } else {
            Err(InputValueError::expected_type(value))
        }
    }

    fn to_value(&self) -> Value {
        Value::String(self.0.clone())
    }
}
