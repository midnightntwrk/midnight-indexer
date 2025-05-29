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

use crate::domain::{LedgerStateStorage, RawLedgerState};
use parking_lot::RwLock;
use std::{convert::Infallible, sync::Arc};

/// In-memory based ledger state storage implementation.
#[derive(Default, Clone)]
pub struct InMemZswapStateStorage {
    data: Arc<RwLock<Data>>,
}

impl LedgerStateStorage for InMemZswapStateStorage {
    type Error = Infallible;

    async fn load_last_index(&self) -> Result<Option<u64>, Self::Error> {
        Ok(self.data.read().last_index)
    }

    async fn load_ledger_state(&self) -> Result<Option<(RawLedgerState, u32)>, Self::Error> {
        Ok(self.data.read().zswap_state.clone())
    }

    async fn save(
        &mut self,
        zswap_state: &RawLedgerState,
        block_height: u32,
        last_index: Option<u64>,
    ) -> Result<(), Self::Error> {
        let mut data = self.data.write();

        data.zswap_state = Some((zswap_state.to_owned(), block_height));
        data.last_index = last_index;

        Ok(())
    }
}

#[derive(Default)]
struct Data {
    zswap_state: Option<(RawLedgerState, u32)>,
    last_index: Option<u64>,
}
