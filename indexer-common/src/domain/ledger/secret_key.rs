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

use crate::domain::{ViewingKey, ledger::Error};
use fastrace::trace;
use midnight_serialize_v1::Deserializable;
use secrecy::{ExposeSecret, SecretBox, zeroize::Zeroizing};

/// Held in a [SecretBox], which zeroizes the key on drop.
#[derive(Debug)]
pub struct SecretKey(SecretBox<midnight_transient_crypto_v2::encryption::SecretKey>);

impl SecretKey {
    #[trace]
    pub fn deserialize(secret_key: impl AsRef<[u8]>) -> Result<Self, Error> {
        let inner = midnight_transient_crypto_v2::encryption::SecretKey::deserialize(
            &mut secret_key.as_ref(),
            0,
        )
        .map_err(|error| Error::Deserialize("SecretKey", error))?;
        Ok(Self(SecretBox::new(Box::new(inner))))
    }
}

impl From<SecretKey> for ViewingKey {
    /// Writes the key into the viewing key's [SecretBox] in place.
    fn from(secret_key: SecretKey) -> Self {
        let repr = Zeroizing::new(secret_key.0.expose_secret().repr());
        Self::init_with_mut(|viewing_key| viewing_key.copy_from_slice(repr.as_slice()))
    }
}
