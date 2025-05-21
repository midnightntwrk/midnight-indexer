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

use crate::domain::NetworkId;
use bech32::{Bech32m, Hrp, decode, encode};
use thiserror::Error;

/// Human-readable part base prefix for unshielded addresses.
pub const HRP_UNSHIELDED_BASE: &str = "mn_addr";

/// Errors returned by [`to_bech32m`] and [`from_bech32m`].
#[derive(Debug, Error)]
pub enum UnshieldedAddressError {
    #[error("cannot bech32m-decode unshielded address")]
    Decode(#[from] bech32::DecodeError),

    #[error("cannot bech32m-encode unshielded address")]
    Encode(#[from] bech32::EncodeError),

    #[error("invalid HRP {0}")]
    InvalidHrp(String),
}

/// Encode raw bytes into a Bech32m address.
pub fn to_bech32m(bytes: &[u8], network: NetworkId) -> Result<String, UnshieldedAddressError> {
    // Build HRP following wallet spec pattern
    let hrp_str = match network {
        NetworkId::MainNet => HRP_UNSHIELDED_BASE.to_string(),
        NetworkId::DevNet => format!("{}_dev", HRP_UNSHIELDED_BASE),
        NetworkId::TestNet => format!("{}_test", HRP_UNSHIELDED_BASE),
        NetworkId::Undeployed => format!("{}_undeployed", HRP_UNSHIELDED_BASE),
    };

    let hrp =
        Hrp::parse(&hrp_str).map_err(|_| UnshieldedAddressError::InvalidHrp(hrp_str.clone()))?;
    encode::<Bech32m>(hrp, bytes).map_err(Into::into)
}

/// Decode a Bech32m string back to the raw bytes stored in DB.
pub fn from_bech32m(s: &str) -> Result<Vec<u8>, UnshieldedAddressError> {
    let (hrp, data) = decode(s)?;
    let hrp_str = hrp.to_lowercase();

    if !hrp_str.starts_with(HRP_UNSHIELDED_BASE) {
        return Err(UnshieldedAddressError::InvalidHrp(hrp_str));
    }

    Ok(data)
}

#[cfg(test)]
mod tests {
    use crate::domain::{
        NetworkId::TestNet,
        unshielded::{from_bech32m, to_bech32m},
    };

    #[test]
    fn roundtrip() {
        let bytes = vec![0u8; 32];
        let addr = to_bech32m(&bytes, TestNet).unwrap();
        let decoded = from_bech32m(&addr).unwrap();
        assert_eq!(decoded, bytes);
    }
}
