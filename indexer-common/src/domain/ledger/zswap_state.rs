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

use midnight_coin_structure_v2::contract::ContractAddress as ContractAddressV8;
use midnight_coin_structure_v3::contract::ContractAddress as ContractAddressV9;
use midnight_storage_core_v1::{arena::Sp, db::DB};
use midnight_transient_crypto_v2::merkle_tree::MerkleTree as MerkleTreeV8;
use midnight_transient_crypto_v3::merkle_tree::MerkleTree as MerkleTreeV9;
use midnight_zswap_v8::ledger::State as ZswapStateV8;
use midnight_zswap_v9::ledger::State as ZswapStateV9;
use std::ops::Range;

/// Zswap state operations for contract entries that read only the commitment tree leaves they need,
/// where zswap's `State::filter` scans every leaf.
pub trait ZswapStateExt {
    /// The contract address a commitment tree leaf can carry as its owner.
    type Address;
    /// The commitment tree, its leaves annotated with their owning contract.
    type CommitmentTree;

    /// Returns the index and owner of every commitment tree leaf in `range` that has an owner.
    fn owned_leaves(&self, range: Range<u64>) -> impl Iterator<Item = (u64, &Self::Address)>;

    /// Returns the commitment tree with every leaf below `first_free` collapsed except those at
    /// `retained`, which must be ascending.
    ///
    /// Given the indices of a contract's entries, the result is identical to `State::filter` for
    /// that contract.
    fn filter_indices(&self, retained: &[u64]) -> Self::CommitmentTree;
}

impl<D: DB> ZswapStateExt for ZswapStateV8<D> {
    type Address = ContractAddressV8;
    type CommitmentTree = MerkleTreeV8<Option<Sp<ContractAddressV8, D>>, D>;

    fn owned_leaves(&self, range: Range<u64>) -> impl Iterator<Item = (u64, &Self::Address)> {
        range.filter_map(|index| match self.coin_coms.index(index) {
            Some((_, Some(address))) => Some((index, &**address)),
            _ => None,
        })
    }

    fn filter_indices(&self, retained: &[u64]) -> Self::CommitmentTree {
        collapse_except(
            &self.coin_coms,
            retained,
            self.first_free,
            |tree, start, end| tree.collapse(start, end),
        )
    }
}

impl<D: DB> ZswapStateExt for ZswapStateV9<D> {
    type Address = ContractAddressV9;
    type CommitmentTree = MerkleTreeV9<Option<Sp<ContractAddressV9, D>>, D>;

    fn owned_leaves(&self, range: Range<u64>) -> impl Iterator<Item = (u64, &Self::Address)> {
        range.filter_map(|index| match self.coin_coms.index(index) {
            Some((_, Some(address))) => Some((index, &**address)),
            _ => None,
        })
    }

    fn filter_indices(&self, retained: &[u64]) -> Self::CommitmentTree {
        collapse_except(
            &self.coin_coms,
            retained,
            self.first_free,
            |tree, start, end| tree.collapse(start, end),
        )
    }
}

// Collapses every leaf of `tree` below `first_free` except those at `retained`, with the same
// `collapse` calls `State::filter` makes after scanning for them, so the result is identical.
fn collapse_except<Tree: Clone>(
    tree: &Tree,
    retained: &[u64],
    first_free: u64,
    collapse: impl Fn(&Tree, u64, u64) -> Tree,
) -> Tree {
    let mut tree = tree.clone();
    let mut start = 0;
    for &index in retained {
        if index > 0 {
            tree = collapse(&tree, start, index - 1);
        }
        if index < u64::MAX {
            start = index + 1;
        }
    }

    if first_free > 0 {
        collapse(&tree, start, first_free - 1)
    } else {
        tree
    }
}

#[cfg(test)]
mod tests {
    // The same tests for each zswap version, whose types differ only by crate.
    macro_rules! zswap_state_tests {
        ($version:ident, $state:ident, $address:ident, $fr:path) => {
            mod $version {
                use crate::domain::ledger::ZswapStateExt;
                use midnight_base_crypto_v1::hash::HashOutput;
                use midnight_storage_core_v1::{arena::Sp, db::InMemoryDB};
                use $fr as Fr;

                type State = super::super::$state<InMemoryDB>;
                type Address = super::super::$address;

                const LEAVES: u64 = 20;

                // Returns a state of `LEAVES` commitments and three contracts: the first owns every
                // third leaf, the second every third from leaf 1 below leaf 9, the third none.
                fn state() -> (State, [Address; 3]) {
                    let addresses =
                        [7, 8, 9].map(|byte| super::super::$address(HashOutput([byte; 32])));
                    let mut state = State::new();
                    state.coin_coms = (0..LEAVES).fold(state.coin_coms.clone(), |tree, index| {
                        tree.try_update(index, &Fr::from(index + 1), owner(&addresses, index))
                            .expect("insert coin commitment")
                    });
                    state.first_free = LEAVES;
                    (state, addresses)
                }

                fn owner(addresses: &[Address; 3], index: u64) -> Option<Sp<Address, InMemoryDB>> {
                    match index % 3 {
                        0 => Some(Sp::new(addresses[0])),
                        1 if index < 9 => Some(Sp::new(addresses[1])),
                        _ => None,
                    }
                }

                #[test]
                fn owned_leaves_yields_the_owned_leaves_in_range() {
                    let (state, addresses) = state();

                    for range in [0..LEAVES, 5..12, 12..12] {
                        let expected = range
                            .clone()
                            .filter_map(|index| {
                                owner(&addresses, index).map(|address| (index, *address))
                            })
                            .collect::<Vec<_>>();
                        let owned = state
                            .owned_leaves(range)
                            .map(|(index, address)| (index, *address))
                            .collect::<Vec<_>>();
                        assert_eq!(owned, expected);
                    }
                }

                #[test]
                fn filter_indices_matches_filter() {
                    let (state, addresses) = state();

                    for address in addresses {
                        let indices = state
                            .owned_leaves(0..LEAVES)
                            .filter(|(_, owner)| **owner == address)
                            .map(|(index, _)| index)
                            .collect::<Vec<_>>();
                        assert!(state.filter_indices(&indices) == state.filter(&[address]));
                    }

                    let empty = State::new();
                    assert!(empty.filter_indices(&[]) == empty.filter(&[addresses[0]]));
                }
            }
        };
    }

    zswap_state_tests!(
        v8,
        ZswapStateV8,
        ContractAddressV8,
        midnight_transient_crypto_v2::curve::Fr
    );
    zswap_state_tests!(
        v9,
        ZswapStateV9,
        ContractAddressV9,
        midnight_transient_crypto_v3::curve::Fr
    );
}
