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

mod bytes;
mod protocol_version;
mod pub_sub;
mod viewing_key;
mod zswap;

pub use bytes::*;
pub use protocol_version::*;
pub use pub_sub::*;
pub use viewing_key::*;
pub use zswap::*;

use derive_more::Display;
use midnight_serialize::NetworkId as LedgerNetworkId;
use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
use thiserror::Error;

pub type BlockAuthor = ByteArray<32>;
pub type ContractAddress = ByteVec;
pub type ContractEntryPoint = ByteVec;
pub type ContractState = ByteVec;
pub type ContractZswapState = ByteVec;
pub type Identifier = ByteVec;
pub type MerkleTreeRoot = ByteVec;
pub type RawTransaction = ByteVec;
pub type SessionId = ByteArray<32>;

/// The apply stage of a transaction.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "cloud", sqlx(type_name = "APPLY_STAGE"))]
pub enum ApplyStage {
    /// Guaranteed and fallible coins succeeded.
    Success,

    /// Only guaranteed coins succeeded.
    PartialSuccess,

    /// Both guaranteed and fallible coins failed.
    #[default]
    Failure,
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

/// Clone of midnight_serialize::NetworkId for the purpose of Serde deserialization.
#[derive(Debug, Display, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum NetworkId {
    Undeployed,
    DevNet,
    TestNet,
    MainNet,
}

impl From<NetworkId> for LedgerNetworkId {
    fn from(network_id: NetworkId) -> Self {
        match network_id {
            NetworkId::Undeployed => LedgerNetworkId::Undeployed,
            NetworkId::DevNet => LedgerNetworkId::DevNet,
            NetworkId::TestNet => LedgerNetworkId::TestNet,
            NetworkId::MainNet => LedgerNetworkId::MainNet,
        }
    }
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
    use midnight_serialize::NetworkId as LedgerNetworkId;

    #[test]
    fn test_network_id_from() {
        let network_id = LedgerNetworkId::from(NetworkId::Undeployed);
        assert_eq!(network_id, LedgerNetworkId::Undeployed);

        let network_id = LedgerNetworkId::from(NetworkId::DevNet);
        assert_eq!(network_id, LedgerNetworkId::DevNet);

        let network_id = LedgerNetworkId::from(NetworkId::TestNet);
        assert_eq!(network_id, LedgerNetworkId::TestNet);

        let network_id = LedgerNetworkId::from(NetworkId::MainNet);
        assert_eq!(network_id, LedgerNetworkId::MainNet);
    }

    #[test]
    fn test_network_id_deserialize() {
        let network_id = serde_json::from_str::<NetworkId>("\"Undeployed\"");
        assert_eq!(network_id.unwrap(), NetworkId::Undeployed);

        let network_id = serde_json::from_str::<NetworkId>("\"FooBarBaz\"");
        assert!(network_id.is_err());
    }
}
