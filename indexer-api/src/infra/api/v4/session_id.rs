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

use crate::{domain::SESSION_TOKEN_LEN, infra::api::v4::HexDecodeError};
use async_graphql::scalar;
use derive_more::{Debug, Display, derive::From};
use indexer_common::domain;
use secrecy::{ExposeSecret, SecretSlice, SecretString};
use serde::{Deserialize, Serialize, Serializer};
use thiserror::Error;

/// Hex-encoded session ID. Exposed as the `HexEncoded` scalar, so the GraphQL schema is the same
/// as for other hex-encoded values.
///
/// `Debug` and `Display` are redacted, because the session ID is a secret. Held in a
/// [SecretString], which zeroizes the string on drop.
#[derive(Debug, Display, Clone, Deserialize, From)]
#[from(&str)]
#[debug("SessionId(<redacted>)")]
#[display("<redacted>")]
pub struct SessionId(SecretString);

// Serializing exposes the secret; the GraphQL scalar needs it to output the value.
impl Serialize for SessionId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.expose_secret().serialize(serializer)
    }
}

// Parsing only fails for non-string values, so a parse error never echoes a session ID. A custom
// parse that validates the string must not put the value into its error message.
//
// Registers the same GraphQL scalar as `HexEncoded`. async-graphql keeps whichever type registers
// first and does not check that the two agree, so keep both definitions identical: no
// description and no validation beyond `HexEncoded`'s.
scalar!(SessionId, "HexEncoded");

impl From<domain::SessionId> for SessionId {
    fn from(session_id: domain::SessionId) -> Self {
        Self(const_hex::encode(session_id.expose_secret()).into())
    }
}

impl From<SecretSlice<u8>> for SessionId {
    fn from(session: SecretSlice<u8>) -> Self {
        Self(const_hex::encode(session.expose_secret()).into())
    }
}

impl SessionId {
    /// Decode a sealed token or legacy session ID while keeping the bytes secret.
    pub fn into_bytes(self) -> Result<SecretSlice<u8>, SessionIdFormatError> {
        let bytes = SecretSlice::from(
            const_hex::decode(self.0.expose_secret()).map_err(HexDecodeError::from)?,
        );
        if !matches!(bytes.expose_secret().len(), 32 | SESSION_TOKEN_LEN) {
            return Err(SessionIdFormatError::InvalidLength);
        }
        Ok(bytes)
    }
}

#[derive(Debug, Error)]
pub enum SessionIdFormatError {
    #[error("invalid session ID length")]
    InvalidLength,

    #[error("cannot hex-decode session ID")]
    HexDecode(#[from] HexDecodeError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_and_legacy_session_roundtrip_without_debug_exposure() {
        for len in [32, SESSION_TOKEN_LEN] {
            let bytes = SecretSlice::from(vec![7; len]);
            let session = SessionId::from(bytes);
            let hex = "07".repeat(len);
            assert!(!format!("{session:?}").contains(&hex));
            assert!(!session.to_string().contains(&hex));
            assert_eq!(
                serde_json::to_string(&session).unwrap(),
                format!("\"{hex}\"")
            );
            assert_eq!(session.into_bytes().unwrap().expose_secret(), vec![7; len]);
        }
        assert!(SessionId::from("07").into_bytes().is_err());
    }

    #[test]
    fn test_debug_and_display_redacted() {
        let secret = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let session_id = SessionId::from(secret);

        let debug = format!("{session_id:?}");
        assert!(!debug.contains(secret), "secret in Debug: {debug}");

        let display = session_id.to_string();
        assert!(!display.contains(secret), "secret in Display: {display}");
    }

    #[test]
    fn test_serialize() {
        let session_id = SessionId::from("0123456789abcdef");
        let json = serde_json::to_string(&session_id).expect("SessionId can be serialized");
        assert_eq!(json, r#""0123456789abcdef""#);
    }
}
