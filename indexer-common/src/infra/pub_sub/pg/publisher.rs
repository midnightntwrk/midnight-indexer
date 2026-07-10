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

use crate::{
    domain::{Message, Publisher},
    infra::pub_sub::pg::channel,
};
use sqlx::Postgres;
use thiserror::Error;

/// Postgres `LISTEN`/`NOTIFY` based [Publisher].
///
/// [Publisher::stage] issues `pg_notify` on the caller's transaction; Postgres holds the
/// notification until `COMMIT` and discards it on rollback, so the notification is delivered iff
/// the transaction commits. [Publisher::deliver] is therefore a no-op.
#[derive(Debug, Clone, Copy, Default)]
pub struct PgPublisher;

impl Publisher for PgPublisher {
    type Database = Postgres;
    type Pending = ();
    type Error = PublisherError;

    async fn stage<T>(
        &self,
        tx: &mut sqlx::Transaction<'static, Postgres>,
        message: &T,
    ) -> Result<Self::Pending, Self::Error>
    where
        T: Message + Send + Sync,
    {
        let payload = serde_json::to_string(message)?;
        // NOTIFY payloads are capped at 8000 bytes; pub-sub messages are tens of bytes.
        debug_assert!(payload.len() < 8000, "NOTIFY payload exceeds 8000 bytes");
        sqlx::query("SELECT pg_notify($1, $2)")
            .bind(channel(T::TOPIC))
            .bind(payload)
            .execute(&mut **tx)
            .await?;
        Ok(())
    }

    async fn deliver(&self, _pending: Self::Pending) -> Result<(), Self::Error> {
        // Postgres delivers the NOTIFY on commit itself; nothing to do here.
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum PublisherError {
    #[error("cannot JSON serialize message")]
    Serialize(#[from] serde_json::Error),

    #[error("cannot execute pg_notify")]
    Notify(#[from] sqlx::Error),
}
