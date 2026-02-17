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

use derive_more::Display;
use parity_scale_codec::Decode;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProtocolVersion {
    V0_20(u32),
    V0_21(u32),
    V0_22(u32),
}

impl ProtocolVersion {
    pub fn ledger_version(self) -> LedgerVersion {
        match self {
            ProtocolVersion::V0_20(_) => LedgerVersion::V7,
            ProtocolVersion::V0_21(_) => LedgerVersion::V7,
            ProtocolVersion::V0_22(_) => LedgerVersion::V8,
        }
    }

    pub fn node_version(self) -> NodeVersion {
        match self {
            ProtocolVersion::V0_20(_) => NodeVersion::V0_20,
            ProtocolVersion::V0_21(_) => NodeVersion::V0_21,
            ProtocolVersion::V0_22(_) => NodeVersion::V0_22,
        }
    }

    pub fn into_i64(self) -> i64 {
        u32::from(self) as i64
    }
}

impl From<ProtocolVersion> for u32 {
    fn from(version: ProtocolVersion) -> Self {
        match version {
            ProtocolVersion::V0_20(n) => n,
            ProtocolVersion::V0_21(n) => n,
            ProtocolVersion::V0_22(n) => n,
        }
    }
}

impl TryFrom<&[u8]> for ProtocolVersion {
    type Error = ProtocolVersionError;

    fn try_from(mut bytes: &[u8]) -> Result<Self, Self::Error> {
        let version = u32::decode(&mut bytes)?;
        version.try_into()
    }
}

impl TryFrom<u32> for ProtocolVersion {
    type Error = ProtocolVersionError;

    fn try_from(version: u32) -> Result<Self, Self::Error> {
        if (0_020_000..0_021_000).contains(&version) {
            Ok(Self::V0_20(version))
        } else if (0_021_000..0_022_000).contains(&version) {
            Ok(Self::V0_21(version))
        } else if (0_022_000..0_023_000).contains(&version) {
            Ok(Self::V0_22(version))
        } else {
            Err(ProtocolVersionError::Unsupported(version))
        }
    }
}

impl TryFrom<i64> for ProtocolVersion {
    type Error = ProtocolVersionError;

    fn try_from(version: i64) -> Result<Self, Self::Error> {
        (version as u32).try_into()
    }
}

#[derive(Debug, Error)]
pub enum ProtocolVersionError {
    #[error("cannot SCALE decode protocol version")]
    ScaleDecode(#[from] parity_scale_codec::Error),

    #[error("unsupported protocol version {0}")]
    Unsupported(u32),
}

// /// The runtime specification version of the chain.
// #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, From)]
// pub struct ProtocolVersion(pub u32);

// impl ProtocolVersion {
//     /// The major version, i.e. `1` in `1.2.3`.
//     pub fn major(self) -> u32 {
//         self.0 / 1_000_000
//     }

//     /// The minor version, i.e. `2` in `1.2.3`.
//     pub fn minor(self) -> u32 {
//         self.0 / 1_000 % 1_000
//     }

//     /// The patch version, i.e. `3` in `1.2.3`.
//     pub fn patch(self) -> u32 {
//         self.0 % 1_000
//     }

//     pub fn ledger_version(self) -> Result<LedgerVersion, UnsupportedProtocolVersion> {
//         if self.is_compatible(0_020_000, 0_022_000) {
//             Ok(LedgerVersion::V7)
//         } else if self.is_compatible(0_022_000, 0_023_000) {
//             Ok(LedgerVersion::V8)
//         } else {
//             Err(UnsupportedProtocolVersion(self))
//         }
//     }

//     pub fn node_version(self) -> Result<NodeVersion, UnsupportedProtocolVersion> {
//         if self.is_compatible(0_020_000, 0_021_000) {
//             Ok(NodeVersion::V0_20)
//         } else if self.is_compatible(0_021_000, 0_022_000) {
//             Ok(NodeVersion::V0_21)
//         } else if self.is_compatible(0_022_000, 0_023_000) {
//             Ok(NodeVersion::V0_22)
//         } else {
//             Err(UnsupportedProtocolVersion(self))
//         }
//     }

//     fn is_compatible(self, from: u32, to: u32) -> bool {
//         from <= self.0 && self.0 < to
//     }
// }

// impl TryFrom<&[u8]> for ProtocolVersion {
//     type Error = ScaleDecodeProtocolVersionError;

//     /// Used to SCALE decode the `ProtocolVersion` from a block header from the node.
//     fn try_from(mut value: &[u8]) -> Result<Self, Self::Error> {
//         let value = u32::decode(&mut value)?;
//         Ok(Self(value))
//     }
// }

// impl TryFrom<i64> for ProtocolVersion {
//     type Error = TryFromIntError;

//     fn try_from(value: i64) -> Result<Self, Self::Error> {
//         let value = u32::try_from(value)?;
//         Ok(Self(value))
//     }
// }

// impl Display for ProtocolVersion {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         let major = self.major();
//         let minor = self.minor();
//         let patch = self.patch();
//         write!(f, "{major}.{minor}.{patch}")
//     }
// }

// #[derive(Debug, Error)]
// #[error("unsupported protocol version {0}")]
// pub struct UnsupportedProtocolVersion(ProtocolVersion);

// /// Error possibly returned by `ProtocolVersion::try_from<&[u8]>`.
// #[derive(Debug, Error)]
// #[error("cannot SCALE decode protocol version")]
// pub struct ScaleDecodeProtocolVersionError(#[from] parity_scale_codec::Error);

#[derive(Debug, Display, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LedgerVersion {
    V7,
    V8,
}

impl LedgerVersion {
    pub const OLDEST: Self = Self::V7;

    pub const LATEST: Self = Self::V8;
}

#[derive(Debug, Display, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NodeVersion {
    V0_20,
    V0_21,
    V0_22,
}

#[cfg(test)]
mod tests {
    use crate::domain::{LedgerVersion, NodeVersion, ProtocolVersion, ProtocolVersionError};
    use assert_matches::assert_matches;

    #[test]
    fn test_protocol_version() {
        let version = ProtocolVersion::try_from(0_019_000_u32);
        assert_matches!(version, Err(ProtocolVersionError::Unsupported(v)) if v == 0_019_000);

        let version = ProtocolVersion::try_from(0_023_000_u32);
        assert_matches!(version, Err(ProtocolVersionError::Unsupported(v)) if v == 0_023_000);

        let version =
            ProtocolVersion::try_from(0_020_000_u32).expect("0_020_000 is valid protocol version");
        assert_eq!(version.ledger_version(), LedgerVersion::V7);
        assert_eq!(version.node_version(), NodeVersion::V0_20);

        let version =
            ProtocolVersion::try_from(0_021_001_u32).expect("0_021_001 is valid protocol version");
        assert_eq!(version.ledger_version(), LedgerVersion::V7);
        assert_eq!(version.node_version(), NodeVersion::V0_21);

        let version =
            ProtocolVersion::try_from(0_022_666_u32).expect("0_022_666 is valid protocol version");
        assert_eq!(version.ledger_version(), LedgerVersion::V8);
        assert_eq!(version.node_version(), NodeVersion::V0_22);
    }
}
