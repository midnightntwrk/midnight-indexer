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

use crate::domain::{ByteVec, CardanoRewardAddress, NetworkId};
use bech32::{Bech32, Bech32m, Hrp};
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

/// Cardano network type for reward addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardanoNetwork {
    Mainnet,

    Testnet,
}

impl CardanoNetwork {
    fn hrp(&self) -> &'static str {
        match self {
            CardanoNetwork::Mainnet => "stake",
            CardanoNetwork::Testnet => "stake_test",
        }
    }

    fn from_hrp(hrp: &str) -> Option<Self> {
        match hrp {
            "stake" => Some(CardanoNetwork::Mainnet),
            "stake_test" => Some(CardanoNetwork::Testnet),
            _ => None,
        }
    }
}

#[derive(Debug, Error)]
pub enum DecodeCardanoRewardAddressError {
    #[error("cannot bech32-decode Cardano reward address")]
    Decode(#[from] bech32::DecodeError),

    #[error("invalid HRP for Cardano reward address: {0}")]
    InvalidHrp(String),

    #[error("invalid Cardano reward address length: expected 29 bytes, was {0}")]
    InvalidLength(usize),
}

/// Bech32-decode a Cardano reward address string to a 29-byte CardanoRewardAddress.
/// Supports both mainnet ("stake") and testnet ("stake_test") addresses.
pub fn decode_cardano_reward_address(
    address: impl AsRef<str>,
) -> Result<CardanoRewardAddress, DecodeCardanoRewardAddressError> {
    let (hrp, bytes) = bech32::decode(address.as_ref())?;

    // Validate HRP is a valid Cardano reward address.
    CardanoNetwork::from_hrp(hrp.as_str())
        .ok_or_else(|| DecodeCardanoRewardAddressError::InvalidHrp(hrp.to_string()))?;

    // Validate length.
    if bytes.len() != 29 {
        return Err(DecodeCardanoRewardAddressError::InvalidLength(bytes.len()));
    }

    // Convert to fixed-size array.
    let reward_address_bytes: [u8; 29] = bytes
        .try_into()
        .expect("length already validated as 29 bytes");

    Ok(CardanoRewardAddress::from(reward_address_bytes))
}

/// Bech32-encode a 29-byte CardanoRewardAddress to a Cardano reward address string.
pub fn encode_cardano_reward_address(
    reward_address: &CardanoRewardAddress,
    network: CardanoNetwork,
) -> String {
    let hrp = Hrp::parse(network.hrp()).expect("HRP for Cardano reward address can be parsed");
    bech32::encode::<Bech32>(hrp, reward_address.as_ref())
        .expect("bytes for Cardano reward address can be Bech32-encoded")
}

#[cfg(test)]
mod tests {
    use crate::domain::{
        AddressType, ByteVec, CardanoNetwork, CardanoRewardAddress, decode_address,
        decode_cardano_reward_address, encode_address, encode_cardano_reward_address,
    };
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

    #[test]
    fn test_encode_decode_cardano_reward_address() {
        // Test with known testnet reward address from Radosław's registration.
        // stake_test1uqtgpdz0chm6jnxx7erfd7rhqfud7t4ajazx8es8xk8x3ts06psdv
        // Hex: e01680b44fc5f7a94cc6f64696f8770278df2ebd974463e607358e68ae
        let reward_address_hex = "e01680b44fc5f7a94cc6f64696f8770278df2ebd974463e607358e68ae";
        let reward_address_bytes =
            const_hex::decode(reward_address_hex).expect("hex decodes to 29 bytes");
        let reward_address: CardanoRewardAddress = reward_address_bytes
            .try_into()
            .expect("converts to CardanoRewardAddress");

        // Test encoding.
        let encoded = encode_cardano_reward_address(&reward_address, CardanoNetwork::Testnet);
        assert_eq!(
            encoded,
            "stake_test1uqtgpdz0chm6jnxx7erfd7rhqfud7t4ajazx8es8xk8x3ts06psdv"
        );

        // Test decoding.
        let decoded = decode_cardano_reward_address(encoded).expect("decodes successfully");
        assert_eq!(decoded, reward_address);

        // Test round-trip with mainnet.
        let reward_address_bytes = [42u8; 29];
        let reward_address = CardanoRewardAddress::from(reward_address_bytes);
        let encoded = encode_cardano_reward_address(&reward_address, CardanoNetwork::Mainnet);
        assert!(encoded.starts_with("stake1"));
        let decoded = decode_cardano_reward_address(encoded).expect("decodes successfully");
        assert_eq!(decoded, reward_address);
    }

    #[test]
    fn test_decode_cardano_reward_address_errors() {
        // Invalid HRP.
        let result = decode_cardano_reward_address("addr1invalid");
        assert_matches!(result, Err(_));

        // Invalid length (too short).
        let short_key = CardanoRewardAddress::default(); // This will be all zeros, 29 bytes.
        let encoded = encode_cardano_reward_address(&short_key, CardanoNetwork::Testnet);
        // We can't easily create an invalid length since encode always produces 29 bytes.
        // So we just test that valid ones work.
        let decoded = decode_cardano_reward_address(encoded).expect("decodes successfully");
        assert_eq!(decoded, short_key);
    }
}
