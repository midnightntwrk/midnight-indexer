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

//! Mirror of the node's ledger wrapper (`Ledger` in midnight-node `ledger/src/ledger_{8,9}/api/
//! ledger.rs`), whose tagged arena key the node stores as `pallet_midnight::StateKey`.
//!
//! The layout and tags must match the node byte for byte, otherwise every block fails validation.

use midnight_serialize_v1::{self as serialize, Tagged};
use midnight_storage_v2::{self as storage, Storable, arena::ArenaKey, db::DB, storable::Loader};
use std::{borrow::Cow, marker::PhantomData};

/// Mirror of the node's `StorableSyntheticCost`: the block fullness accrued by the transactions
/// applied so far in the current block.
#[derive(Debug, Storable)]
#[storable(db = D)]
struct StorableSyntheticCost<D: DB> {
    read_time: u64,
    compute_time: u64,
    block_usage: u64,
    bytes_written: u64,
    bytes_churned: u64,
    _marker: PhantomData<D>,
}

impl<D: DB> StorableSyntheticCost<D> {
    fn zero() -> Self {
        Self {
            read_time: 0,
            compute_time: 0,
            block_usage: 0,
            bytes_written: 0,
            bytes_churned: 0,
            _marker: PhantomData,
        }
    }
}

impl<D: DB> Clone for StorableSyntheticCost<D> {
    fn clone(&self) -> Self {
        Self {
            read_time: self.read_time,
            compute_time: self.compute_time,
            block_usage: self.block_usage,
            bytes_written: self.bytes_written,
            bytes_churned: self.bytes_churned,
            _marker: PhantomData,
        }
    }
}

macro_rules! node_ledger {
    ($name:ident, $ledger_state:ty) => {
        /// Mirror of the node's `Ledger` wrapper for one ledger version.
        #[derive(Debug, Storable)]
        #[storable(db = D)]
        pub(super) struct $name<D: DB> {
            state: $ledger_state,
            block_fullness: StorableSyntheticCost<D>,
        }

        impl<D: DB> $name<D> {
            /// The wrapper as committed at the end of a block, where `on_finalize` has reset the
            /// block fullness to zero; genesis and the v8 to v9 migration also start at zero.
            pub(super) fn end_of_block(state: $ledger_state) -> Self {
                Self {
                    state,
                    block_fullness: StorableSyntheticCost::zero(),
                }
            }
        }

        impl<D: DB> Clone for $name<D> {
            fn clone(&self) -> Self {
                Self {
                    state: self.state.clone(),
                    block_fullness: self.block_fullness.clone(),
                }
            }
        }

        impl<D: DB> Tagged for $name<D> {
            fn tag() -> Cow<'static, str> {
                <$ledger_state as Tagged>::tag()
            }

            fn tag_unique_factor() -> String {
                <$ledger_state as Tagged>::tag_unique_factor()
            }
        }
    };
}

node_ledger!(NodeLedgerV8, midnight_ledger_v8::structure::LedgerState<D>);
node_ledger!(NodeLedgerV9, midnight_ledger_v9::structure::LedgerState<D>);
