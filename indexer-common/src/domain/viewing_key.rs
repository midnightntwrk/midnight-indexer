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

use crate::domain::SessionId;
use chacha20poly1305::{
    AeadCore, ChaCha20Poly1305,
    aead::{Aead, OsRng, Payload},
};
use derive_more::{AsRef, From, Into};
use midnight_transient_crypto::encryption::SecretKey;
use midnight_zswap::keys::SecretKeys;
use sha2::{Digest, Sha256};
use sqlx::{Type, types::Uuid};
use std::fmt::{self, Debug, Display};
use thiserror::Error;

/// A secret key that is encrypted at rest.
#[derive(Clone, Copy, PartialEq, Eq, Hash, AsRef, From, Into, Type)]
#[as_ref([u8])]
#[sqlx(transparent)]
pub struct ViewingKey(pub [u8; SecretKey::BYTES]);

impl ViewingKey {
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
        let bytes = bytes
            .try_into()
            .map_err(|bytes: Vec<u8>| DecryptViewingKeyError::Array(bytes.len()))?;

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
    pub fn to_session_id(&self) -> SessionId {
        let mut hasher = Sha256::new();
        hasher.update(self.0);
        let session_id = hasher.finalize();

        <[u8; 32]>::from(session_id).into()
    }

    /// For testing purposes only!
    pub fn make_for_testing_yes_i_know_what_i_am_doing() -> Self {
        let bytes = SecretKeys::from_rng_seed(&mut OsRng)
            .encryption_secret_key
            .repr();
        Self(bytes)
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

impl TryFrom<&[u8]> for ViewingKey {
    type Error = TryFromBytesForViewingKey;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        let bytes = bytes
            .try_into()
            .map_err(|_| TryFromBytesForViewingKey(SecretKey::BYTES, bytes.len()))?;
        Ok(Self(bytes))
    }
}

impl From<SecretKey> for ViewingKey {
    fn from(secret_key: SecretKey) -> Self {
        Self(secret_key.repr())
    }
}

impl From<ViewingKey> for SecretKey {
    fn from(viewing_key: ViewingKey) -> Self {
        SecretKey::from_repr(&viewing_key.0).expect("SecretKey can be created from repr")
    }
}

#[derive(Debug, Error)]
#[error("cannot create viewing key of len {0} from slice of len {1}")]
pub struct TryFromBytesForViewingKey(usize, usize);

#[derive(Debug, Error)]
pub enum DecryptViewingKeyError {
    #[error("cannot decrypt secret")]
    DecryptViewingKeyError(#[from] chacha20poly1305::Error),

    #[error("cannot create byte array of len 64 from slice of len {0}")]
    Array(usize),
}
