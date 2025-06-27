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

use crate::{
    domain::{AsBytesExt, HexEncoded, PROTOCOL_VERSION, ViewingKey, storage::Storage},
    infra::api::{
        ContextExt, ResultExt,
        v1::{Unit, hex_decode_session_id},
    },
};
use anyhow::Context as _;
use async_graphql::{Context, Object};
use fastrace::trace;
use log::debug;
use metrics::{Counter, counter};
use std::marker::PhantomData;

pub struct Mutation<S> {
    connect_calls: Counter,
    disconnect_calls: Counter,
    _s: PhantomData<S>,
}

impl<S> Default for Mutation<S> {
    fn default() -> Self {
        let connect_calls = counter!("indexer_api_calls_mutation_connect");
        let disconnect_calls = counter!("indexer_api_calls_mutation_disconnect");

        Self {
            connect_calls,
            disconnect_calls,
            _s: PhantomData,
        }
    }
}

#[Object]
impl<S> Mutation<S>
where
    S: Storage,
{
    /// Connect the wallet with the given viewing key and return a session ID.
    #[trace]
    async fn connect(
        &self,
        cx: &Context<'_>,
        viewing_key: ViewingKey,
    ) -> async_graphql::Result<HexEncoded> {
        self.connect_calls.increment(1);

        let viewing_key = viewing_key
            .try_into_domain(cx.get_network_id(), PROTOCOL_VERSION)
            .context("decode viewing key")?;

        cx.get_storage::<S>()
            .connect_wallet(&viewing_key)
            .await
            .internal("connect wallet")?;

        let session_id = viewing_key.to_session_id();
        debug!(session_id:%; "wallet connected");

        Ok(session_id.hex_encode())
    }

    /// Disconnect the wallet with the given session ID.
    #[trace(properties = { "session_id": "{session_id}" })]
    async fn disconnect(
        &self,
        cx: &Context<'_>,
        session_id: HexEncoded,
    ) -> async_graphql::Result<Unit> {
        self.disconnect_calls.increment(1);

        let session_id = hex_decode_session_id(session_id)?;

        cx.get_storage::<S>()
            .disconnect_wallet(session_id)
            .await
            .internal("disconnect wallet")?;

        debug!(session_id:%; "wallet disconnected");

        Ok(Unit)
    }
}
