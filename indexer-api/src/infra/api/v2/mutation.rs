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
    domain::{AsBytesExt, HexEncoded, Storage, ViewingKey},
    infra::api::{ContextExt, Unit},
};
use anyhow::Context as AnyhowContext;
use async_graphql::{Context, Object};
use indexer_common::{error::StdErrorExt};
use std::marker::PhantomData;
use tracing::{debug, error, instrument};

pub struct Mutation<S> {
    _s: PhantomData<S>,
}

impl<S> Default for Mutation<S> {
    fn default() -> Self {
        Self { _s: PhantomData }
    }
}

#[Object]
impl<S> Mutation<S>
where
    S: Storage,
{
    /// Connect a wallet using a viewing key, returns a session ID.
    #[instrument(skip(self, cx, viewing_key))]
    async fn connect(
        &self,
        cx: &Context<'_>,
        viewing_key: ViewingKey,
    ) -> async_graphql::Result<HexEncoded> {
        let common_viewing_key = deserialize_and_validate_key(viewing_key, cx.get_network_id()?)?;

        cx.get_storage::<S>()?
            .connect_wallet(&common_viewing_key)
            .await
            .inspect_err(|error| error!(error = error.as_chain(), "cannot connect wallet"))?;
        let session_id = common_viewing_key.as_session_id();
        debug!(?session_id, "wallet connected");

        Ok(session_id.hex_encode())
    }

    /// Disconnect a wallet using the session ID.
    #[instrument(skip(self, cx))]
    async fn disconnect(
        &self,
        cx: &Context<'_>,
        session_id: HexEncoded,
    ) -> async_graphql::Result<Unit> {
        let session_id = session_id.hex_decode().context("decode session ID")?;

        cx.get_storage::<S>()?
            .disconnect_wallet(&session_id)
            .await
            .inspect_err(|error| error!(error = error.as_chain(), "cannot disconnect wallet"))?;
        debug!(?session_id, "wallet disconnected");

        Ok(Unit)
    }
}
