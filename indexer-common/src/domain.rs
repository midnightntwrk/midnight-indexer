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
use serde::Deserialize;
use std::str::FromStr;
use thiserror::Error;

pub type BlockAuthor = ByteArray<32>;
pub type BlockHash = ByteArray<32>;

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
