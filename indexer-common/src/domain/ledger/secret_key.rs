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

use crate::domain::{
    ByteArray, ByteVec, PROTOCOL_VERSION_000_013_000, ProtocolVersion, VIEWING_KEY_LEN,
    ledger::{Error, TaggedSerializableV5Ext},
};
use bip32::{DerivationPath, XPrv};
use fastrace::trace;
use midnight_serialize::tagged_deserialize;
use midnight_transient_crypto::encryption::SecretKey as SecretKeyV5;
use midnight_zswap::keys::{SecretKeys as SecretKeysV5, Seed as SeedV5};
use std::str::FromStr;

/// Facade for `SecretKey` from `midnight_ledger` across supported (protocol) versions.
#[derive(Debug, Clone)]
pub enum SecretKey {
    V5(SecretKeyV5),
}

impl SecretKey {
    /// Deserialize the given raw secret key using the given protocol version.
    #[trace(properties = { "protocol_version": "{protocol_version}" })]
    pub fn deserialize(
        secret_key: impl AsRef<[u8]>,
        protocol_version: ProtocolVersion,
    ) -> Result<Self, Error> {
        if protocol_version.is_compatible(PROTOCOL_VERSION_000_013_000) {
            let secret_key = tagged_deserialize(&mut secret_key.as_ref())
                .map_err(|error| Error::Io("cannot deserialize SecretKeyV5", error))?;
            Ok(Self::V5(secret_key))
        } else {
            Err(Error::InvalidProtocolVersion(protocol_version))
        }
    }

    /// Get the repr of this secret key.
    pub fn expose_secret(&self) -> [u8; VIEWING_KEY_LEN] {
        match self {
            SecretKey::V5(secret_key) => secret_key.repr(),
        }
    }

    /// Derive a serialized secret key for testing from the given root seed.
    pub fn derive_for_testing(
        seed: ByteArray<32>,
        protocol_version: ProtocolVersion,
    ) -> Result<ByteVec, Error> {
        if protocol_version.is_compatible(PROTOCOL_VERSION_000_013_000) {
            let derivation_path = DerivationPath::from_str("m/44'/2400'/0'/3/0")
                .expect(r#"derivation path "m/44'/2400'/0'/3/0" is valid"#);
            let derived: [u8; 32] = XPrv::derive_from_path(seed, &derivation_path)
                .expect("key can be derived")
                .private_key()
                .to_bytes()
                .into();

            let secret_keys = SecretKeysV5::from(SeedV5::from(derived));

            let bytes = secret_keys
                .encryption_secret_key
                .tagged_serialize()
                .map_err(|error| Error::Io("cannot serialize encryption::SecretKeyV5", error))?;

            Ok(bytes.into())
        } else {
            Err(Error::InvalidProtocolVersion(protocol_version))
        }
    }
}
