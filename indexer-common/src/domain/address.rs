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

use crate::domain::{ByteVec, NetworkId};
use bech32::{Bech32m, Hrp};
use thiserror::Error;

/// A Midnight address type.
pub enum AddressType {
    Unshielded,
    SecretEncryptionKey,
}

impl AddressType {
    fn hrp(&self, network_id: &NetworkId) -> String {
        let prefix = self.hrp_prefix();

        if network_id.eq_ignore_ascii_case("mainnet") {
            prefix.to_string()
        } else {
            format!("{prefix}_{network_id}")
        }
    }

    fn hrp_prefix(&self) -> &'static str {
        match self {
            AddressType::Unshielded => "mn_addr",
            AddressType::SecretEncryptionKey => "mn_shield-esk",
        }
    }
}

#[derive(Debug, Error)]
pub enum DecodeAddressError {
    #[error("cannot bech32m-decode address")]
    Decode(#[from] bech32::DecodeError),

    #[error("expected HRP {expected_hrp}, but was {hrp}")]
    InvalidHrp { expected_hrp: String, hrp: String },
}

#[derive(Debug, Error)]
pub enum EncodeAddressError {
    #[error("cannot bech32m-encode address")]
    Encode(#[from] bech32::EncodeError),

    #[error("expected HRP {expected_hrp}, but was {hrp}")]
    InvalidHrp { expected_hrp: String, hrp: String },
}

/// Bech32m-decode the given address string as a byte vector, thereby validate the given address
/// type and the given network ID.
pub fn decode_address(
    address: impl AsRef<str>,
    address_type: AddressType,
    network_id: &NetworkId,
) -> Result<ByteVec, DecodeAddressError> {
    let (hrp, bytes) = bech32::decode(address.as_ref())?;

    let expected_hrp = address_type.hrp(network_id);
    if hrp.as_str() != expected_hrp {
        let hrp = hrp.to_string();
        return Err(DecodeAddressError::InvalidHrp { expected_hrp, hrp });
    }

    Ok(bytes.into())
}

/// Bech32m-encode the given address bytes as a string for the given address type and network ID.
pub fn encode_address(
    address: impl AsRef<[u8]>,
    address_type: AddressType,
    network_id: &NetworkId,
) -> String {
    let hrp = Hrp::parse(&address_type.hrp(network_id)).expect("HRP for address can be parsed");
    bech32::encode::<Bech32m>(hrp, address.as_ref())
        .expect("bytes for unshielded address can be Bech32m-encoded")
}

#[cfg(test)]
mod tests {
    use crate::domain::{AddressType, ByteVec, decode_address, encode_address};
    use assert_matches::assert_matches;

    #[test]
    fn test_encode_address() {
        let address = ByteVec::from(vec![0, 1, 2, 3]);

        let encoded = encode_address(
            &address,
            AddressType::SecretEncryptionKey,
            &"undeployed".try_into().unwrap(),
        );
        assert!(encoded.starts_with("mn_shield-esk_undeployed1"));

        let encoded = encode_address(
            &address,
            AddressType::Unshielded,
            &"mainnet".try_into().unwrap(),
        );
        assert!(encoded.starts_with("mn_addr1"));
    }

    #[test]
    fn test_encode_decode_address() {
        let address = ByteVec::from(vec![0, 1, 2, 3]);
        let encoded = encode_address(
            &address,
            AddressType::SecretEncryptionKey,
            &"undeployed".try_into().unwrap(),
        );
        let decoded = decode_address(
            encoded,
            AddressType::SecretEncryptionKey,
            &"undeployed".try_into().unwrap(),
        );
        assert_matches!(decoded, Ok(a) if a == address);

        let address = ByteVec::from(vec![0, 1, 2, 3]);
        let encoded = encode_address(
            &address,
            AddressType::Unshielded,
            &"mainnet".try_into().unwrap(),
        );
        let decoded = decode_address(
            encoded,
            AddressType::Unshielded,
            &"mainnet".try_into().unwrap(),
        );
        assert_matches!(decoded, Ok(a) if a == address);
    }
}
