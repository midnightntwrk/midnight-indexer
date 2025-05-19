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

#[cfg(any(feature = "cloud", feature = "standalone"))]
use crate::domain::{self, ContractAttributes};
#[cfg(any(feature = "cloud", feature = "standalone"))]
use indexer_common::domain::{ContractAddress, ContractState, ContractZswapState};
#[cfg(any(feature = "cloud", feature = "standalone"))]
use sqlx::FromRow;

#[cfg_attr(docsrs, doc(cfg(feature = "cloud")))]
#[cfg(feature = "cloud")]
pub mod postgres;
#[cfg_attr(docsrs, doc(cfg(feature = "standalone")))]
#[cfg(feature = "standalone")]
pub mod sqlite;

#[cfg(any(feature = "cloud", feature = "standalone"))]
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct ContractAction {
    #[sqlx(try_from = "i64")]
    pub id: u64,

    pub address: ContractAddress,

    pub state: ContractState,

    #[sqlx(json)]
    pub attributes: ContractAttributes,

    pub zswap_state: ContractZswapState,
}

#[cfg(any(feature = "cloud", feature = "standalone"))]
impl From<ContractAction> for domain::ContractAction {
    fn from(action: ContractAction) -> Self {
        let ContractAction {
            id,
            address,
            state,
            attributes,
            zswap_state,
        } = action;

        Self {
            id,
            address,
            state,
            attributes,
            zswap_state,
        }
    }
}
