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

pub mod ledger;

mod address;
mod bytes;
mod ledger_state_storage;
mod network_id;
mod protocol_version;
mod pub_sub;
mod viewing_key;

pub use address::*;
pub use bytes::*;
pub use ledger_state_storage::*;
pub use network_id::*;
pub use protocol_version::*;
pub use pub_sub::*;
use serde::{Deserialize, Serialize};
use sqlx::Type;
pub use viewing_key::*;

pub type BlockAuthor = ByteArray<32>;
pub type BlockHash = ByteArray<32>;
pub type IntentHash = ByteArray<32>;
pub type RawTokenType = ByteArray<32>;
pub type RawUnshieldedAddress = ByteArray<32>;
pub type SerializedLedgerEvent = ByteVec;

/// The result of applying a regular transaction to the ledger state along with extracted data.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ApplyRegularTransactionResult {
    pub transaction_result: TransactionResult,
    pub created_unshielded_utxos: Vec<UnshieldedUtxo>,
    pub spent_unshielded_utxos: Vec<UnshieldedUtxo>,
    pub ledger_events: Vec<LedgerEvent>,
}

/// The result of applying a transaction to the ledger state.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionResult {
    /// All guaranteed and fallible coins succeeded.
    Success,

    /// Not all fallible coins succeeded; the value maps segemt ID to success.
    PartialSuccess(Vec<(u16, bool)>),

    /// Guaranteed coins failed.
    #[default]
    Failure,
}

/// An unshielded UTXO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnshieldedUtxo {
    pub owner: RawUnshieldedAddress,
    pub token_type: RawTokenType,
    pub value: u128,
    pub intent_hash: IntentHash,
    pub output_index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerEvent {
    pub variant: LedgerEventVariant,
    pub grouping: LedgerEventGrouping,
    pub raw: SerializedLedgerEvent,
}

impl LedgerEvent {
    fn zswap_input(raw: SerializedLedgerEvent) -> Self {
        Self {
            variant: LedgerEventVariant::ZswapInput,
            grouping: LedgerEventGrouping::Zswap,
            raw,
        }
    }

    fn zswap_output(raw: SerializedLedgerEvent) -> Self {
        Self {
            variant: LedgerEventVariant::ZswapOutput,
            grouping: LedgerEventGrouping::Zswap,
            raw,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "cloud", sqlx(type_name = "LEDGER_EVENT_VARIANT"))]
pub enum LedgerEventVariant {
    ZswapInput,
    ZswapOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "cloud", sqlx(type_name = "LEDGER_EVENT_GROUPING"))]
pub enum LedgerEventGrouping {
    Zswap,
}
