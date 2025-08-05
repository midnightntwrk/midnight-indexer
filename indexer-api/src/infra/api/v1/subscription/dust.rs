// This file is part of midnight-indexer.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::{
    domain::{self, storage::Storage},
    infra::api::{
        ApiError, ApiResult, ContextExt, HexEncoded, InnerApiError, ResultExt,
        v1::dust::{
            DustCommitmentEvent, DustGenerationEvent, DustGenerationInfo, DustGenerationProgress,
            DustNullifierTransactionEvent, RegistrationAddress, RegistrationUpdateEvent,
        },
    },
};
use async_graphql::{Context, Subscription, async_stream::try_stream};
use drop_stream::DropStreamExt;
use futures::{Stream, StreamExt, TryStreamExt};
use indexer_common::domain::{DustAddress, DustPrefix};
use std::{marker::PhantomData, num::NonZeroU32, pin::pin, time::Duration};
use stream_cancel::{StreamExt as _, Trigger, Tripwire};
use tokio::time::interval;
use tokio_stream::wrappers::IntervalStream;

// TODO: Make configurable!
const BATCH_SIZE: NonZeroU32 = NonZeroU32::new(100).unwrap();

// TODO: Make configurable!
const PROGRESS_UPDATES_INTERVAL: Duration = Duration::from_secs(30);

/// DUST GraphQL subscriptions.
pub struct DustSubscription<S> {
    _s: PhantomData<S>,
}

impl<S> Default for DustSubscription<S> {
    fn default() -> Self {
        Self { _s: PhantomData }
    }
}

#[Subscription]
impl<S> DustSubscription<S>
where
    S: Storage,
{
    /// Stream generation info with merkle updates for wallet reconstruction.
    async fn dust_generations<'a>(
        &self,
        cx: &'a Context<'a>,
        dust_address: HexEncoded,
        from_generation_index: Option<u64>,
        from_merkle_index: Option<u64>,
        only_active: Option<bool>,
    ) -> Result<impl Stream<Item = ApiResult<DustGenerationEvent>> + use<'a, S>, ApiError> {
        // Decode the DUST address.
        let dust_address = dust_address
            .hex_decode()
            .map_err_into_client_error(|| "invalid address")?;

        // Default to true to show only currently active (non-destroyed) generations.
        let only_active = only_active.unwrap_or(true);

        // Build a stream of dust generation events by merging dust_generations and
        // progress_updates. The dust_generations stream should be infinite by definition.
        // However, if it nevertheless completes, we use a Tripwire to ensure
        // the progress_updates stream also completes, preventing the merged stream from
        // hanging indefinitely waiting for both streams to complete.
        let (trigger, tripwire) = Tripwire::new();

        let dust_generations = make_dust_generations::<S>(
            cx,
            dust_address,
            from_generation_index.unwrap_or(0),
            from_merkle_index.unwrap_or(0),
            only_active,
            trigger,
        )
        .map_ok(DustGenerationEvent::Info);

        let progress_updates = make_progress_updates::<S>(cx, dust_address)
            .take_until_if(tripwire)
            .map_ok(DustGenerationEvent::Progress);

        // Merge the streams
        let events = tokio_stream::StreamExt::merge(dust_generations, progress_updates);

        Ok(events)
    }

    /// Stream regular transactions containing DUST nullifiers.
    async fn dust_nullifier_transactions<'a>(
        &self,
        cx: &'a Context<'a>,
        prefixes: Vec<HexEncoded>,
        min_prefix_length: u32,
        from_block: Option<u32>,
    ) -> Result<impl Stream<Item = ApiResult<DustNullifierTransactionEvent>> + use<'a, S>, ApiError>
    {
        // Validate minimum prefix length.
        if min_prefix_length < 8 {
            return Err(ApiError::Client(InnerApiError(
                "minimum prefix length must be at least 8".to_string(),
                None,
            )));
        }

        let storage = cx.get_storage::<S>();

        // Convert hex prefixes to binary.
        let binary_prefixes = prefixes
            .into_iter()
            .map(|p| p.hex_decode::<DustPrefix>())
            .collect::<Result<Vec<_>, _>>()
            .map_err_into_client_error(|| "invalid dust prefix")?;

        // Default to 0 to start from the genesis block.
        let from_block = from_block.unwrap_or(0);

        let stream = try_stream! {
            let nullifier_stream = storage
                .get_dust_nullifier_transactions(&binary_prefixes, min_prefix_length, from_block, BATCH_SIZE);
            let mut nullifier_stream = pin!(nullifier_stream);

            while let Some(event) = nullifier_stream
                .try_next()
                .await
                .map_err_into_server_error(|| "get next DUST nullifier transaction event")?
            {
                yield event.into();
            }
        };

        Ok(stream)
    }

    /// Stream DUST commitments with merkle tree updates, filtered by prefix.
    async fn dust_commitments<'a>(
        &self,
        cx: &'a Context<'a>,
        commitment_prefixes: Vec<HexEncoded>,
        start_index: u64,
        min_prefix_length: u32,
    ) -> Result<impl Stream<Item = ApiResult<DustCommitmentEvent>> + use<'a, S>, ApiError> {
        // Validate minimum prefix length.
        if min_prefix_length < 8 {
            return Err(ApiError::Client(InnerApiError(
                "Minimum prefix length must be at least 8".to_string(),
                None,
            )));
        }

        let storage = cx.get_storage::<S>();

        // Convert hex prefixes to binary.
        let binary_prefixes = commitment_prefixes
            .into_iter()
            .map(|p| p.hex_decode::<indexer_common::domain::DustPrefix>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| {
                ApiError::Client(InnerApiError(format!("Invalid hex prefix: {e}"), None))
            })?;

        let stream = try_stream! {
            let commitment_stream = storage
                .get_dust_commitments(&binary_prefixes, start_index, min_prefix_length, BATCH_SIZE)
                .await
                .map_err_into_server_error(|| "start DUST commitments stream")?;
            let mut commitment_stream = pin!(commitment_stream);

            while let Some(event) = commitment_stream
                .try_next()
                .await
                .map_err_into_server_error(|| "get next DUST commitment event")?
            {
                yield event.into();
            }
        };

        Ok(stream)
    }

    /// Stream registration changes for multiple address types.
    async fn registration_updates<'a>(
        &self,
        cx: &'a Context<'a>,
        addresses: Vec<RegistrationAddress>,
    ) -> Result<impl Stream<Item = ApiResult<RegistrationUpdateEvent>> + use<'a, S>, ApiError> {
        let storage = cx.get_storage::<S>();

        // Convert API types to domain types.
        let addresses = addresses
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<domain::dust::RegistrationAddress>, _>>()
            .map_err(|e| ApiError::Client(InnerApiError(format!("Invalid address: {e}"), None)))?;

        let stream = try_stream! {
            let registration_stream = storage
                .get_registration_updates(&addresses, BATCH_SIZE)
                .await
                .map_err_into_server_error(|| "start registration updates stream")?;
            let mut registration_stream = pin!(registration_stream);

            while let Some(event) = registration_stream
                .try_next()
                .await
                .map_err_into_server_error(|| "get next registration update event")?
            {
                yield event.into();
            }
        };

        Ok(stream)
    }
}

fn make_dust_generations<'a, S>(
    cx: &'a Context<'a>,
    dust_address: DustAddress,
    from_generation_index: u64,
    from_merkle_index: u64,
    only_active: bool,
    trigger: Trigger,
) -> impl Stream<Item = ApiResult<DustGenerationInfo>> + use<'a, S>
where
    S: Storage,
{
    let storage = cx.get_storage::<S>();

    try_stream! {
        let dust_generations = storage
            .get_dust_generations(
                &dust_address,
                from_generation_index,
                from_merkle_index,
                only_active,
                BATCH_SIZE,
            );
        let mut dust_generations = pin!(dust_generations);

        while let Some(event) = dust_generations
            .try_next()
            .await
            .map_err_into_server_error(|| "get next DUST generation event")?
        {
            match event {
                domain::dust::DustGenerationEvent::Info(info) => yield info.into(),
                domain::dust::DustGenerationEvent::MerkleUpdate(_) => {
                    // Skip merkle updates in this stream - they're part of Info events now
                },
                domain::dust::DustGenerationEvent::Progress(_) => {
                    // Skip progress - handled separately
                },
            }
        }
    }
    .on_drop(move || drop(trigger))
}

fn make_progress_updates<'a, S>(
    cx: &'a Context<'a>,
    dust_address: DustAddress,
) -> impl Stream<Item = ApiResult<DustGenerationProgress>> + use<'a, S>
where
    S: Storage,
{
    let intervals = IntervalStream::new(interval(PROGRESS_UPDATES_INTERVAL));
    intervals
        .then(move |_| make_dust_generation_progress_update(dust_address, cx.get_storage::<S>()))
}

async fn make_dust_generation_progress_update<S>(
    dust_address: DustAddress,
    storage: &S,
) -> ApiResult<DustGenerationProgress>
where
    S: Storage,
{
    // Get highest generation index for this address
    let highest_index = storage
        .get_highest_generation_index_for_dust_address(&dust_address)
        .await
        .map_err_into_server_error(|| "get highest generation index")?
        .unwrap_or(0);

    // Get count of active generations
    let active_generation_count = storage
        .get_active_generation_count_for_dust_address(&dust_address)
        .await
        .map_err_into_server_error(|| "get active generation count")?;

    Ok(DustGenerationProgress {
        highest_index,
        active_generation_count,
    })
}
