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

//! How a block's extrinsics were executed: the [`Phase`] of block execution an item is recorded
//! under, the [`ExtrinsicIndex`] and [`EventIndex`] it comes from, and how a transaction was
//! [`Applied`].

use indexer_common::domain::TransactionHash;

/// The index of an extrinsic in the block body.
pub type ExtrinsicIndex = u32;
/// The index of an event in the block's `System.Events`. The genesis block has no events; there
/// it is the position in the storage read the item comes from, which is ordered by hashed key and
/// so is determined by the chain state.
pub type EventIndex = u32;

/// The phase of block execution an item is recorded under. Declared in execution order, so the
/// derived `Ord` matches it.
///
/// This is not FRAME's `Phase`, whose declaration order is not execution order. FRAME stamps the
/// events of the step after the inherents with the index of the next extrinsic, which may not
/// exist; that is kept as is, because it is what the chain records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Phase {
    Initialization,
    ApplyExtrinsic(ExtrinsicIndex),
    Finalization,
}

/// How a transaction was applied, with the hash recorded for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applied {
    Fully { tx_hash: TransactionHash },
    Partially { tx_hash: TransactionHash },
}

impl Applied {
    pub fn tx_hash(&self) -> TransactionHash {
        match self {
            Self::Fully { tx_hash } | Self::Partially { tx_hash } => *tx_hash,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Applied, Phase::*};

    #[test]
    fn phase_orders_by_execution() {
        let mut phases = vec![
            Finalization,
            ApplyExtrinsic(2),
            Initialization,
            ApplyExtrinsic(0),
        ];
        phases.sort();

        assert_eq!(
            phases,
            [
                Initialization,
                ApplyExtrinsic(0),
                ApplyExtrinsic(2),
                Finalization
            ]
        );
    }

    #[test]
    fn applied_carries_the_hash() {
        let tx_hash = [1; 32].into();

        assert_eq!(Applied::Fully { tx_hash }.tx_hash(), tx_hash);
        assert_eq!(Applied::Partially { tx_hash }.tx_hash(), tx_hash);
    }
}
