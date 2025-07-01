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
use indexer_common::{
    domain::{IntentHash, RawTokenType, RawUnshieldedAddress},
    infra::sqlx::{SqlxOption, U128BeBytes},
};
use sqlx::FromRow;

/// Represents an unshielded UTXO at the API-domain level.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct UnshieldedUtxo {
    /// The unshielded address that owns this UTXO.
    #[cfg_attr(feature = "standalone", sqlx(try_from = "&'a [u8]"))]
    pub owner_address: RawUnshieldedAddress,

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

    /// Full transaction data for the spending transaction (populated by queries).
    #[sqlx(skip)]
    pub spent_at_transaction: Option<Transaction>,
}

/// Token balance held by a contract.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct ContractBalance {
    /// Token type identifier.
    #[cfg_attr(feature = "standalone", sqlx(try_from = "&'a [u8]"))]
    pub token_type: RawTokenType,

    /// Balance amount.
    #[sqlx(try_from = "U128BeBytes")]
    pub amount: u128,
}
