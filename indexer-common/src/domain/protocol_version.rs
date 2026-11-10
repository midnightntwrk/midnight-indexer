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
}

impl ProtocolVersion {
    pub fn ledger_version(self) -> LedgerVersion {
        match self {
            ProtocolVersion::V0_22(_) => LedgerVersion::V8,
            ProtocolVersion::V1_0(_) => LedgerVersion::V8,
            ProtocolVersion::V2_0(_) => LedgerVersion::V9,
            ProtocolVersion::V2_1(_) => LedgerVersion::V9,
        }
    }

    pub fn node_version(self) -> NodeVersion {
        match self {
            ProtocolVersion::V0_22(_) => NodeVersion::V0_22,
            ProtocolVersion::V1_0(_) => NodeVersion::V1_0,
            ProtocolVersion::V2_0(_) => NodeVersion::V2_0,
            ProtocolVersion::V2_1(_) => NodeVersion::V2_1,
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
}

#[cfg(test)]
mod tests {
    use crate::domain::{LedgerVersion, NodeVersion, ProtocolVersion, ProtocolVersionError};
    use assert_matches::assert_matches;

    #[test]
    fn test_protocol_version() {
        let version = ProtocolVersion::try_from(0_019_000_u32);
        assert_matches!(version, Err(ProtocolVersionError::Unsupported(v)) if v == 0_019_000);

        let version = ProtocolVersion::try_from(0_021_000_u32);
        assert_matches!(version, Err(ProtocolVersionError::Unsupported(v)) if v == 0_021_000);

        let version = ProtocolVersion::try_from(0_023_000_u32);
        assert_matches!(version, Err(ProtocolVersionError::Unsupported(v)) if v == 0_023_000);

        let version = ProtocolVersion::try_from(1_001_000_u32);
        assert_matches!(version, Err(ProtocolVersionError::Unsupported(v)) if v == 1_001_000);

        let version = ProtocolVersion::try_from(2_002_000_u32);
        assert_matches!(version, Err(ProtocolVersionError::Unsupported(v)) if v == 2_002_000);

        let version =
            ProtocolVersion::try_from(0_022_666_u32).expect("0_022_666 is valid protocol version");
        assert_eq!(version.ledger_version(), LedgerVersion::V8);
        assert_eq!(version.node_version(), NodeVersion::V0_22);

        let version =
            ProtocolVersion::try_from(1_000_000_u32).expect("1_000_000 is valid protocol version");
        assert_eq!(version.ledger_version(), LedgerVersion::V8);
        assert_eq!(version.node_version(), NodeVersion::V1_0);

        let version =
            ProtocolVersion::try_from(2_000_000_u32).expect("2_000_000 is valid protocol version");
        assert_eq!(version.ledger_version(), LedgerVersion::V9);
        assert_eq!(version.node_version(), NodeVersion::V2_0);

        let version =
            ProtocolVersion::try_from(2_001_000_u32).expect("2_001_000 is valid protocol version");
        assert_eq!(version.ledger_version(), LedgerVersion::V9);
        assert_eq!(version.node_version(), NodeVersion::V2_1);
    }
}
