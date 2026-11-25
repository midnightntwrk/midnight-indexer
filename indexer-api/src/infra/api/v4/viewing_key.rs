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

use async_graphql::{InputValueError, InputValueResult, Scalar, ScalarType, Value};
use derive_more::{Debug, Display, derive::From};
use fastrace::trace;
use indexer_common::domain::{NetworkId, ledger};
use secrecy::{ExposeSecret, SecretString, zeroize::Zeroizing};
use thiserror::Error;

use crate::infra::api::v4::{AddressType, DecodeAddressError, decode_address};

/// Bech32m-encoded viewing key.
///
/// `Debug` and `Display` are redacted, because the viewing key is a secret. Held in a
/// [SecretString], which zeroizes the string on drop.
#[derive(Debug, Display, Clone, From)]
#[from(&str)]
#[debug("ViewingKey(<redacted>)")]
#[display("<redacted>")]
pub struct ViewingKey(SecretString);

// Implemented by hand rather than with `scalar!`, which would require `Serialize` and thereby a
// way to serialize the secret. The viewing key is input-only, so `to_value` is never used.
#[Scalar]
impl ScalarType for ViewingKey {
    // Only a non-string value is echoed in the error message, never a viewing key. Validation of
    // the string must not put the value into its error message either.
    fn parse(value: Value) -> InputValueResult<Self> {
        match value {
            Value::String(viewing_key) => Ok(Self(viewing_key.into())),
            other => Err(InputValueError::expected_type(other)),
        }
    }

    fn to_value(&self) -> Value {
        debug_assert!(
            false,
            "ViewingKey is input-only and never converted to a value"
        );
        Value::String("<redacted>".to_owned())
    }
}

impl ViewingKey {
    /// Converts this API viewing key into a domain viewing key, validating the bech32m format and
    /// network ID and deserializing the bech32m data.
    ///
    /// Format expectations:
    /// - For mainnet: "mn_shield-esk" + bech32m data
    /// - For other networks: "mn_shield-esk_" + network-id + bech32m data where network-id can be
    ///   any non-empty alphanumeric string (e.g., "undeployed", "preview", "qanet", "preprod")
    #[trace(properties = { "network_id": "{network_id}" })]
    pub fn try_into_domain(
        self,
        network_id: &NetworkId,
    ) -> Result<indexer_common::domain::ViewingKey, ViewingKeyFormatError> {
        let bytes = Zeroizing::new(Vec::from(decode_address(
            self.0.expose_secret(),
            AddressType::SecretEncryptionKey,
            network_id,
        )?));
        let viewing_key = ledger::SecretKey::deserialize(bytes.as_slice())?.into();

        Ok(viewing_key)
    }
}

#[derive(Debug, Error)]
pub enum ViewingKeyFormatError {
    #[error("cannot bech32m-decode viewing key")]
    Decode(#[from] DecodeAddressError),

    #[error(transparent)]
    Ledger(#[from] ledger::Error),
}

#[cfg(test)]
mod tests {
    use crate::infra::api::v4::viewing_key::ViewingKey;
    use async_graphql::{ScalarType, Value};
    use secrecy::ExposeSecret;

    #[test]
    fn test_try_into_domain() {
        let viewing_key = ViewingKey::from(
            "mn_shield-esk_undeployed1dlyj7u8juj68fd4psnkqhjxh32sec0q480vzswg8kd485e2kljcs9ete5h",
        );
        let domain_viewing_key = viewing_key.try_into_domain(&"undeployed".try_into().unwrap());
        println!("{domain_viewing_key:?}");
        assert!(domain_viewing_key.is_ok());
    }

    #[test]
    fn test_debug_and_display_redacted() {
        let secret =
            "mn_shield-esk_undeployed1dlyj7u8juj68fd4psnkqhjxh32sec0q480vzswg8kd485e2kljcs9ete5h";
        let viewing_key = ViewingKey::from(secret);

        let debug = format!("{viewing_key:?}");
        assert!(!debug.contains(secret), "secret in Debug: {debug}");

        let display = viewing_key.to_string();
        assert!(!display.contains(secret), "secret in Display: {display}");
    }

    #[test]
    fn test_scalar() {
        let secret = "mn_shield-esk_undeployed1dlyj7u8";

        let viewing_key = ViewingKey::parse(Value::String(secret.to_owned()))
            .expect("string can be parsed as ViewingKey");
        assert_eq!(viewing_key.0.expose_secret(), secret);
        assert!(ViewingKey::parse(Value::Number(42.into())).is_err());
    }

    #[test]
    #[cfg(not(debug_assertions))]
    fn test_to_value_redacted() {
        let secret = "mn_shield-esk_undeployed1dlyj7u8";
        let value = ViewingKey::from(secret).to_value().to_string();
        assert!(!value.contains(secret), "secret in value: {value}");
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "input-only")]
    fn test_to_value_unused() {
        let _ = ViewingKey::from("mn_shield-esk_undeployed1dlyj7u8").to_value();
    }
}
