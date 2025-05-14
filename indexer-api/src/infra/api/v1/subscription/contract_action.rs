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
        ContextExt, ResultExt,
        v1::{BlockOffset, ContractAction, resolve_height},
    },
};
use anyhow::Context as AnyhowContext;
use async_graphql::{Context, Subscription, async_stream::try_stream};
use futures::{Stream, TryStreamExt};
use indexer_common::domain::{BlockIndexed, Subscriber};
use log::{debug, warn};
use metrics::{Counter, counter};
use std::{num::NonZeroU32, pin::pin};

// TODO: Make configurable!
const BATCH_SIZE: NonZeroU32 = NonZeroU32::new(100).unwrap();

pub struct ContractActionSubscription<S, B> {
    contract_actions_calls: Counter,
    _storage: std::marker::PhantomData<S>,
    _subscriber: std::marker::PhantomData<B>,
}

impl<S, B> Default for ContractActionSubscription<S, B> {
    fn default() -> Self {
        let contract_actions_calls = counter!("indexer_api_calls_subscription_contract_actions");

        Self {
            contract_actions_calls,
            _storage: std::marker::PhantomData,
            _subscriber: std::marker::PhantomData,
        }
    }
}

#[Subscription]
impl<S, B> ContractActionSubscription<S, B>
where
    S: Storage,
    B: Subscriber,
{
    /// Subscribe to contract actions.
    async fn contract_actions<'a>(
        &self,
        cx: &'a Context<'a>,
        address: HexEncoded,
        offset: Option<BlockOffset>,
    ) -> async_graphql::Result<
        impl Stream<Item = async_graphql::Result<ContractAction<S>>> + use<'a, S, B>,
    > {
        self.contract_actions_calls.increment(1);

        let storage = cx.get_storage::<S>();
        let subscriber = cx.get_subscriber::<B>();

        let block_indexed_stream = subscriber
            .subscribe::<BlockIndexed>()
            .await
            .internal("subscribe to BlockIndexed events")?;

        let address = address.hex_decode().context("hex-decode address")?;

        // It is fine to use `?` on `resolve_height`, because it implements correct error handling.
        let height = resolve_height(offset, storage).await?;

        let contract_actions = try_stream! {
            let mut block_indexed_stream = pin!(block_indexed_stream);
            let mut next_contract_action_id = 0;

            // First get all stored `ContractActions`s from the requested `height`.
            let contract_actions = storage.get_contract_actions_by_address(
                &address,
                height,
                next_contract_action_id,
                BATCH_SIZE,
            );
            debug!(height; "got contract actions");

            // Then yield all stored `ContractAction`s.
            let mut contract_actions = pin!(contract_actions);
            while let Some(contract_action) = contract_actions
                .try_next()
                .await
                .internal("get next contract action")?
            {
                next_contract_action_id = contract_action.id + 1;

                yield contract_action.into();
            }

            // Then get now stored `Contract`s after receiving a `BlockIndexed` event.
            while let Some(BlockIndexed { height, .. }) = block_indexed_stream
                .try_next()
                .await
                .internal("get next BlockIndexed event")?
            {
                debug!(height; "handling BlockIndexed event");

                let contract_actions = storage.get_contract_actions_by_address(
                    &address,
                    0,
                    next_contract_action_id,
                    BATCH_SIZE,
                );
                let mut contract_actions = pin!(contract_actions);

                while let Some(contract_action) = contract_actions
                    .try_next()
                    .await
                    .internal("get next contract action")?
                {
                    next_contract_action_id = contract_action.id + 1;

                    yield contract_action.into();
                }
            }

            warn!("stream of BlockIndexed events completed unexpectedly");
        };

        Ok(contract_actions)
    }
}
