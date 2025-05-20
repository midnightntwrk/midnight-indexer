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

use indexer_common::domain::{IntentHash, RawTokenType, UnshieldedAddress};
use sqlx::FromRow;

/// Represents unshielded UTXO data parsed from node events during chain-indexer processing.
/// This struct holds information needed to insert a new UTXO or identify one being spent.
/// It does *not* include the database-generated primary key (`id`) or the
/// `spending_transaction_id`.
#[derive(Debug, Clone, FromRow)]
pub struct UnshieldedUtxo {
    #[sqlx(try_from = "i64")]
    pub creating_transaction_id: u64,

    /// Matches ledger's u32 type but stored as BIGINT since u32 max exceeds PostgreSQL INT range
    #[sqlx(try_from = "i64")]
    pub output_index: u32,

    pub owner_address: UnshieldedAddress,

    pub token_type: RawTokenType,

    pub intent_hash: IntentHash,

    pub value: u128,
}
