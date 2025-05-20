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

use crate::domain::{AsBytesExt, HexEncoded, Transaction};
use derive_more::Deref;
use indexer_common::{
    domain::{IntentHash, RawTokenType, UnshieldedAddress as CommonUnshieldedAddress},
    infra::sqlx::{SqlxOption, U128BeBytes},
};
use sqlx::FromRow;

/// Represents an unshielded UTXO at the API domain level.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct UnshieldedUtxo {
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

    #[sqlx(try_from = "i64")]
    pub creating_transaction_id: u64,

    /// Database ID of the transaction that spent this UTXO, if any.
    #[sqlx(try_from = "SqlxOption<i64>")]
    pub spending_transaction_id: Option<u64>,

    #[sqlx(skip)]
    pub created_at_transaction: Option<Transaction>,

    #[sqlx(skip)]
    pub spent_at_transaction: Option<Transaction>,
}

/// Wrapper for HexEncoded address for the POC `unshieldedUtxos` query.
/// Will be replaced by a proper Bech32m scalar later.
#[derive(Debug, Deref, Clone, PartialEq, Eq)]
pub struct UnshieldedAddress(pub HexEncoded);

impl From<CommonUnshieldedAddress> for UnshieldedAddress {
    fn from(addr: CommonUnshieldedAddress) -> Self {
        Self(addr.hex_encode())
    }
}
