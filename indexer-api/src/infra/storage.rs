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

mod block;
mod bridge;
mod contract_action;
mod contract_event;
mod dust;
mod dust_generations;
mod ledger_events;
mod ledger_state;
mod shielded_nullifiers;
mod spo;
mod system_parameters;
mod transaction;
mod unshielded;
mod wallet;

use crate::domain;
use chacha20poly1305::ChaCha20Poly1305;

/// Unified storage implementation for PostgreSQL (cloud) and SQLite (standalone). Uses Cargo
/// features to select the appropriate database backend at build time.
#[derive(Clone)]
pub struct Storage {
    cipher: ChaCha20Poly1305,

    #[cfg(feature = "cloud")]
    pool: indexer_common::infra::pool::postgres::PostgresPool,

    #[cfg(feature = "standalone")]
    pool: indexer_common::infra::pool::sqlite::SqlitePool,
}

impl Storage {
    #[cfg(feature = "cloud")]
    pub fn new(
        cipher: ChaCha20Poly1305,
        pool: indexer_common::infra::pool::postgres::PostgresPool,
    ) -> Self {
        Self { cipher, pool }
    }

    #[cfg(feature = "standalone")]
    pub fn new(
        cipher: ChaCha20Poly1305,
        pool: indexer_common::infra::pool::sqlite::SqlitePool,
    ) -> Self {
        Self { cipher, pool }
    }
}

impl domain::storage::Storage for Storage {}

/// Exclusive lexicographic upper bound for a byte prefix. Carry through trailing `ff`
/// bytes and truncate them: `12ff` ends at `13`. An all-`ff` (or empty) prefix has no
/// finite upper bound.
fn prefix_upper_bound(prefix: &[u8]) -> Option<Vec<u8>> {
    prefix.iter().rposition(|&b| b < u8::MAX).map(|i| {
        let mut upper = prefix[..=i].to_vec();
        // `rposition` only finds bytes below `0xff`, so this cannot overflow.
        upper[i] += 1;
        upper
    })
}

#[cfg(test)]
mod tests {
    use super::prefix_upper_bound;

    #[test]
    fn carries_through_trailing_ff() {
        assert_eq!(prefix_upper_bound(&[]), None);
        assert_eq!(prefix_upper_bound(&[0x00]), Some(vec![0x01]));
        assert_eq!(prefix_upper_bound(&[0x12]), Some(vec![0x13]));
        assert_eq!(prefix_upper_bound(&[0x12, 0xfe]), Some(vec![0x12, 0xff]));
        assert_eq!(prefix_upper_bound(&[0x12, 0xff]), Some(vec![0x13]));
        assert_eq!(prefix_upper_bound(&[0x12, 0xff, 0xff]), Some(vec![0x13]));
        assert_eq!(prefix_upper_bound(&[0xff]), None);
        assert_eq!(prefix_upper_bound(&[0xff; 32]), None);
    }
}
