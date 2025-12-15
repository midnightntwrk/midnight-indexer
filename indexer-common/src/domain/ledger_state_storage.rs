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

#![cfg_attr(coverage_nightly, coverage(off))]

use crate::domain::SerializedLedgerState;
use std::{convert::Infallible, error::Error as StdError};

/// Abstraction for ledger state storage.
#[trait_variant::make(Send)]
pub trait LedgerStateStorage: Clone + Sync + 'static {
    type Error: StdError + Send + Sync + 'static;

    /// Load the ledger state.
    async fn load(&self, key: &str) -> Result<SerializedLedgerState, Self::Error>;

    /// Save the given ledger state.
    async fn save(
        &mut self,
        ledger_state: &SerializedLedgerState,
        key: &str,
    ) -> Result<(), Self::Error>;
}

#[derive(Clone)]
pub struct NoopLedgerStateStorage;

#[allow(unused_variables)]
impl LedgerStateStorage for NoopLedgerStateStorage {
    type Error = Infallible;

    async fn load(&self, key: &str) -> Result<SerializedLedgerState, Self::Error> {
        unimplemented!()
    }

    async fn save(
        &mut self,
        ledger_state: &SerializedLedgerState,
        key: &str,
    ) -> Result<(), Self::Error> {
        unimplemented!()
    }
}
