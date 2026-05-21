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

pub mod dust;
pub mod ledger;

mod bytes;
mod protocol_version;
mod pub_sub;
mod viewing_key;

pub use bytes::*;
pub use protocol_version::*;
pub use pub_sub::*;
pub use viewing_key::*;

use derive_more::{Deref, Display, Into};
use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
use thiserror::Error;

// Plain bytes: very simple hashes/identifiers used without serialization.
pub type BlockAuthor = ByteArray<32>;
pub type BlockHash = ByteArray<32>;
pub type IntentHash = ByteArray<32>;
pub type Nonce = ByteArray<32>;
pub type SessionId = ByteArray<32>;
pub type ViewingKeyHash = ByteArray<32>;
pub type TermsAndConditionsHash = ByteArray<32>;
pub type TokenType = ByteArray<32>;
pub type TransactionHash = ByteArray<32>;
pub type UnshieldedAddress = ByteArray<32>;

// DUST-specific types for dustGenerationStatus query.
pub type DustPublicKey = ByteVec;
pub type CardanoRewardAddress = ByteArray<29>;
pub type NightUtxoHash = ByteArray<32>;
pub type DustUtxoId = ByteVec;

// Untagged serialization: simple and/or stable types that are not expected to change.
pub type SerializedLedgerStateKey = ByteVec;
pub type SerializedTransactionIdentifier = ByteVec;
pub type SerializedZswapMerkleTreeRoot = ByteVec;

// Tagged serialization: complex types that may evolve; tags allow version-awareness.
pub type SerializedContractAddress = ByteVec;
pub type SerializedContractState = ByteVec;
pub type SerializedDustTreeInsertionPath = ByteVec;
pub type SerializedLedgerEvent = ByteVec;
pub type SerializedLedgerParameters = ByteVec;
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
        } else if s.chars().any(|c| c.is_uppercase()) {
            Err(InvalidNetworkIdError::NotLowercase(s))
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
    #[error("network ID must be all lowercase (was: {0})")]
    NotLowercase(String),
}

#[cfg(test)]
mod network_id_tests {
    use crate::domain::{InvalidNetworkIdError, NetworkId};

    #[test]
    fn test_valid_lowercase_network_id() {
        assert!(NetworkId::try_from("qanet".to_string()).is_ok());
    }

    #[test]
    fn test_reject_mixed_case_network_id() {
        let result = NetworkId::try_from("DevNet".to_string());
        assert!(matches!(
            result,
            Err(InvalidNetworkIdError::NotLowercase(_))
        ));
    }
}

/// A timestamp in milliseconds since the Unix epoch (Substrate Timestamp pallet convention).
/// Use when working with values from the `blocks.timestamp` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimestampMs(pub u64);

/// A timestamp in seconds since the Unix epoch (ledger convention).
/// Use when working with values from `dust_generation_info.ctime`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimestampSecs(pub u64);

impl TimestampSecs {
    /// Convert to milliseconds.
    pub fn to_ms(self) -> TimestampMs {
        TimestampMs(self.0 * 1000)
    }
}

impl TimestampMs {
    /// Calculate elapsed seconds since an earlier timestamp.
    pub fn elapsed_seconds_since(self, earlier: TimestampMs) -> u64 {
        self.0.saturating_sub(earlier.0) / 1000
    }
}

/// The outcome of applying a regular transaction to the ledger state along with extracted data.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ApplyRegularTransactionOutcome {
    pub transaction_result: TransactionResult,
    pub created_unshielded_utxos: Vec<UnshieldedUtxo>,
    pub spent_unshielded_utxos: Vec<UnshieldedUtxo>,
    pub ledger_events: Vec<LedgerEvent>,
    pub fees: u128,
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
    /// Emitting contract address. Populated only for contract events
    /// (`LedgerEventGrouping::Contract`); `None` for zswap/dust events. Mapped
    /// onto the indexed `ledger_events.contract_address` column for fast
    /// filtering on the `contractEvents` query/subscription surface.
    pub contract_address: Option<ByteVec>,
}

impl LedgerEvent {
    fn zswap_input(raw: SerializedLedgerEvent, nullifier: ByteVec) -> Self {
        Self {
            grouping: LedgerEventGrouping::Zswap,
            raw,
            attributes: LedgerEventAttributes::ZswapInput { nullifier },
            contract_address: None,
        }
    }

    fn zswap_output(raw: SerializedLedgerEvent) -> Self {
        Self {
            grouping: LedgerEventGrouping::Zswap,
            raw,
            attributes: LedgerEventAttributes::ZswapOutput,
            contract_address: None,
        }
    }

    fn param_change(raw: SerializedLedgerEvent) -> Self {
        Self {
            grouping: LedgerEventGrouping::Dust,
            raw,
            attributes: LedgerEventAttributes::ParamChange,
            contract_address: None,
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
            contract_address: None,
        }
    }

    fn dust_generation_dtime_update(
        raw: SerializedLedgerEvent,
        generation_info: dust::DustGenerationInfo,
        generation_index: u64,
        tree_insertion_path: SerializedDustTreeInsertionPath,
    ) -> Self {
        Self {
            grouping: LedgerEventGrouping::Dust,
            raw,
            attributes: LedgerEventAttributes::DustGenerationDtimeUpdate {
                generation_info,
                generation_index,
                tree_insertion_path,
            },
            contract_address: None,
        }
    }

    fn dust_spend_processed(
        raw: SerializedLedgerEvent,
        nullifier: ByteVec,
        commitment: ByteVec,
    ) -> Self {
        Self {
            grouping: LedgerEventGrouping::Dust,
            raw,
            attributes: LedgerEventAttributes::DustSpendProcessed {
                nullifier,
                commitment,
            },
            contract_address: None,
        }
    }

    /// Construct a contract event from already-typed attributes. Use when the
    /// chain-indexer has parsed `VersionedLogItem` into a known `LogEventType`
    /// variant via `make_ledger_events_v9` (see ticket #1158).
    pub fn contract_event(
        raw: SerializedLedgerEvent,
        contract_address: ByteVec,
        attributes: LedgerEventAttributes,
    ) -> Self {
        debug_assert!(
            matches!(
                attributes,
                LedgerEventAttributes::ContractShieldedSpend { .. }
                    | LedgerEventAttributes::ContractShieldedReceive { .. }
                    | LedgerEventAttributes::ContractShieldedMint { .. }
                    | LedgerEventAttributes::ContractShieldedBurn { .. }
                    | LedgerEventAttributes::ContractUnshieldedSpend { .. }
                    | LedgerEventAttributes::ContractUnshieldedReceive { .. }
                    | LedgerEventAttributes::ContractUnshieldedMint { .. }
                    | LedgerEventAttributes::ContractUnshieldedBurn { .. }
                    | LedgerEventAttributes::ContractPaused { .. }
                    | LedgerEventAttributes::ContractUnpaused { .. }
                    | LedgerEventAttributes::ContractMisc { .. }
            ),
            "contract_event() called with non-contract attributes"
        );
        Self {
            grouping: LedgerEventGrouping::Contract,
            raw,
            attributes,
            contract_address: Some(contract_address),
        }
    }

    /// Extract the indexed-field rows for the sidecar storage. Returns
    /// `(field_name, field_value)` pairs covering every "hint: indexed" field
    /// from CoIP-442 Appendix A for the matching variant. Empty for
    /// non-contract events and for `Paused`/`Unpaused`/`Misc` (no indexable
    /// fields per the design).
    pub fn indexable_contract_fields(&self) -> Vec<(&'static str, ByteVec)> {
        use LedgerEventAttributes::*;
        match &self.attributes {
            ContractShieldedSpend { nullifier, .. } => {
                vec![("nullifier", nullifier.clone())]
            }
            ContractShieldedReceive {
                commitment,
                ciphertext,
                ..
            } => {
                let mut out = vec![("commitment", commitment.clone())];
                if let Some(c) = ciphertext {
                    out.push(("ciphertext", c.clone()));
                }
                out
            }
            ContractShieldedMint {
                commitment,
                domain_sep,
                ..
            } => vec![
                ("commitment", commitment.clone()),
                ("domainSep", domain_sep.clone()),
            ],
            ContractShieldedBurn { nullifier, .. } => {
                vec![("nullifier", nullifier.clone())]
            }
            ContractUnshieldedSpend {
                sender,
                domain_sep,
                token_type,
                ..
            } => vec![
                ("sender", sender.as_bytes()),
                ("domainSep", domain_sep.clone()),
                ("tokenType", token_type.clone()),
            ],
            ContractUnshieldedReceive {
                recipient,
                domain_sep,
                token_type,
                ..
            } => vec![
                ("recipient", recipient.as_bytes()),
                ("domainSep", domain_sep.clone()),
                ("tokenType", token_type.clone()),
            ],
            ContractUnshieldedMint {
                domain_sep,
                token_type,
                ..
            } => vec![
                ("domainSep", domain_sep.clone()),
                ("tokenType", token_type.clone()),
            ],
            ContractUnshieldedBurn {
                sender, token_type, ..
            } => vec![
                ("sender", sender.as_bytes()),
                ("tokenType", token_type.clone()),
            ],
            ContractPaused { .. }
            | ContractUnpaused { .. }
            | ContractMisc { .. }
            | ZswapInput { .. }
            | ZswapOutput
            | ParamChange
            | DustInitialUtxo { .. }
            | DustGenerationDtimeUpdate { .. }
            | DustSpendProcessed { .. } => vec![],
        }
    }
}

impl AddressOrContract {
    /// Flat byte representation for sidecar storage (32 bytes). Indexer
    /// filters by the raw bytes regardless of whether the address is a user
    /// or contract; the `kind` discriminator is in the JSONB payload only.
    pub fn as_bytes(&self) -> ByteVec {
        match self {
            AddressOrContract::User(b) => b.clone(),
            AddressOrContract::Contract(b) => b.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LedgerEventAttributes {
    ZswapInput {
        nullifier: ByteVec,
    },

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
        /// Tagged-serialised `TreeInsertionPath<DustGenerationInfo>` from the
        /// originating ledger event. Surfaced verbatim on the GraphQL API so
        /// wallets can hand it to `generating_tree.update_from_evidence(...)`.
        tree_insertion_path: SerializedDustTreeInsertionPath,
    },

    DustSpendProcessed {
        nullifier: ByteVec,
        commitment: ByteVec,
    },

    // ------------------------------------------------------------------------
    // Contract events (MIP-107 / CoIP-442). One variant per `LogEventType` from
    // `onchain-vm/src/ops.rs`. Field shapes follow CoIP-442 Appendix A head.
    // `entry_point` is the originating call's entry point (from
    // `EventDetailsV9::ContractLog.entry_point`), used by the nested
    // ContractCall.contractEvents surface for correlation.
    // ------------------------------------------------------------------------
    ContractShieldedSpend {
        version: u32,
        entry_point: ByteVec,
        nullifier: ByteVec,
    },

    ContractShieldedReceive {
        version: u32,
        entry_point: ByteVec,
        commitment: ByteVec,
        ciphertext: Option<ByteVec>,
        receiving_contract_address: Option<ByteVec>,
    },

    ContractShieldedMint {
        version: u32,
        entry_point: ByteVec,
        commitment: ByteVec,
        domain_sep: ByteVec,
        amount: Option<String>,
    },

    ContractShieldedBurn {
        version: u32,
        entry_point: ByteVec,
        nullifier: ByteVec,
        amount: Option<String>,
    },

    ContractUnshieldedSpend {
        version: u32,
        entry_point: ByteVec,
        sender: AddressOrContract,
        domain_sep: ByteVec,
        token_type: ByteVec,
        amount: String,
    },

    ContractUnshieldedReceive {
        version: u32,
        entry_point: ByteVec,
        recipient: AddressOrContract,
        domain_sep: ByteVec,
        token_type: ByteVec,
        amount: String,
    },

    ContractUnshieldedMint {
        version: u32,
        entry_point: ByteVec,
        domain_sep: ByteVec,
        token_type: ByteVec,
        amount: String,
    },

    ContractUnshieldedBurn {
        version: u32,
        entry_point: ByteVec,
        sender: AddressOrContract,
        token_type: ByteVec,
        amount: String,
    },

    ContractPaused {
        version: u32,
        entry_point: ByteVec,
    },

    ContractUnpaused {
        version: u32,
        entry_point: ByteVec,
    },

    ContractMisc {
        version: u32,
        entry_point: ByteVec,
        name: ByteVec,
        payload: ByteVec,
    },
}

/// Tagged union for fields like `Either<ZswapCoinPublicKey, ContractAddress>`
/// used in standard unshielded contract events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AddressOrContract {
    /// User wallet address (Zswap coin public key).
    User(ByteVec),
    /// Contract address.
    Contract(ByteVec),
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
    Contract,
}
