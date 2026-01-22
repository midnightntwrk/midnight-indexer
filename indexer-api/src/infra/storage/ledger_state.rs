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

use crate::{domain::storage::ledger_state::LedgerStateStorage, infra::storage::Storage};
use indexer_common::domain::{ProtocolVersion, SerializedLedgerStateKey};
use indoc::indoc;
use std::io;

impl LedgerStateStorage for Storage {
    async fn get_ledger_state(
        &self,
    ) -> Result<Option<(SerializedLedgerStateKey, ProtocolVersion)>, sqlx::Error> {
        todo!()
        // let query = indoc! {"
        //     SELECT
        //         protocol_version,
        //         ab_selector
        //     FROM ledger_state
        //     WHERE id = 0
        // "};

        // let Some((protocol_version, ab_selector)) = sqlx::query_as::<_, (i64, ABSelector)>(query)
        //     .fetch_optional(&*self.pool)
        //     .await?
        // else {
        //     return Ok(None);
        // };

        // let ledger_state = self
        //     .ledger_state_storage
        //     .load(ab_selector.as_str())
        //     .await
        //     .map_err(|error| sqlx::Error::Io(io::Error::other(error)))?;

        // Ok(Some((ledger_state, (protocol_version as u32).into())))
    }
}
