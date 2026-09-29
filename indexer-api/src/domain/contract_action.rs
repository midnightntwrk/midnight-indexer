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

use indexer_common::domain::{
    ContractAttributes, SerializedContractAddress, SerializedContractStateKey,
    SerializedZswapStateKey,
};
use sqlx::FromRow;

/// A contract action.
///
/// The states are ledger-arena keys, resolved to bytes on demand: a row is tens of bytes rather
/// than the ~860 KB the state blob used to cost, which matters most for the queries that return up
/// to 500 rows or drive `Transaction.contractActions` through the loader.
///
/// Both keys are nullable. A failed action has no contract state, which is represented today as an
/// empty `state` blob, so the API resolves a missing key to the empty string.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct ContractAction {
    #[sqlx(try_from = "i64")]
    pub id: u64,

    pub address: SerializedContractAddress,

    pub state_key: Option<SerializedContractStateKey>,

    #[sqlx(json)]
    pub attributes: ContractAttributes,

    pub zswap_state_key: Option<SerializedZswapStateKey>,

    #[sqlx(try_from = "i64")]
    pub transaction_id: u64,
}
