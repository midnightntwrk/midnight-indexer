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

mod bytes;
mod ledger_state_storage;
mod protocol_version;
mod pub_sub;
mod viewing_key;

pub use bytes::*;
pub use ledger_state_storage::*;
pub use protocol_version::*;
pub use pub_sub::*;
pub use viewing_key::*;

use derive_more::Display;
use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
use thiserror::Error;

pub type BlockAuthor = ByteArray<32>;
pub type BlockHash = ByteArray<32>;
pub type ContractEntryPoint = ByteVec;
pub type DustCommitment = ByteArray<32>;
pub type DustNonce = ByteArray<32>;
pub type DustNullifier = ByteArray<32>;
pub type DustOwner = ByteArray<32>;
pub type NightUtxoHash = ByteArray<32>;
pub type NightUtxoNonce = ByteArray<32>;
pub type IntentHash = ByteArray<32>;
pub type RawContractAddress = ByteVec;
pub type RawContractState = ByteVec;
pub type RawLedgerState = ByteVec;
pub type RawTokenType = ByteArray<32>;
pub type RawTransaction = ByteVec;
pub type RawTransactionIdentifier = ByteVec;
pub type RawUnshieldedAddress = ByteArray<32>;
pub type RawZswapState = ByteVec;
pub type RawZswapStateRoot = ByteVec;
pub type SessionId = ByteArray<32>;
pub type TransactionHash = ByteArray<32>;

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

/// Extended transaction result that includes events when available.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionResultWithDustEvents<T> {
    /// The basic transaction result.
    pub result: TransactionResult,
    /// Events emitted during transaction processing (if available).
    pub dust_events: Vec<T>,
}

/// A contract action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractAction {
    pub address: RawContractAddress,
    pub state: RawContractState,
    pub attributes: ContractAttributes,
}

/// Attributes for a specific contract action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ContractAttributes {
    Deploy,
    Call { entry_point: ContractEntryPoint },
    Update,
}

/// The variant of a contract action.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "cloud", sqlx(type_name = "CONTRACT_ACTION_VARIANT"))]
pub enum ContractActionVariant {
    /// A contract deployment.
    #[default]
    Deploy,

    /// A contract call.
    Call,

    /// A contract update.
    Update,
}

impl From<&ContractAttributes> for ContractActionVariant {
    fn from(attributes: &ContractAttributes) -> Self {
        match attributes {
            ContractAttributes::Deploy => Self::Deploy,
            ContractAttributes::Call { .. } => Self::Call,
            ContractAttributes::Update => Self::Update,
        }
    }
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

/// Transaction structure for fees calculation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransactionStructure {
    pub segment_count: usize,
    pub estimated_input_count: usize,
    pub estimated_output_count: usize,
    pub has_contract_operations: bool,
    pub size: usize,
}

/// Token balance of a contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContractBalance {
    /// Token type identifier.
    pub token_type: RawTokenType,

    /// Balance amount as u128.
    pub amount: u128,
}

/// Clone of midnight_serialize::NetworkId for the purpose of Serde deserialization.
#[derive(Debug, Display, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum NetworkId {
    Undeployed,
    DevNet,
    TestNet,
    MainNet,
}

impl FromStr for NetworkId {
    type Err = UnknownNetworkIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.try_into()
    }
}

impl TryFrom<&str> for NetworkId {
    type Error = UnknownNetworkIdError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s.to_lowercase().as_str() {
            "undeployed" => Ok(Self::Undeployed),
            "dev" => Ok(Self::DevNet),
            "test" => Ok(Self::TestNet),
            "" => Ok(Self::MainNet),
            _ => Err(UnknownNetworkIdError(s.to_owned())),
        }
    }
}

#[derive(Debug, Error)]
#[error("unknown NetworkId {0}")]
pub struct UnknownNetworkIdError(String);

#[cfg(test)]
mod tests {
    use crate::domain::NetworkId;

    #[test]
    fn test_network_id_deserialize() {
        let network_id = serde_json::from_str::<NetworkId>("\"Undeployed\"");
        assert_eq!(network_id.unwrap(), NetworkId::Undeployed);

        let network_id = serde_json::from_str::<NetworkId>("\"FooBarBaz\"");
        assert!(network_id.is_err());
    }
}
