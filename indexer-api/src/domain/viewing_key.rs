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

use async_graphql::scalar;
use derive_more::derive::From;
use indexer_common::domain::{
    NetworkId, TryFromBytesForViewingKey, UnknownNetworkIdError, ViewingKey as CommonViewingKey,
};
use midnight_ledger::{serialize::Deserializable, transient_crypto::encryption::SecretKey};
use serde::{Deserialize, Serialize};
use std::io;
use thiserror::Error;

/// Wrapper around Bech32m encoded viewing key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, From)]
#[from(String, &str)]
pub struct ViewingKey(pub String);

scalar!(ViewingKey);

impl ViewingKey {
    /// Converts this API viewing key into a domain viewing key, validating the bech32m format and
    /// network ID and deserializing the bech32m data.
    ///
    /// Format expectations:
    /// - For mainnet: "mn_shield-esk" + bech32m data
    /// - For other networks: "mn_shield-esk_" + network-id + bech32m data where network-id is one
    ///   of: "dev", "test", "undeployed"
    pub fn try_into_domain(
        self,
        network_id: NetworkId,
    ) -> Result<CommonViewingKey, ViewingKeyFormatError> {
        let (hrp, bytes) = bech32::decode(&self.0).map_err(ViewingKeyFormatError::Decode)?;
        let hrp = hrp.to_lowercase();

        let Some(n) = hrp.strip_prefix("mn_shield-esk") else {
            return Err(ViewingKeyFormatError::InvalidHrp(hrp));
        };
        let n = n.strip_prefix("_").unwrap_or(n).try_into()?;
        if n != network_id {
            return Err(ViewingKeyFormatError::UnexpectedNetworkId(n));
        }

        SecretKey::deserialize(&mut bytes.as_slice(), 0)?
            .repr()
            .as_slice()
            .try_into()
            .map_err(ViewingKeyFormatError::Array)
    }
}

#[derive(Debug, Error)]
pub enum ViewingKeyFormatError {
    #[error("cannot bech32m-decode viewing key")]
    Decode(#[from] bech32::DecodeError),

    #[error("unexpected bech32m HRP {0}")]
    InvalidHrp(String),

    #[error(transparent)]
    TryFromStrForNetworkIdError(#[from] UnknownNetworkIdError),

    #[error("unexpected network ID {0}")]
    UnexpectedNetworkId(NetworkId),

    #[error("cannot deserialize viewing key")]
    Deserialize(#[from] io::Error),

    #[error(transparent)]
    Array(TryFromBytesForViewingKey),
}

#[cfg(test)]
mod tests {
    use crate::domain::{ViewingKey, ViewingKeyFormatError};
    use assert_matches::assert_matches;
    use indexer_common::domain::{NetworkId, ViewingKey as CommonViewingKey};
    use midnight_ledger::zswap::keys::{SecretKeys, Seed};

    #[test]
    fn test() {
        let viewing_key = ViewingKey::from(
            "mn_shield-esk1qvqzq76fwwf9jqlv0uqywd9jk0q4klqk44qyc809que8q0v309z8u7qzzyn2qx",
        )
        .try_into_domain(NetworkId::MainNet);
        assert_matches!(viewing_key, Ok(key) if key == seed_to_viewing_key("0000000000000000000000000000000000000000000000000000000000000000"));

        let viewing_key = ViewingKey::from(
            "mn_shield-esk1qvqzq76fwwf9jqlv0uqywd9jk0q4klqk44qyc809que8q0v309z8u7qzzyn2qx",
        )
        .try_into_domain(NetworkId::DevNet);
        assert_matches!(
            viewing_key,
            Err(ViewingKeyFormatError::UnexpectedNetworkId(
                NetworkId::MainNet
            ))
        );

        let viewing_key = ViewingKey::from(
            "mn_shield-esk_dev1qvqzq76fwwf9jqlv0uqywd9jk0q4klqk44qyc809que8q0v309z8u7qz7pax9d",
        )
        .try_into_domain(NetworkId::DevNet);
        assert_matches!(viewing_key, Ok(key) if key == seed_to_viewing_key("0000000000000000000000000000000000000000000000000000000000000000"));
    }

    fn seed_to_viewing_key(seed: &str) -> CommonViewingKey {
        let seed_bytes = const_hex::decode(seed).expect("seed can be hex-decoded");
        let seed_bytes = <[u8; 32]>::try_from(seed_bytes).expect("seed has 32 bytes");
        SecretKeys::from(Seed::from(seed_bytes))
            .encryption_secret_key
            .into()
    }
}
