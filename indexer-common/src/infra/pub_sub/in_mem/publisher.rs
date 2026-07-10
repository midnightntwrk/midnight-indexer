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
    domain::{Message, Publisher, Topic},
    infra::pub_sub::in_mem::InMemPubSub,
};
use serde_json::Value;
use std::fmt::Debug;
use thiserror::Error;

/// In memory based implementations for [Publisher].
#[derive(Clone)]
pub struct InMemPublisher(InMemPubSub);

impl InMemPublisher {
    #[allow(missing_docs)]
    pub fn new(in_mem_pub_sub: InMemPubSub) -> Self {
        Self(in_mem_pub_sub)
    }
}

impl Publisher for InMemPublisher {
    type Database = sqlx::Sqlite;
    type Pending = (Topic, Value);
    type Error = PublisherError;

    /// Capture the message for delivery in [Publisher::deliver]. The transaction is ignored: an
    /// in-memory broadcast cannot join a SQLite commit, so delivery is deferred to after the
    /// caller commits (see [Publisher::deliver]) to preserve "delivered iff committed".
    async fn stage<T>(
        &self,
        _tx: &mut sqlx::Transaction<'static, sqlx::Sqlite>,
        message: &T,
    ) -> Result<Self::Pending, Self::Error>
    where
        T: Message + Send + Sync,
    {
        let value = serde_json::to_value(message)?;
        Ok((T::TOPIC, value))
    }

    async fn deliver(&self, pending: Self::Pending) -> Result<(), Self::Error> {
        let (topic, value) = pending;

        match topic {
            Topic("BlockIndexed") => {
                self.0.block_indexed_sender.send(value)?;
            }

            Topic("WalletIndexed") => {
                self.0.wallet_indexed_sender.send(value)?;
            }

            Topic("UnshieldedUtxoIndexed") => {
                self.0.unshielded_utxo_sender.send(value)?;
            }

            // This must not happen; if it happens, we forgot to add an arm for the topic above!
            _ => panic!("unexpected topic {topic:?}"),
        }

        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum PublisherError {
    #[error("cannot JSON serialize message")]
    Serialize(#[from] serde_json::Error),

    #[error("cannot send message")]
    Send(#[from] tokio::sync::broadcast::error::SendError<Value>),
}
