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
    domain::{HexEncoded, Storage},
    infra::api::{
        v1::{into_from_height, BlockOffsetInput, ContractCallOrDeploy},
        ContextExt,
    },
};
use anyhow::Context as AnyhowContext;
use async_graphql::{async_stream::try_stream, Context, Subscription};
use fastrace::trace;
use futures::{Stream, TryStreamExt};
use indexer_common::{
    domain::{BlockIndexed, Subscriber},
    error::StdErrorExt,
};
use log::{debug, error, warn};
use std::{num::NonZeroU32, pin::pin};

// TODO: Make configurable!
const BATCH_SIZE: NonZeroU32 = NonZeroU32::new(100).unwrap();

pub struct ContractSubscription<S, B> {
    _storage: std::marker::PhantomData<S>,
    _subscriber: std::marker::PhantomData<B>,
}

impl<S, B> Default for ContractSubscription<S, B> {
    fn default() -> Self {
        Self {
            _storage: std::marker::PhantomData,
            _subscriber: std::marker::PhantomData,
        }
    }
}

#[Subscription]
impl<S, B> ContractSubscription<S, B>
where
    S: Storage,
    B: Subscriber,
{
    /// Subscribe to contract updates.
    #[trace]
    async fn contracts<'a>(
        &self,
        ctx: &'a Context<'a>,
        address: HexEncoded,
        offset: Option<BlockOffsetInput>,
    ) -> async_graphql::Result<
        impl Stream<Item = async_graphql::Result<ContractCallOrDeploy>> + use<'a, S, B>,
    > {
        let storage = ctx.get_storage::<S>()?;
        let subscriber = ctx.get_subscriber::<B>()?;

        let block_indexed_stream =
            subscriber
                .subscribe::<BlockIndexed>()
                .await
                .inspect_err(|error| {
                    error!(
                        error:? = error.as_chain();
                        "cannot subscribe to BlockIndexed events"
                    )
                })?;

        let address = address.hex_decode().context("decode address")?;
        let from_height = into_from_height(offset, storage).await?;

        let contract_updates_stream = try_stream! {
            let mut block_indexed_stream = pin!(block_indexed_stream);
            let mut next_from_contract_id = 0;

            // First get all stored `ContractActions`s from the requested `from_height`.
            let contract_actions = storage.get_contract_actions_by_address(
                &address,
                from_height,
                next_from_contract_id,
                BATCH_SIZE,
            );
            debug!(from_height; "got contract actions");

            // Then yield all stored `ContractAction`s.
            let mut contract_actions = pin!(contract_actions);
            while let Some(contract_action) =
                contract_actions.try_next().await.inspect_err(|error| {
                    error!(error:? = error.as_chain(); "cannot get next contract action")
                })?
            {
                next_from_contract_id = contract_action.id + 1;

                yield contract_action.into();
            }

            // Then get now stored `Contract`s after receiving a `BlockIndexed` event.
            while let Some(BlockIndexed { height, .. }) =
                block_indexed_stream.try_next().await.inspect_err(|error| {
                    error!(
                        error:? = error.as_chain();
                        "cannot get next BlockIndexed event"
                    )
                })?
            {
                debug!(height; "handling BlockIndexed event");

                let contract_actions = storage.get_contract_actions_by_address(
                    &address,
                    0,
                    next_from_contract_id,
                    BATCH_SIZE,
                );
                let mut contract_actions = pin!(contract_actions);
                while let Some(contract_action) =
                    contract_actions.try_next().await.inspect_err(|error| {
                        error!(error:? = error.as_chain(); "cannot get next contract action")
                    })?
                {
                    next_from_contract_id = contract_action.id + 1;

                    yield contract_action.into();
                }
            }

            warn!("stream of BlockIndexed events completed unexpectedly");
        };

        Ok(contract_updates_stream)
    }
}
