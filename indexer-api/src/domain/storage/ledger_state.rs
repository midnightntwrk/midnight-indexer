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

use indexer_common::domain::{ProtocolVersion, SerializedLedgerStateKey};

use crate::domain::storage::NoopStorage;

#[trait_variant::make(Send)]
pub trait LedgerStateStorage
where
    Self: Clone + Send + Sync + 'static,
{
    /// Get the ledger state key and protocol version.
    async fn get_ledger_state(
        &self,
    ) -> Result<Option<(SerializedLedgerStateKey, ProtocolVersion)>, sqlx::Error>;
}

#[allow(unused_variables)]
impl LedgerStateStorage for NoopStorage {
    async fn get_ledger_state(
        &self,
    ) -> Result<Option<(SerializedLedgerStateKey, ProtocolVersion)>, sqlx::Error> {
        unimplemented!()
    }
}
