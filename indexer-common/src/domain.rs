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
pub use viewing_key::*;

use serde::{Deserialize, Serialize};
use sqlx::Type;

pub type BlockAuthor = ByteArray<32>;
pub type BlockHash = ByteArray<32>;
pub type IntentHash = ByteArray<32>;
pub type RawTokenType = ByteArray<32>;
pub type RawUnshieldedAddress = ByteArray<32>;
pub type SerializedContractAddress = ByteVec;
pub type SerializedContractEntryPoint = ByteVec;
pub type SerializedContractState = ByteVec;
pub type SerializedLedgerEvent = ByteVec;
pub type SerializedLedgerState = ByteVec;
pub type SerializedTransaction = ByteVec;
pub type SerializedTransactionIdentifier = ByteVec;
pub type SerializedZswapState = ByteVec;
pub type SerializedZswapStateRoot = ByteVec;
pub type TransactionHash = ByteArray<32>;

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

/// A contract action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractAction {
    pub address: SerializedContractAddress,
    pub state: SerializedContractState,
    pub attributes: ContractAttributes,
}

/// Attributes for a specific contract action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContractAttributes {
    Deploy,
    Call {
        entry_point: SerializedContractEntryPoint,
    },
    Update,
}

/// Token balance of a contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContractBalance {
    /// Token type identifier.
    pub token_type: RawTokenType,

    /// Balance amount as u128.
    pub amount: u128,
}

/// Transaction structure for fees calculation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransactionStructure {
    pub segment_count: usize,
    pub estimated_input_count: usize,
    pub estimated_output_count: usize,
    pub has_contract_operations: bool,
    pub size: usize,
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
    pub grouping: LedgerEventGrouping,
    pub raw: SerializedLedgerEvent,
    pub attributes: LedgerEventAttributes,
}

impl LedgerEvent {
    fn zswap_input(raw: SerializedLedgerEvent) -> Self {
        Self {
            grouping: LedgerEventGrouping::Zswap,
            raw,
            attributes: LedgerEventAttributes::ZswapInput,
        }
    }

    fn zswap_output(raw: SerializedLedgerEvent) -> Self {
        Self {
            grouping: LedgerEventGrouping::Zswap,
            raw,
            attributes: LedgerEventAttributes::ZswapOutput,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LedgerEventAttributes {
    ZswapInput,
    ZswapOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "cloud", sqlx(type_name = "LEDGER_EVENT_GROUPING"))]
pub enum LedgerEventGrouping {
    Zswap,
}
