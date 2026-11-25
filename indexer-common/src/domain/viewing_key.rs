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

use crate::domain::{ByteArrayLenError, ByteVec, ViewingKeyHash};
use chacha20poly1305::{
    AeadCore, ChaCha20Poly1305,
    aead::{Aead, OsRng, Payload},
};
use derive_more::{Debug, Display};
use secrecy::{ExposeSecret, SecretBox, zeroize::Zeroizing};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

pub const VIEWING_KEY_LEN: usize = 32;

/// A secret key that is encrypted at rest.
/// Attention: Do not accidentally leak the secret!
///
/// Held in a [SecretBox], which zeroizes the bytes on drop.
#[derive(Debug, Display)]
#[debug("ViewingKey(REDACTED)")]
#[display("REDACTED")]
pub struct ViewingKey(SecretBox<[u8; VIEWING_KEY_LEN]>);

impl ViewingKey {
    /// Create a viewing key by writing its bytes in place, so they are never copied.
    pub(crate) fn init_with_mut(ctr: impl FnOnce(&mut [u8; VIEWING_KEY_LEN])) -> Self {
        Self(SecretBox::init_with_mut(ctr))
    }

    /// Expose the sercret.
    pub fn expose_secret(&self) -> &[u8; VIEWING_KEY_LEN] {
        self.0.expose_secret()
    }

    /// Try to decrypt the given bytes as viewing key using ChaCha20Poly1305 AEAD with the given
    /// nonce and ciphertext and the given wallet ID.
    pub fn decrypt(
        nonce_and_ciphertext: impl AsRef<[u8]>,
        wallet_id: Uuid,
        cipher: &ChaCha20Poly1305,
    ) -> Result<Self, DecryptViewingKeyError> {
        let nonce_and_ciphertext = nonce_and_ciphertext.as_ref();

        let nonce = &nonce_and_ciphertext[0..12];
        let ciphertext = &nonce_and_ciphertext[12..];

        let payload = Payload {
            msg: ciphertext,
            aad: wallet_id.as_bytes(),
        };
        let bytes = Zeroizing::new(cipher.decrypt(nonce.into(), payload)?);
        let viewing_key = (bytes.len() == VIEWING_KEY_LEN)
            .then(|| Self::init_with_mut(|viewing_key| viewing_key.copy_from_slice(&bytes)))
            .ok_or(ByteArrayLenError(VIEWING_KEY_LEN, bytes.len()))?;

        Ok(viewing_key)
    }

    /// Encrypt this viewing key using ChaCha20Poly1305 AEAD.
    pub fn encrypt(
        &self,
        id: Uuid,
        cipher: &ChaCha20Poly1305,
    ) -> Result<ByteVec, chacha20poly1305::Error> {
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);

        let payload = Payload {
            msg: self.expose_secret(),
            aad: id.as_bytes(),
        };
        let mut ciphertext = cipher.encrypt(&nonce, payload)?;

        let mut nonce_and_ciphertext = nonce.to_vec();
        nonce_and_ciphertext.append(&mut ciphertext);

        Ok(nonce_and_ciphertext.into())
    }

    /// Return the viewing key hash (Sha256) for deduplication.
    pub fn hash(&self) -> ViewingKeyHash {
        let mut hasher = Sha256::new();
        hasher.update(self.expose_secret());
        let hash = hasher.finalize();

        <[u8; 32]>::from(hash).into()
    }
}

impl From<[u8; VIEWING_KEY_LEN]> for ViewingKey {
    fn from(bytes: [u8; VIEWING_KEY_LEN]) -> Self {
        Self(SecretBox::new(Box::new(bytes)))
    }
}

impl Clone for ViewingKey {
    fn clone(&self) -> Self {
        Self::init_with_mut(|viewing_key| viewing_key.copy_from_slice(self.expose_secret()))
    }
}

#[derive(Debug, Error)]
pub enum DecryptViewingKeyError {
    #[error("cannot decrypt secret")]
    DecryptViewingKeyError(#[from] chacha20poly1305::Error),

    #[error("cannot convert into viewing key")]
    ByteArrayLen(#[from] ByteArrayLenError),
}

#[cfg(test)]
mod tests {
    use crate::domain::ViewingKey;

    #[test]
    fn test_clone() {
        let viewing_key = ViewingKey::from([1; 32]);
        let clone = viewing_key.clone();
        assert_eq!(clone.expose_secret(), viewing_key.expose_secret());
    }
}
