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

use chacha20poly1305::{
    AeadCore, ChaCha20Poly1305,
    aead::{Aead, OsRng, Payload},
};
use indexer_common::domain::{ByteVec, VIEWING_KEY_LEN, ViewingKey};
use sqlx::types::time::OffsetDateTime;
use std::time::Duration;
use thiserror::Error;

/// Domain separation from the viewing-key-at-rest encryption, which uses the wallet ID as AAD.
const AAD: &[u8] = b"midnight-indexer-session-token-v1";

const NONCE_LEN: usize = 12;
const PAYLOAD_LEN: usize = VIEWING_KEY_LEN + 8 + 8;
const TAG_LEN: usize = 16;

/// Length of a sealed session token; distinct from the 32-byte legacy session ID.
pub const SESSION_TOKEN_LEN: usize = NONCE_LEN + PAYLOAD_LEN + TAG_LEN;

/// A self-contained wallet session: the payload is sealed with the shared server-side cipher, so
/// any API instance sharing the same secret can validate a token, no matter which instance handled
/// `connect`. Like the legacy session ID, a token is a bearer credential and must stay secret.
#[derive(Debug)]
pub struct SessionToken {
    pub viewing_key: ViewingKey,
    pub start_index: u64,

    /// Unix timestamp in seconds.
    pub issued_at: i64,
}

impl SessionToken {
    /// Seal this session token with a fresh random nonce: `nonce || AEAD ciphertext`.
    pub fn seal(&self, cipher: &ChaCha20Poly1305) -> Result<ByteVec, chacha20poly1305::Error> {
        let mut payload = [0; PAYLOAD_LEN];
        payload[..32].copy_from_slice(self.viewing_key.expose_secret().as_ref());
        payload[32..40].copy_from_slice(&self.start_index.to_be_bytes());
        payload[40..].copy_from_slice(&self.issued_at.to_be_bytes());

        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
        let mut ciphertext = cipher.encrypt(
            &nonce,
            Payload {
                msg: &payload,
                aad: AAD,
            },
        )?;

        let mut token = nonce.to_vec();
        token.append(&mut ciphertext);

        Ok(token.into())
    }

    /// Open and authenticate a sealed session token and verify it is no older than `ttl`.
    pub fn open(
        token: &[u8],
        cipher: &ChaCha20Poly1305,
        ttl: Duration,
    ) -> Result<Self, OpenSessionTokenError> {
        if token.len() != SESSION_TOKEN_LEN {
            return Err(OpenSessionTokenError::Invalid);
        }

        let (nonce, ciphertext) = token.split_at(NONCE_LEN);
        let payload = cipher
            .decrypt(
                nonce.into(),
                Payload {
                    msg: ciphertext,
                    aad: AAD,
                },
            )
            .map_err(|_| OpenSessionTokenError::Invalid)?;

        let viewing_key = ViewingKey::from(
            <[u8; 32]>::try_from(&payload[..32]).map_err(|_| OpenSessionTokenError::Invalid)?,
        );
        let start_index = u64::from_be_bytes(payload[32..40].try_into().expect("8 bytes"));
        let issued_at = i64::from_be_bytes(payload[40..].try_into().expect("8 bytes"));

        let age = OffsetDateTime::now_utc().unix_timestamp() - issued_at;
        if age > ttl.as_secs() as i64 {
            return Err(OpenSessionTokenError::Expired);
        }

        Ok(Self {
            viewing_key,
            start_index,
            issued_at,
        })
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum OpenSessionTokenError {
    #[error("invalid session token")]
    Invalid,

    #[error("expired session token")]
    Expired,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chacha20poly1305::KeyInit;

    fn cipher(key: u8) -> ChaCha20Poly1305 {
        ChaCha20Poly1305::new(&[key; 32].into())
    }

    fn token() -> SessionToken {
        SessionToken {
            viewing_key: [7; 32].into(),
            start_index: 42,
            issued_at: OffsetDateTime::now_utc().unix_timestamp(),
        }
    }

    #[test]
    fn roundtrip() {
        let sealed = token().seal(&cipher(0)).unwrap();
        assert_eq!(sealed.as_ref().len(), SESSION_TOKEN_LEN);

        let opened =
            SessionToken::open(sealed.as_ref(), &cipher(0), Duration::from_secs(60)).unwrap();
        assert_eq!(opened.viewing_key, [7; 32].into());
        assert_eq!(opened.start_index, 42);
    }

    #[test]
    fn expired() {
        let sealed = SessionToken {
            issued_at: OffsetDateTime::now_utc().unix_timestamp() - 100,
            ..token()
        }
        .seal(&cipher(0))
        .unwrap();

        let result = SessionToken::open(sealed.as_ref(), &cipher(0), Duration::from_secs(10));
        assert_eq!(result.unwrap_err(), OpenSessionTokenError::Expired);
    }

    #[test]
    fn tampered() {
        let mut sealed = token().seal(&cipher(0)).unwrap().as_ref().to_vec();
        sealed[NONCE_LEN] ^= 1;

        let result = SessionToken::open(&sealed, &cipher(0), Duration::from_secs(60));
        assert_eq!(result.unwrap_err(), OpenSessionTokenError::Invalid);
    }

    #[test]
    fn wrong_key() {
        let sealed = token().seal(&cipher(0)).unwrap();

        let result = SessionToken::open(sealed.as_ref(), &cipher(1), Duration::from_secs(60));
        assert_eq!(result.unwrap_err(), OpenSessionTokenError::Invalid);
    }

    #[test]
    fn legacy_session_id_is_not_a_token() {
        let result = SessionToken::open(&[0; 32], &cipher(0), Duration::from_secs(60));
        assert_eq!(result.unwrap_err(), OpenSessionTokenError::Invalid);
    }
}
