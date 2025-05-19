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
    domain::{NetworkId, SessionId},
    serialize::SerializableExt,
};
use chacha20poly1305::{
    aead::{Aead, OsRng, Payload},
    AeadCore, ChaCha20Poly1305,
};
use derive_more::{AsRef, From, Into};
use midnight_ledger::{
    serialize::{deserialize, Deserializable},
    transient_crypto::encryption::SecretKey,
    zswap::keys::SecretKeys,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::{types::Uuid, Type};
use std::{
    fmt::{self, Debug, Display},
    io,
};
use thiserror::Error;

/// A secret key that is encrypted at rest and does not leak its secret.
///
/// ATTENTION: Do not leak the secret! Only provide explicit methods like `validate` and
/// `deserialize` that operate on the secret.
#[derive(Clone, PartialEq, Eq, Hash, AsRef, From, Into, Deserialize, Type)]
#[as_ref([u8])]
#[sqlx(transparent)]
pub struct ViewingKey(Vec<u8>);

impl ViewingKey {
    /// Expose the secret!
    pub fn expose_secret(&self) -> &[u8] {
        &self.0
    }

    /// Try to decrypt the given bytes as [ViewingKey].
    pub fn decrypt(
        nonce_and_ciphertext: &[u8],
        id: Uuid,
        cipher: &ChaCha20Poly1305,
    ) -> Result<Self, DecryptViewingKeyError> {
        let nonce = &nonce_and_ciphertext[0..12];
        let ciphertext = &nonce_and_ciphertext[12..];

        let payload = Payload {
            msg: ciphertext,
            aad: id.as_bytes(),
        };
        let bytes = cipher.decrypt(nonce.into(), payload)?;

        Ok(Self(bytes))
    }

    /// Encrypt this [ViewingKey].
    pub fn encrypt(
        &self,
        id: Uuid,
        cipher: &ChaCha20Poly1305,
    ) -> Result<Vec<u8>, chacha20poly1305::Error> {
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);

        let payload = Payload {
            msg: &self.0,
            aad: id.as_bytes(),
        };
        let mut ciphertext = cipher.encrypt(&nonce, payload)?;

        let mut nonce_and_ciphertext = nonce.to_vec();
        nonce_and_ciphertext.append(&mut ciphertext);

        Ok(nonce_and_ciphertext)
    }

    /// Return the session ID for this [ViewingKey].
    pub fn as_session_id(&self) -> SessionId {
        let mut hasher = Sha256::new();
        hasher.update(&self.0);
        let session_id = hasher.finalize();

        <[u8; 32]>::from(session_id).into()
    }

    /// Validate that this [ViewingKey] is a valid ledger `SecretKey` by deserializing it.
    pub fn validate(&self, network_id: NetworkId) -> bool {
        self.deserialize(network_id).is_ok()
    }

    /// Deserialize this [ViewingKey] as ledger `SecretKey`.
    pub fn deserialize(&self, network_id: NetworkId) -> Result<SecretKey, io::Error> {
        SecretKey::deserialize(&mut self.as_ref(), 0)
            .or_else(|_| deserialize::<SecretKey, _>(&mut self.as_ref(), network_id.into()))
    }

    /// For testing purposes only!
    pub fn make_for_testing_yes_i_know_what_i_am_doing(network_id: NetworkId) -> ViewingKey {
        SecretKeys::from_rng_seed(&mut OsRng)
            .encryption_secret_key
            .serialize(network_id)
            .expect("SecretKey can be serialized")
            .into()
    }
}

impl Debug for ViewingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ViewingKey(REDACTED)")
    }
}

impl Display for ViewingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "REDACTED")
    }
}

#[derive(Debug, Error)]
#[error("cannot create viewing key of len 32 from slice of len {0}")]
pub struct TryIntoViewingKey(usize);

#[derive(Debug, Error)]
#[error("cannot decrypt secret")]
pub struct DecryptViewingKeyError(#[from] chacha20poly1305::Error);

#[cfg(test)]
mod tests {
    use crate::{
        domain::{NetworkId, ViewingKey},
        serialize::SerializableExt,
    };
    use assert_matches::assert_matches;
    use midnight_ledger::{
        serialize::Serializable,
        transient_crypto::encryption::SecretKey,
        zswap::keys::{SecretKeys, Seed},
    };

    const SEED_0001: &str = "0000000000000000000000000000000000000000000000000000000000000001";

    #[test]
    fn test_desesrialize() {
        let secret_key = seed_to_viewing_key(SEED_0001);

        let mut bytes = vec![];
        <SecretKey as Serializable>::serialize(&secret_key, &mut bytes)
            .expect("secret key can be serialized");
        let viewing_key = ViewingKey::from(bytes);

        assert_matches!(viewing_key.deserialize(NetworkId::DevNet), Ok(key) if key == secret_key);
        assert_matches!(viewing_key.deserialize(NetworkId::TestNet), Ok(key) if key == secret_key);

        let bytes = secret_key
            .serialize(NetworkId::DevNet)
            .expect("secret key can be serialized");
        let viewing_key = ViewingKey::from(bytes);

        assert_matches!(viewing_key.deserialize(NetworkId::DevNet), Ok(key) if key == secret_key);
        assert!(viewing_key.deserialize(NetworkId::TestNet).is_err());
    }

    /// Produce a viewing key string from a 32‐byte hex seed.
    fn seed_to_viewing_key(seed: &str) -> SecretKey {
        let seed_bytes = const_hex::decode(seed).expect("seed can be hex-decoded");
        let seed_bytes = <[u8; 32]>::try_from(seed_bytes).expect("seed has 32 bytes");
        SecretKeys::from(Seed::from(seed_bytes)).encryption_secret_key
    }
}
