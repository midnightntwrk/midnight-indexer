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

use crate::infra::api::v4::HexDecodeError;
use async_graphql::scalar;
use derive_more::{Debug, Display, derive::From};
use indexer_common::domain::{self, ByteArrayLenError};
use secrecy::{ExposeSecret, SecretString, zeroize::Zeroizing};
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

impl TryFrom<SessionId> for domain::SessionId {
    type Error = SessionIdFormatError;

    /// Hex-decodes the given API session ID into a domain session ID.
    fn try_from(session_id: SessionId) -> Result<Self, Self::Error> {
        let bytes = Zeroizing::new(
            const_hex::decode(session_id.0.expose_secret()).map_err(HexDecodeError::from)?,
        );
        let session_id = Self::try_from(bytes.as_slice())?;
        Ok(session_id)
    }
}

#[derive(Debug, Error)]
pub enum SessionIdFormatError {
    #[error("cannot hex-decode session ID")]
    HexDecode(#[from] HexDecodeError),

    #[error("cannot convert into session ID")]
    ByteArrayLen(#[from] ByteArrayLenError),
}

#[cfg(test)]
mod tests {
    use crate::infra::api::v4::session_id::SessionId;

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
