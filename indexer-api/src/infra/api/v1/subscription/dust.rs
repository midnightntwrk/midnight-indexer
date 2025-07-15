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
    domain::storage::Storage,
    infra::api::{
        ApiError, ApiResult, ContextExt, InnerApiError, ResultExt,
        v1::dust::{
            DustCommitmentEventGraphQL, DustGenerationEventGraphQL,
            DustNullifierTransactionEventGraphQL, RegistrationAddress,
            RegistrationUpdateEventGraphQL,
        },
    },
};
use async_graphql::{Context, Subscription, async_stream::try_stream};
use fastrace::trace;
use futures::{Stream, TryStreamExt};
use indexer_common::domain::Subscriber;
use log::warn;
use std::{marker::PhantomData, num::NonZeroU32, pin::pin};

const BATCH_SIZE: NonZeroU32 = NonZeroU32::new(100).unwrap();

pub struct DustSubscription<S, B> {
    _s: PhantomData<S>,
    _b: PhantomData<B>,
}

impl<S, B> Default for DustSubscription<S, B> {
    fn default() -> Self {
        Self {
            _s: PhantomData,
            _b: PhantomData,
        }
    }
}

#[Subscription]
impl<S, B> DustSubscription<S, B>
where
    S: Storage,
    B: Subscriber,
{
    /// Stream generation info with merkle updates for wallet reconstruction.
    #[trace(properties = { "dust_address": "{dust_address}", "from_generation_index": "{from_generation_index:?}", "from_merkle_index": "{from_merkle_index:?}", "only_active": "{only_active:?}" })]
    async fn dust_generations<'a>(
        &self,
        cx: &'a Context<'a>,
        dust_address: String,
        from_generation_index: Option<i64>,
        from_merkle_index: Option<i64>,
        only_active: Option<bool>,
    ) -> Result<impl Stream<Item = ApiResult<DustGenerationEventGraphQL>> + use<'a, S, B>, ApiError>
    {
        let storage = cx.get_storage::<S>();
        let from_generation_index = from_generation_index.unwrap_or(0);
        let from_merkle_index = from_merkle_index.unwrap_or(0);
        let only_active = only_active.unwrap_or(true);

        let dust_generations = try_stream! {
            let generation_stream = storage.get_dust_generations(
                &dust_address,
                from_generation_index,
                from_merkle_index,
                only_active,
                BATCH_SIZE
            );

            let mut generation_stream = pin!(generation_stream);
            while let Some(event) = generation_stream
                .try_next()
                .await
                .map_err_into_server_error(|| {
                    format!("get next dust generation event for address {dust_address}")
                })?
            {
                yield event.into();
            }

            warn!("stream of dust generation events completed unexpectedly");
        };

        Ok(dust_generations)
    }

    /// Stream regular transactions containing DUST nullifiers.
    #[trace(properties = { "prefixes": "{prefixes:?}", "min_prefix_length": "{min_prefix_length:?}", "from_block": "{from_block:?}" })]
    async fn dust_nullifier_transactions<'a>(
        &self,
        cx: &'a Context<'a>,
        prefixes: Vec<String>,
        min_prefix_length: Option<i32>,
        from_block: Option<i64>,
    ) -> Result<
        impl Stream<Item = ApiResult<DustNullifierTransactionEventGraphQL>> + use<'a, S, B>,
        ApiError,
    > {
        let storage = cx.get_storage::<S>();
        let from_block = from_block.unwrap_or(0);
        let min_prefix_length = min_prefix_length.unwrap_or(8) as usize;

        // Prevent DOS attacks by limiting database query complexity
        if prefixes.len() > 10 {
            return Err(ApiError::Client(InnerApiError(
                "Maximum 10 prefixes allowed per request".to_owned(),
                None,
            )));
        }

        // Preserve privacy by ensuring sufficient anonymity set size
        if prefixes.iter().any(|p| p.len() < min_prefix_length) {
            return Err(ApiError::Client(InnerApiError(
                format!("All prefixes must be at least {min_prefix_length} characters long"),
                None,
            )));
        }

        let dust_nullifier_transactions = try_stream! {
            let nullifier_stream = storage.get_dust_nullifier_transactions(
                &prefixes,
                min_prefix_length,
                from_block,
                BATCH_SIZE
            );

            let mut nullifier_stream = pin!(nullifier_stream);
            while let Some(event) = nullifier_stream
                .try_next()
                .await
                .map_err_into_server_error(|| {
                    format!("get next dust nullifier transaction from block {from_block}")
                })?
            {
                yield event.into();
            }

            warn!("stream of dust nullifier transaction events completed unexpectedly");
        };

        Ok(dust_nullifier_transactions)
    }

    /// Stream DUST commitments with merkle tree updates, filtered by prefix.
    #[trace(properties = { "commitment_prefixes": "{commitment_prefixes:?}", "start_index": "{start_index}", "min_prefix_length": "{min_prefix_length:?}" })]
    async fn dust_commitments<'a>(
        &self,
        cx: &'a Context<'a>,
        commitment_prefixes: Vec<String>,
        start_index: i64,
        min_prefix_length: Option<i32>,
    ) -> Result<impl Stream<Item = ApiResult<DustCommitmentEventGraphQL>> + use<'a, S, B>, ApiError>
    {
        let storage = cx.get_storage::<S>();
        let min_prefix_length = min_prefix_length.unwrap_or(8) as usize;

        // Prevent DOS attacks by limiting database query complexity
        if commitment_prefixes.len() > 10 {
            return Err(ApiError::Client(InnerApiError(
                "Maximum 10 commitment prefixes allowed per request".to_owned(),
                None,
            )));
        }

        // Preserve privacy by ensuring sufficient anonymity set size
        if commitment_prefixes
            .iter()
            .any(|p| p.len() < min_prefix_length)
        {
            return Err(ApiError::Client(InnerApiError(
                format!(
                    "All commitment prefixes must be at least {min_prefix_length} characters long"
                ),
                None,
            )));
        }

        let dust_commitments = try_stream! {
            let commitment_stream = storage.get_dust_commitments(
                &commitment_prefixes,
                min_prefix_length,
                start_index,
                BATCH_SIZE
            );

            let mut commitment_stream = pin!(commitment_stream);
            while let Some(event) = commitment_stream
                .try_next()
                .await
                .map_err_into_server_error(|| {
                    format!("get next dust commitment from index {start_index}")
                })?
            {
                yield event.into();
            }

            warn!("stream of dust commitment events completed unexpectedly");
        };

        Ok(dust_commitments)
    }

    /// Stream registration changes for multiple address types.
    #[trace(properties = { "addresses": "{addresses:?}", "from_timestamp": "{from_timestamp:?}" })]
    async fn registration_updates<'a>(
        &self,
        cx: &'a Context<'a>,
        addresses: Vec<RegistrationAddress>,
        from_timestamp: Option<i64>,
    ) -> Result<
        impl Stream<Item = ApiResult<RegistrationUpdateEventGraphQL>> + use<'a, S, B>,
        ApiError,
    > {
        let storage = cx.get_storage::<S>();
        let from_timestamp = from_timestamp.unwrap_or(0);

        // Prevent DOS attacks by limiting database query complexity
        if addresses.len() > 100 {
            return Err(ApiError::Client(InnerApiError(
                "Maximum 100 addresses allowed per request".to_owned(),
                None,
            )));
        }

        let address_tuples: Vec<(indexer_common::domain::AddressType, String)> = addresses
            .into_iter()
            .map(|addr| (addr.address_type.into(), addr.value))
            .collect();

        let registration_updates = try_stream! {
            let registration_stream = storage.get_registration_updates(
                &address_tuples,
                from_timestamp,
                BATCH_SIZE
            );

            let mut registration_stream = pin!(registration_stream);
            while let Some(event) = registration_stream
                .try_next()
                .await
                .map_err_into_server_error(|| {
                    format!("get next registration update from timestamp {from_timestamp}")
                })?
            {
                yield event.into();
            }

            warn!("stream of registration update events completed unexpectedly");
        };

        Ok(registration_updates)
    }
}
