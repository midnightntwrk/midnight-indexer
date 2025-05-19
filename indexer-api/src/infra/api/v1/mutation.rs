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
use async_graphql::{Context, Object};
use fastrace::trace;
use indexer_common::{
    domain::{SessionId, ViewingKey as CommonViewingKey},
    error::StdErrorExt,
};
use log::{debug, error};
use std::marker::PhantomData;

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
    #[trace]
    async fn connect(
        &self,
        cx: &Context<'_>,
        viewing_key: ViewingKey,
    ) -> async_graphql::Result<HexEncoded> {
        let viewing_key = CommonViewingKey::try_from(viewing_key)
            .map_err(|error| async_graphql::Error::new(error.to_string()))?;

        if !viewing_key.validate(cx.get_network_id()?) {
            return Err(async_graphql::Error::new("invalid viewing key"));
        }

        cx.get_storage::<S>()?
            .connect_wallet(&viewing_key)
            .await
            .inspect_err(|error| error!(error:? = error.as_chain(); "cannot connect wallet"))?;
        let session_id = viewing_key.as_session_id();
        debug!(session_id:?; "wallet connected");

        Ok(session_id.hex_encode())
    }

    /// Disconnect a wallet using the session ID.
    #[trace]
    async fn disconnect(
        &self,
        cx: &Context<'_>,
        session_id: HexEncoded,
    ) -> async_graphql::Result<Unit> {
        let session_id = session_id.hex_decode::<Vec<u8>>().map_err(|error| {
            async_graphql::Error::new(format!("cannot hex-decode session ID: {error}"))
        })?;
        let session_id = SessionId::try_from(session_id.as_slice())
            .map_err(|error| async_graphql::Error::new(format!("invalid session ID: {error}")))?;

        cx.get_storage::<S>()?
            .disconnect_wallet(&session_id)
            .await
            .inspect_err(|error| error!(error:? = error.as_chain(); "cannot disconnect wallet"))?;
        debug!(session_id:?; "wallet disconnected");

        Ok(Unit)
    }
}
