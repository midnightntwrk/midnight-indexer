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

use crate::{
    domain::LedgerEvent,
    infra::api::v3::{AsBytesExt, HexEncoded},
};
use async_graphql::{Interface, SimpleObject};
use indexer_common::domain::LedgerEventAttributes;

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

/// A dust related ledger event.
#[derive(Debug, Interface)]
#[allow(clippy::duplicated_attributes)]
#[graphql(
    field(name = "id", ty = "&u64"),
    field(name = "raw", ty = "&HexEncoded"),
    field(name = "max_id", ty = "&u64")
)]
pub enum DustLedgerEvent {
    // A general parameter change; possibly conveys modified dust related parameters like
    // generation rate or decay.
    ParamChange(ParamChange),

    // An initial dust UTXO.
    DustInitialUtxo(DustInitialUtxo),

    // A dtime update for a dust generation.
    DustGenerationDtimeUpdate(DustGenerationDtimeUpdate),

    // A processed dust spend.
    DustSpendProcessed(DustSpendProcessed),
}

impl From<LedgerEvent> for DustLedgerEvent {
    fn from(ledger_event: LedgerEvent) -> Self {
        match ledger_event.attributes {
            LedgerEventAttributes::ParamChange => DustLedgerEvent::ParamChange(ParamChange {
                id: ledger_event.id,
                raw: ledger_event.raw.hex_encode(),
                max_id: ledger_event.max_id,
            }),

            LedgerEventAttributes::DustInitialUtxo { output, .. } => {
                DustLedgerEvent::DustInitialUtxo(DustInitialUtxo {
                    id: ledger_event.id,
                    raw: ledger_event.raw.hex_encode(),
                    max_id: ledger_event.max_id,
                    output: DustOutput {
                        nonce: output.nonce.hex_encode(),
                    },
                })
            }

            LedgerEventAttributes::DustGenerationDtimeUpdate { .. } => {
                DustLedgerEvent::DustGenerationDtimeUpdate(DustGenerationDtimeUpdate {
                    id: ledger_event.id,
                    raw: ledger_event.raw.hex_encode(),
                    max_id: ledger_event.max_id,
                })
            }

            LedgerEventAttributes::DustSpendProcessed => {
                DustLedgerEvent::DustSpendProcessed(DustSpendProcessed {
                    id: ledger_event.id,
                    raw: ledger_event.raw.hex_encode(),
                    max_id: ledger_event.max_id,
                })
            }

            other => panic!("unexpected Dust ledger event: {other:?}"),
        }
    }
}

#[derive(Debug, SimpleObject)]
// A general parameter change; possibly conveys modified dust related parameters like
// generation rate or decay.
pub struct ParamChange {
    /// The ID of this dust ledger event.
    id: u64,

    /// The hex-encoded serialized event.
    raw: HexEncoded,

    /// The maximum ID of all dust ledger events.
    max_id: u64,
}

// An initial dust UTXO.
#[derive(Debug, SimpleObject)]
pub struct DustInitialUtxo {
    /// The ID of this dust ledger event.
    id: u64,

    /// The hex-encoded serialized event.
    raw: HexEncoded,

    /// The maximum ID of all dust ledger events.
    max_id: u64,

    /// The dust output.
    output: DustOutput,
}

// A dtime update for a dust generation.
#[derive(Debug, SimpleObject)]
pub struct DustGenerationDtimeUpdate {
    /// The ID of this dust ledger event.
    id: u64,

    /// The hex-encoded serialized event.
    raw: HexEncoded,

    /// The maximum ID of all dust ledger events.
    max_id: u64,
}

// A processed dust spend.
#[derive(Debug, SimpleObject)]
pub struct DustSpendProcessed {
    /// The ID of this dust ledger event.
    id: u64,

    /// The hex-encoded serialized event.
    raw: HexEncoded,

    /// The maximum ID of all dust ledger events.
    max_id: u64,
}

/// A dust output.
#[derive(Debug, SimpleObject)]
pub struct DustOutput {
    /// The hex-encoded 32-byte nonce.
    nonce: HexEncoded,
}

impl From<indexer_common::domain::DustOutput> for DustOutput {
    fn from(dust_output: indexer_common::domain::DustOutput) -> Self {
        Self {
            nonce: dust_output.nonce.hex_encode(),
        }
    }
}
