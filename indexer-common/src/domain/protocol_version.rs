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

use std::num::TryFromIntError;

use derive_more::Display;
use parity_scale_codec::Decode;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProtocolVersion {
    V0_22(u32),
    V1_0(u32),
    V2_0(u32),
    V2_1(u32),
    V3_0(u32),
}

impl ProtocolVersion {
    pub fn ledger_version(self) -> LedgerVersion {
        match self {
            ProtocolVersion::V0_22(_) => LedgerVersion::V8,
            ProtocolVersion::V1_0(_) => LedgerVersion::V8,
            ProtocolVersion::V2_0(_) => LedgerVersion::V9,
            ProtocolVersion::V2_1(_) => LedgerVersion::V9,
            ProtocolVersion::V3_0(_) => LedgerVersion::V9,
        }
    }

    pub fn node_version(self) -> NodeVersion {
        match self {
            ProtocolVersion::V0_22(_) => NodeVersion::V0_22,
            ProtocolVersion::V1_0(_) => NodeVersion::V1_0,
            ProtocolVersion::V2_0(_) => NodeVersion::V2_0,
            ProtocolVersion::V2_1(_) => NodeVersion::V2_1,
            ProtocolVersion::V3_0(_) => NodeVersion::V3_0,
        }
    }

    pub fn into_i64(self) -> i64 {
        u32::from(self) as i64
    }
}

impl From<ProtocolVersion> for u32 {
    fn from(version: ProtocolVersion) -> Self {
        match version {
            ProtocolVersion::V0_22(n) => n,
            ProtocolVersion::V1_0(n) => n,
            ProtocolVersion::V2_0(n) => n,
            ProtocolVersion::V2_1(n) => n,
            ProtocolVersion::V3_0(n) => n,
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
        if (0_022_000..0_023_000).contains(&version) {
            Ok(Self::V0_22(version))
        } else if (1_000_000..1_001_000).contains(&version) {
            Ok(Self::V1_0(version))
        } else if (2_000_000..2_001_000).contains(&version) {
            Ok(Self::V2_0(version))
        } else if (2_001_000..2_002_000).contains(&version) {
            Ok(Self::V2_1(version))
        } else if (3_000_000..3_001_000).contains(&version) {
            Ok(Self::V3_0(version))
        } else {
            Err(ProtocolVersionError::Unsupported(version))
        }
    }
}

impl TryFrom<i64> for ProtocolVersion {
    type Error = ProtocolVersionError;

    fn try_from(version: i64) -> Result<Self, Self::Error> {
        u32::try_from(version)
            .map_err(|error| ProtocolVersionError::TryFromI64(version, error))?
            .try_into()
    }
}

#[derive(Debug, Error)]
pub enum ProtocolVersionError {
    #[error("cannot SCALE decode protocol version")]
    ScaleDecode(#[from] parity_scale_codec::Error),

    #[error("unsupported protocol version {0}")]
    Unsupported(u32),

    #[error("invalid i64 protocol version {0}")]
    TryFromI64(i64, #[source] TryFromIntError),
}

#[derive(Debug, Display, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LedgerVersion {
    V8,
    V9,
}

impl LedgerVersion {
    pub const OLDEST: Self = Self::V8;
    // Dust-query decode version. This build serves ledger-9 chains (devnet and
    // stagenet under the node 2.0 rollout). Deriving the version per chain
    // rather than from this constant is the tracked follow-up.
    pub const LATEST: Self = Self::V9;

    /// Which incarnation of the DUST generation tree this ledger version writes
    /// into.
    ///
    /// A hard fork whose state translation *wipes* dust state starts the tree
    /// over: `first_free` returns to zero, and generation/commitment tree
    /// indices are reused for entirely different leaves. Rows recorded before
    /// such a wipe are dead - the ledger no longer holds those entries - and
    /// their indices name leaves that no longer exist, so mixing epochs
    /// double-counts NIGHT balances and hands out Merkle indices into a tree
    /// that is gone. Every read of `dust_generation_info` therefore scopes to
    /// one epoch (see `indexer-api`'s dust storage).
    ///
    /// The mapping is deliberately explicit rather than derived from the
    /// version number: a future ledger major that does *not* wipe dust must
    /// keep the same epoch, or it would hide entries that are still live.
    ///
    /// - V8 -> 0
    /// - V9 -> 1, because the 8 -> 9 translation replaces dust state with
    ///   `DustState::default()` (midnight-node #2012, backported as #2057) and
    ///   the node then replays only cNIGHT's slice of the generating set.
    pub const fn dust_epoch(self) -> i64 {
        match self {
            Self::V8 => 0,
            Self::V9 => 1,
        }
    }
}

#[derive(Debug, Display, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NodeVersion {
    V0_22,
    V1_0,
    V2_0,
    V2_1,
    V3_0,
}

#[cfg(test)]
mod tests {
    use crate::domain::{LedgerVersion, NodeVersion, ProtocolVersion, ProtocolVersionError};
    use assert_matches::assert_matches;

    #[test]
    fn test_unsupported_protocol_version() {
        for version in [
            0_019_000_u32,
            0_021_000,
            0_023_000,
            1_001_000,
            2_002_000,
            3_001_000,
        ] {
            assert_matches!(
                ProtocolVersion::try_from(version),
                Err(ProtocolVersionError::Unsupported(v)) if v == version
            );
        }
    }

    #[test]
    fn test_protocol_version() {
        // Sweeping every minor version reaches every variant `try_from` can return, as each range
        // starts on a multiple of 1_000.
        let protocol_versions = (0..=u32::MAX)
            .step_by(1_000)
            .filter_map(|version| ProtocolVersion::try_from(version).ok());

        for protocol_version in protocol_versions {
            let version = u32::from(protocol_version);

            // Exhaustive, so that a new protocol version does not compile until it has a case.
            use ProtocolVersion::*;
            let (versions, ledger_version, node_version) = match protocol_version {
                V0_22(_) => (0_022_000..0_023_000, LedgerVersion::V8, NodeVersion::V0_22),
                V1_0(_) => (1_000_000..1_001_000, LedgerVersion::V8, NodeVersion::V1_0),
                V2_0(_) => (2_000_000..2_001_000, LedgerVersion::V9, NodeVersion::V2_0),
                V2_1(_) => (2_001_000..2_002_000, LedgerVersion::V9, NodeVersion::V2_1),
                V3_0(_) => (3_000_000..3_001_000, LedgerVersion::V9, NodeVersion::V3_0),
            };

            assert!(versions.contains(&version), "{version}");
            assert_eq!(
                protocol_version.ledger_version(),
                ledger_version,
                "{version}"
            );
            assert_eq!(protocol_version.node_version(), node_version, "{version}");
        }
    }
}
