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

use async_graphql::SimpleObject;

use crate::{
    domain::LedgerEvent,
    infra::api::v1::{AsBytesExt, HexEncoded},
};

/// A zswap related ledger event.
#[derive(Debug, SimpleObject)]
pub struct ZswapLedgerEvent {
    /// The ID of this zswap ledger event.
    id: u64,

    /// The hex-encoded serialized event.
    raw: HexEncoded,

    /// The maximum ID of all zswap ledger events.
    max_id: u64,
}

impl From<LedgerEvent> for ZswapLedgerEvent {
    fn from(ledger_event: LedgerEvent) -> Self {
        Self {
            id: ledger_event.id,
            raw: ledger_event.raw.hex_encode(),
            max_id: ledger_event.max_id,
        }
    }
}
