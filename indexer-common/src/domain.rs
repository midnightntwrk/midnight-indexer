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

pub mod dust;
pub mod ledger;

mod address;
mod bytes;
mod ledger_state_storage;
mod protocol_version;
mod pub_sub;
mod viewing_key;

use std::str::FromStr;

pub use address::*;
pub use bytes::*;
use derive_more::{Deref, Display, Into};
pub use ledger_state_storage::*;
pub use protocol_version::*;
pub use pub_sub::*;
use thiserror::Error;
pub use viewing_key::*;

use serde::{Deserialize, Serialize};
use sqlx::Type;

// Plain bytes: very simple hashes/identifiers used without serialization.
pub type BlockAuthor = ByteArray<32>;
pub type BlockHash = ByteArray<32>;
pub type IntentHash = ByteArray<32>;
pub type Nonce = ByteArray<32>;
pub type SessionId = ByteArray<32>;
pub type TokenType = ByteArray<32>;
pub type TransactionHash = ByteArray<32>;
pub type UnshieldedAddress = ByteArray<32>;

// DUST-specific types for dustGenerationStatus query.
pub type DustOwner = ByteArray<32>;
pub type DustAddress = ByteArray<32>;
pub type CardanoStakeKey = ByteVec;
pub type NightUtxoHash = ByteArray<32>;
pub type DustUtxoId = ByteVec;

// Untagged serialization: simple and/or stable types that are not expected to change.
pub type SerializedTransactionIdentifier = ByteVec;
pub type SerializedZswapStateRoot = ByteVec;

// Tagged serialization: complex types that may evolve; tags allow version-awareness.
pub type SerializedContractAddress = ByteVec;
pub type SerializedContractState = ByteVec;
pub type SerializedLedgerEvent = ByteVec;
pub type SerializedLedgerParameters = ByteVec;
pub type SerializedLedgerState = ByteVec;
pub type SerializedTransaction = ByteVec;
pub type SerializedZswapState = ByteVec;

/// Network identifier.
#[derive(Debug, Display, Clone, PartialEq, Eq, Hash, Deref, Into, Deserialize)]
#[deref(forward)]
#[serde(try_from = "String")]
pub struct NetworkId(pub String);

impl TryFrom<String> for NetworkId {
    type Error = InvalidNetworkIdError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        if s.is_empty() {
            Err(InvalidNetworkIdError::Empty)
        } else {
            Ok(Self(s))
        }
    }
}

impl TryFrom<&str> for NetworkId {
    type Error = InvalidNetworkIdError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.to_owned().try_into()
    }
}

impl FromStr for NetworkId {
    type Err = InvalidNetworkIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.try_into()
    }
}

#[derive(Debug, Error)]
pub enum InvalidNetworkIdError {
    #[error("network ID must not be empty")]
    Empty,
}

/// The outcome of applying a regular transaction to the ledger state along with extracted data.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ApplyRegularTransactionOutcome {
    pub transaction_result: TransactionResult,
    pub created_unshielded_utxos: Vec<UnshieldedUtxo>,
    pub spent_unshielded_utxos: Vec<UnshieldedUtxo>,
    pub ledger_events: Vec<LedgerEvent>,
}

/// The outcome of applying a system transaction to the ledger state along with extracted data.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ApplySystemTransactionOutcome {
    pub created_unshielded_utxos: Vec<UnshieldedUtxo>,
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

/// The variant of a transaction: regular or system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Type)]
#[cfg_attr(feature = "cloud", sqlx(type_name = "TRANSACTION_VARIANT"))]
pub enum TransactionVariant {
    Regular,
    System,
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
    Call { entry_point: String },
    Update,
}

/// Token balance of a contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContractBalance {
    /// Token type identifier.
    pub token_type: TokenType,

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
    pub owner: UnshieldedAddress,
    pub token_type: TokenType,
    pub value: u128,
    pub intent_hash: IntentHash,
    pub output_index: u32,
    pub ctime: Option<u64>,
    pub initial_nonce: Nonce,
    pub registered_for_dust_generation: bool,
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

    fn param_change(raw: SerializedLedgerEvent) -> Self {
        Self {
            grouping: LedgerEventGrouping::Dust,
            raw,
            attributes: LedgerEventAttributes::ParamChange,
        }
    }

    fn dust_initial_utxo(
        raw: SerializedLedgerEvent,
        output: dust::QualifiedDustOutput,
        generation_info: dust::DustGenerationInfo,
        generation_index: u64,
    ) -> Self {
        Self {
            grouping: LedgerEventGrouping::Dust,
            raw,
            attributes: LedgerEventAttributes::DustInitialUtxo {
                output,
                generation_info,
                generation_index,
            },
        }
    }

    fn dust_generation_dtime_update(
        raw: SerializedLedgerEvent,
        generation_info: dust::DustGenerationInfo,
        generation_index: u64,
        merkle_path: Vec<dust::DustMerklePathEntry>,
    ) -> Self {
        Self {
            grouping: LedgerEventGrouping::Dust,
            raw,
            attributes: LedgerEventAttributes::DustGenerationDtimeUpdate {
                generation_info,
                generation_index,
                merkle_path,
            },
        }
    }

    fn dust_spend_processed(raw: SerializedLedgerEvent) -> Self {
        Self {
            grouping: LedgerEventGrouping::Dust,
            raw,
            attributes: LedgerEventAttributes::DustSpendProcessed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LedgerEventAttributes {
    ZswapInput,

    ZswapOutput,

    ParamChange,

    DustInitialUtxo {
        output: dust::QualifiedDustOutput,
        generation_info: dust::DustGenerationInfo,
        generation_index: u64,
    },

    DustGenerationDtimeUpdate {
        generation_info: dust::DustGenerationInfo,
        generation_index: u64,
        merkle_path: Vec<dust::DustMerklePathEntry>,
    },

    DustSpendProcessed,
}

/// Minimal DUST output info for backwards compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DustOutput {
    pub nonce: Nonce,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "cloud", sqlx(type_name = "LEDGER_EVENT_GROUPING"))]
pub enum LedgerEventGrouping {
    Zswap,
    Dust,
}
