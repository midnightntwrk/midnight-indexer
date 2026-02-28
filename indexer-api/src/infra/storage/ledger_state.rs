// This file is part of midnight-indexer.
// Copyright (C) 2025-2026 Midnight Foundation
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

impl LedgerStateStorage for Storage {
    async fn get_highest_ledger_state(
        &self,
    ) -> Result<Option<(ProtocolVersion, SerializedLedgerStateKey)>, sqlx::Error> {
        let query = indoc! {"
            SELECT protocol_version, ledger_state_key
            FROM blocks
            ORDER BY height DESC
            LIMIT 1
        "};

        sqlx::query_as::<_, (i64, SerializedLedgerStateKey)>(query)
            .fetch_optional(&*self.pool)
            .await?
            .map(|(protocol_version, key)| {
                let protocol_version = ProtocolVersion::try_from(protocol_version)
                    .map_err(|error| sqlx::Error::Decode(error.into()))?;
                Ok((protocol_version, key))
            })
            .transpose()
    }
}
