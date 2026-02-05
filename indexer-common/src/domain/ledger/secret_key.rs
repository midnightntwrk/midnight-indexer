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

use crate::domain::{ByteArray, LedgerVersion, ProtocolVersion, VIEWING_KEY_LEN, ledger::Error};
use fastrace::trace;
use midnight_serialize_v7_0_0::Deserializable as DeserializableV7_0_0;
use midnight_transient_crypto_v7_0_0::encryption::SecretKey as SecretKeyV7_0_0;

/// Facade for `SecretKey` from `midnight_ledger` across supported (protocol) versions.
#[derive(Debug, Clone)]
pub enum SecretKey {
    V7_0_0(SecretKeyV7_0_0),
}

impl SecretKey {
    /// Untagged deserialize the given serialized secret key using the given protocol version.
    #[trace(properties = { "protocol_version": "{protocol_version}" })]
    pub fn deserialize(
        secret_key: impl AsRef<[u8]>,
        protocol_version: ProtocolVersion,
    ) -> Result<Self, Error> {
        let key = match protocol_version.ledger_version()? {
            LedgerVersion::V7 => {
                let secret_key = SecretKeyV7_0_0::deserialize(&mut secret_key.as_ref(), 0)
                    .map_err(|error| Error::Deserialize("SecretKeyV7_0_0", error))?;
                Self::V7_0_0(secret_key)
            }
        };

        Ok(key)
    }

    /// Get the repr of this secret key.
    pub fn expose_secret(&self) -> ByteArray<VIEWING_KEY_LEN> {
        match self {
            Self::V7_0_0(secret_key) => secret_key.repr().into(),
        }
    }
}
