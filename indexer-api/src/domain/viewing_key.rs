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
use indexer_common::domain::ViewingKey as CommonViewingKey;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Wrapper around viewing key string that supports both Bech32m and hex formats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, From)]
pub struct ViewingKey(pub String);

scalar!(ViewingKey);

#[derive(Debug, Error)]
#[error("invalid viewing key format: failed both Bech32m and hex decoding")]
pub struct ViewingKeyFormatError;

impl TryFrom<ViewingKey> for CommonViewingKey {
    type Error = ViewingKeyFormatError;

    fn try_from(key: ViewingKey) -> Result<Self, Self::Error> {
        if let Ok((_, bytes)) = bech32::decode(&key.0) {
            Ok(CommonViewingKey::from(bytes))
        } else if let Ok(bytes) = const_hex::decode(&key.0) {
            Ok(CommonViewingKey::from(bytes))
        } else {
            Err(ViewingKeyFormatError)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::ViewingKey;
    use bech32::{Bech32m, Hrp};
    use indexer_common::{
        domain::{NetworkId, ViewingKey as CommonViewingKey},
        serialize::SerializableExt,
    };
    use midnight_ledger::{
        serialize::Serializable,
        transient_crypto::encryption::SecretKey,
        zswap::keys::{SecretKeys, Seed},
    };

    const SEED_0001: &str = "0000000000000000000000000000000000000000000000000000000000000001";

    #[test]
    fn test_viewing_key_try_from_hex_no_network_id() {
        let secret_key = seed_to_viewing_key(SEED_0001);

        let mut bytes = vec![];
        <SecretKey as Serializable>::serialize(&secret_key, &mut bytes)
            .expect("secret key can be serialized");
        let encoded = const_hex::encode(bytes);
        let viewing_key = ViewingKey::from(encoded);

        let viewing_key = CommonViewingKey::try_from(viewing_key);
        assert!(viewing_key.is_ok());
    }

    #[test]
    fn test_viewing_key_try_from_hex_network_id() {
        let secret_key = seed_to_viewing_key(SEED_0001);

        let bytes = secret_key
            .serialize(NetworkId::DevNet)
            .expect("secret key can be serialized");
        let encoded = const_hex::encode(bytes);
        let viewing_key = ViewingKey::from(encoded);

        let viewing_key = CommonViewingKey::try_from(viewing_key);
        assert!(viewing_key.is_ok());
    }

    #[test]
    fn test_viewing_key_try_from_bech32m_no_network_id() {
        let secret_key = seed_to_viewing_key(SEED_0001);

        let mut bytes = vec![];
        <SecretKey as Serializable>::serialize(&secret_key, &mut bytes)
            .expect("secret key can be serialized");

        let hrp = Hrp::parse("foo_bar").expect("HRP is valid");
        let encoded =
            bech32::encode::<Bech32m>(hrp, &bytes).expect("secret key can be bech32m encoded");
        let viewing_key = ViewingKey::from(encoded);

        let viewing_key = CommonViewingKey::try_from(viewing_key);
        assert!(viewing_key.is_ok());
    }

    #[test]
    fn test_viewing_key_try_from_bech32m_network_id() {
        let secret_key = seed_to_viewing_key(SEED_0001);

        let bytes = secret_key
            .serialize(NetworkId::DevNet)
            .expect("secret key can be serialized");
        let hrp = Hrp::parse("foo_bar").expect("HRP is valid");
        let encoded =
            bech32::encode::<Bech32m>(hrp, &bytes).expect("secret key can be bech32m encoded");
        let viewing_key = ViewingKey::from(encoded);

        let viewing_key = CommonViewingKey::try_from(viewing_key);
        assert!(viewing_key.is_ok());
    }

    /// Produce a viewing key string from a 32‐byte hex seed.
    fn seed_to_viewing_key(seed: &str) -> SecretKey {
        let seed_bytes = const_hex::decode(seed).expect("seed can be hex-decoded");
        let seed_bytes = <[u8; 32]>::try_from(seed_bytes).expect("seed has 32 bytes");
        SecretKeys::from(Seed::from(seed_bytes)).encryption_secret_key
    }
}
