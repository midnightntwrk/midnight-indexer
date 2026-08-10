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

use crate::domain::storage::NoopStorage;
use indexer_common::domain::{ByteVec, ViewingKey};
use std::time::Duration;
use uuid::Uuid;

#[trait_variant::make(Send)]
pub trait WalletStorage
where
    Self: Clone + Send + Sync + 'static,
{
    /// Connect a wallet, i.e. add it to the active ones, and return its wallet ID along with a
    /// sealed session token. If `start_index` is provided, transactions before that index are
    /// skipped.
    async fn connect_wallet(
        &self,
        viewing_key: &ViewingKey,
        start_index: Option<u64>,
    ) -> Result<(Uuid, ByteVec), sqlx::Error>;

    /// Disconnect a wallet session and return whether it was valid. A legacy session ID is
    /// removed; a sealed session token cannot be revoked on other instances, so it is not revoked
    /// here either — the wallet drops out of the active set once the inactivity TTL lapses.
    async fn disconnect_wallet(
        &self,
        session: &[u8],
        token_ttl: Duration,
    ) -> Result<bool, sqlx::Error>;

    /// Resolve a sealed session token or a legacy session ID to the corresponding wallet ID. A
    /// valid token upserts the wallet, so any instance sharing the cipher key can serve it.
    async fn resolve_session(
        &self,
        session: &[u8],
        token_ttl: Duration,
    ) -> Result<Option<Uuid>, sqlx::Error>;

    /// Refresh the wallet's last active timestamp to avoid timing out.
    async fn keep_wallet_active(&self, wallet_id: Uuid) -> Result<(), sqlx::Error>;
}

#[allow(unused_variables)]
impl WalletStorage for NoopStorage {
    async fn connect_wallet(
        &self,
        viewing_key: &ViewingKey,
        start_index: Option<u64>,
    ) -> Result<(Uuid, ByteVec), sqlx::Error> {
        unimplemented!()
    }

    async fn disconnect_wallet(
        &self,
        session: &[u8],
        token_ttl: Duration,
    ) -> Result<bool, sqlx::Error> {
        unimplemented!()
    }

    async fn resolve_session(
        &self,
        session: &[u8],
        token_ttl: Duration,
    ) -> Result<Option<Uuid>, sqlx::Error> {
        unimplemented!()
    }

    async fn keep_wallet_active(&self, wallet_id: Uuid) -> Result<(), sqlx::Error> {
        unimplemented!()
    }
}
