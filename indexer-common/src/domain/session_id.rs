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

use crate::domain::ByteArrayLenError;
use derive_more::{Debug, Display};
use secrecy::{ExposeSecret, SecretBox};

const SESSION_ID_LEN: usize = 32;

/// A session ID.
///
/// Attention: Do not accidentally leak the secret!
///
/// Held in a [SecretBox], which zeroizes the bytes on drop.
#[derive(Debug, Display)]
#[debug("SessionId(REDACTED)")]
#[display("REDACTED")]
pub struct SessionId(SecretBox<[u8; SESSION_ID_LEN]>);

impl SessionId {
    /// Create a session ID by writing its bytes in place, so they are never copied.
    pub fn init_with_mut(ctr: impl FnOnce(&mut [u8; SESSION_ID_LEN])) -> Self {
        Self(SecretBox::init_with_mut(ctr))
    }

    /// Expose the secret.
    pub fn expose_secret(&self) -> &[u8; SESSION_ID_LEN] {
        self.0.expose_secret()
    }
}

impl From<[u8; SESSION_ID_LEN]> for SessionId {
    fn from(bytes: [u8; SESSION_ID_LEN]) -> Self {
        Self(SecretBox::new(Box::new(bytes)))
    }
}

impl TryFrom<&[u8]> for SessionId {
    type Error = ByteArrayLenError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        (bytes.len() == SESSION_ID_LEN)
            .then(|| Self::init_with_mut(|session_id| session_id.copy_from_slice(bytes)))
            .ok_or(ByteArrayLenError(SESSION_ID_LEN, bytes.len()))
    }
}

impl Clone for SessionId {
    fn clone(&self) -> Self {
        Self::init_with_mut(|session_id| session_id.copy_from_slice(self.expose_secret()))
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::SessionId;

    #[test]
    fn test_debug_and_display_redacted() {
        let session_id = SessionId::from([0xab; 32]);

        let debug = format!("{session_id:?}");
        assert!(!debug.contains("abab"), "secret in Debug: {debug}");

        let display = session_id.to_string();
        assert!(!display.contains("abab"), "secret in Display: {display}");
    }

    #[test]
    fn test_clone() {
        let session_id = SessionId::from([1; 32]);
        let clone = session_id.clone();
        assert_eq!(clone.expose_secret(), session_id.expose_secret());
    }
}
