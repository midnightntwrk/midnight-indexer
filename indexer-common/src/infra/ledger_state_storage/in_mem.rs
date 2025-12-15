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

use crate::domain::{LedgerStateStorage, SerializedLedgerState};
use parking_lot::RwLock;
use std::{collections::HashMap, sync::Arc};
use thiserror::Error;

/// In-memory based ledger state storage implementation.
#[derive(Default, Clone)]
pub struct InMemLedgerStateStorage {
    ledger_state: Arc<RwLock<HashMap<String, SerializedLedgerState>>>,
}

impl LedgerStateStorage for InMemLedgerStateStorage {
    type Error = NotFound;

    async fn load(&self, key: &str) -> Result<SerializedLedgerState, Self::Error> {
        self.ledger_state
            .read()
            .get(key)
            .cloned()
            .ok_or_else(|| NotFound(key.to_owned()))
    }

    async fn save(
        &mut self,
        ledger_state: &SerializedLedgerState,
        key: &str,
    ) -> Result<(), Self::Error> {
        self.ledger_state
            .write()
            .insert(key.to_owned(), ledger_state.to_owned());
        Ok(())
    }
}

#[derive(Debug, Error)]
#[error("ledger state for key {0} not found")]
pub struct NotFound(String);
