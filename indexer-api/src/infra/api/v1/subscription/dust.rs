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
        ApiError, ApiResult, ContextExt, OptionExt, ResultExt,
        v1::{
            HexEncoded,
            dust::{
                DustCommitmentEvent, DustCommitmentProgress, DustGenerationEvent,
                DustGenerationInfo, DustGenerationProgress, DustNullifierTransactionEvent,
                DustNullifierTransactionProgress, RegistrationAddress, RegistrationUpdateEvent,
                RegistrationUpdateProgress,
            },
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

        // Merge the streams.
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
        Some(())
            .filter(|_| min_prefix_length >= 8)
            .ok_or_client_error(|| "minimum prefix length must be at least 8")?;

        // Convert hex prefixes to binary.
        let binary_prefixes = prefixes
            .into_iter()
            .map(|p| p.hex_decode::<DustPrefix>())
            .collect::<Result<Vec<_>, _>>()
            .map_err_into_client_error(|| "invalid dust prefix")?;

        // Default to 0 to start from the genesis block.
        let from_block = from_block.unwrap_or(0);

        // Build a stream by merging transactions and progress updates.
        let (trigger, tripwire) = Tripwire::new();

        let nullifier_transactions = make_dust_nullifier_transactions::<S>(
            cx,
            binary_prefixes.clone(),
            min_prefix_length,
            from_block,
            trigger,
        )
        .map_ok(|tx| DustNullifierTransactionEvent::Transaction(tx.into()));

        let progress_updates =
            make_nullifier_progress_updates::<S>(cx, binary_prefixes, from_block)
                .take_until_if(tripwire)
                .map_ok(DustNullifierTransactionEvent::Progress);

        let events = tokio_stream::StreamExt::merge(nullifier_transactions, progress_updates);

        Ok(events)
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
        Some(())
            .filter(|_| min_prefix_length >= 8)
            .ok_or_client_error(|| "minimum prefix length must be at least 8")?;

        // Convert hex prefixes to binary.
        let binary_prefixes = commitment_prefixes
            .into_iter()
            .map(|p| p.hex_decode::<indexer_common::domain::DustPrefix>())
            .collect::<Result<Vec<_>, _>>()
            .map_err_into_client_error(|| "invalid hex prefix")?;

        // Build a stream by merging commitments and progress updates.
        let (trigger, tripwire) = Tripwire::new();

        let commitments = make_dust_commitments::<S>(
            cx,
            binary_prefixes.clone(),
            start_index,
            min_prefix_length,
            trigger,
        )
        .map_ok(|c| match c {
            domain::dust::DustCommitmentEvent::Commitment(commitment) => {
                DustCommitmentEvent::Commitment(commitment.into())
            }
            domain::dust::DustCommitmentEvent::MerkleUpdate(update) => {
                DustCommitmentEvent::MerkleUpdate(update.into())
            }
        });

        let progress_updates =
            make_commitment_progress_updates::<S>(cx, binary_prefixes.clone(), start_index)
                .take_until_if(tripwire)
                .map_ok(DustCommitmentEvent::Progress);

        let events = tokio_stream::StreamExt::merge(commitments, progress_updates);

        Ok(events)
    }

    /// Stream registration changes for multiple address types.
    async fn dust_registration_updates<'a>(
        &self,
        cx: &'a Context<'a>,
        addresses: Vec<RegistrationAddress>,
    ) -> Result<impl Stream<Item = ApiResult<RegistrationUpdateEvent>> + use<'a, S>, ApiError> {
        // Convert API types to domain types.
        let addresses = addresses
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<domain::dust::RegistrationAddress>, _>>()
            .map_err_into_client_error(|| "invalid address")?;

        // Build a stream by merging updates and progress.
        let (trigger, tripwire) = Tripwire::new();

        let registration_updates = make_registration_updates::<S>(cx, addresses.clone(), trigger)
            .map_ok(|update| RegistrationUpdateEvent::Update(update.into()));

        let progress_updates = make_registration_progress_updates::<S>(cx, addresses)
            .take_until_if(tripwire)
            .map_ok(RegistrationUpdateEvent::Progress);

        let events = tokio_stream::StreamExt::merge(registration_updates, progress_updates);

        Ok(events)
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
                    // Skip merkle updates in this stream - they're part of Info events now.
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
    // Get highest generation index for this address.
    let highest_index = storage
        .get_highest_generation_index_for_dust_address(&dust_address)
        .await
        .map_err_into_server_error(|| "get highest generation index")?
        .unwrap_or(0);

    // Get count of active generations.
    let active_generation_count = storage
        .get_active_generation_count_for_dust_address(&dust_address)
        .await
        .map_err_into_server_error(|| "get active generation count")?;

    Ok(DustGenerationProgress {
        highest_index,
        active_generation_count,
    })
}

fn make_dust_nullifier_transactions<'a, S>(
    cx: &'a Context<'a>,
    prefixes: Vec<DustPrefix>,
    min_prefix_length: u32,
    from_block: u32,
    trigger: Trigger,
) -> impl Stream<Item = ApiResult<domain::dust::DustNullifierTransaction>> + use<'a, S>
where
    S: Storage,
{
    let storage = cx.get_storage::<S>();

    try_stream! {
        let nullifier_stream = storage
            .get_dust_nullifier_transactions(&prefixes, min_prefix_length, from_block, BATCH_SIZE);
        let mut nullifier_stream = pin!(nullifier_stream);

        while let Some(event) = nullifier_stream
            .try_next()
            .await
            .map_err_into_server_error(|| "get next DUST nullifier transaction event")?
        {
            match event {
                domain::dust::DustNullifierTransactionEvent::Transaction(tx) => yield tx,
            }
        }
    }
    .on_drop(move || drop(trigger))
}

fn make_nullifier_progress_updates<'a, S>(
    cx: &'a Context<'a>,
    prefixes: Vec<DustPrefix>,
    from_block: u32,
) -> impl Stream<Item = ApiResult<DustNullifierTransactionProgress>> + use<'a, S>
where
    S: Storage,
{
    let storage = cx.get_storage::<S>();
    let intervals = IntervalStream::new(interval(PROGRESS_UPDATES_INTERVAL));

    // Get real progress updates from storage.
    intervals.then(move |_| {
        let storage = storage.clone();
        let prefixes = prefixes.clone();
        async move {
            let (highest_block, matched_count) = storage
                .get_dust_nullifier_progress(&prefixes, 8, from_block)
                .await
                .map_err_into_server_error(|| "get nullifier progress")?;

            Ok(DustNullifierTransactionProgress {
                highest_block,
                matched_count,
            })
        }
    })
}

fn make_dust_commitments<'a, S>(
    cx: &'a Context<'a>,
    prefixes: Vec<DustPrefix>,
    start_index: u64,
    min_prefix_length: u32,
    trigger: Trigger,
) -> impl Stream<Item = ApiResult<domain::dust::DustCommitmentEvent>> + use<'a, S>
where
    S: Storage,
{
    let storage = cx.get_storage::<S>();

    try_stream! {
        let commitment_stream = storage
            .get_dust_commitments(&prefixes, start_index, min_prefix_length, BATCH_SIZE);
        let mut commitment_stream = pin!(commitment_stream);

        while let Some(event) = commitment_stream
            .try_next()
            .await
            .map_err_into_server_error(|| "get next DUST commitment event")?
        {
            yield event;
        }
    }
    .on_drop(move || drop(trigger))
}

fn make_commitment_progress_updates<'a, S>(
    cx: &'a Context<'a>,
    commitment_prefixes: Vec<DustPrefix>,
    start_index: u64,
) -> impl Stream<Item = ApiResult<DustCommitmentProgress>> + use<'a, S>
where
    S: Storage,
{
    let storage = cx.get_storage::<S>();
    let intervals = IntervalStream::new(interval(PROGRESS_UPDATES_INTERVAL));

    // Get real progress updates from storage.
    intervals.then(move |_| {
        let storage = storage.clone();
        let prefixes = commitment_prefixes.clone();
        async move {
            let (highest_index, commitment_count) = storage
                .get_dust_commitment_progress(&prefixes, 8, start_index)
                .await
                .map_err_into_server_error(|| "get commitment progress")?;

            Ok(DustCommitmentProgress {
                highest_index,
                commitment_count,
            })
        }
    })
}

fn make_registration_updates<'a, S>(
    cx: &'a Context<'a>,
    addresses: Vec<domain::dust::RegistrationAddress>,
    trigger: Trigger,
) -> impl Stream<Item = ApiResult<domain::dust::RegistrationUpdate>> + use<'a, S>
where
    S: Storage,
{
    let storage = cx.get_storage::<S>();

    try_stream! {
        let registration_updates = storage
            .get_registration_updates(&addresses, BATCH_SIZE);
        let mut registration_updates = pin!(registration_updates);

        while let Some(registration_update) = registration_updates
            .try_next()
            .await
            .map_err_into_server_error(|| "get next registration update event")?
        {
            yield registration_update;
        }
    }
    .on_drop(move || drop(trigger))
}

fn make_registration_progress_updates<'a, S>(
    cx: &'a Context<'a>,
    addresses: Vec<domain::dust::RegistrationAddress>,
) -> impl Stream<Item = ApiResult<RegistrationUpdateProgress>> + use<'a, S>
where
    S: Storage,
{
    let storage = cx.get_storage::<S>();
    let intervals = IntervalStream::new(interval(PROGRESS_UPDATES_INTERVAL));

    // Get real progress updates from storage.
    intervals.then(move |_| {
        let storage = storage.clone();
        let addresses = addresses.clone();
        async move {
            let (latest_timestamp, update_count) = storage
                .get_registration_progress(&addresses)
                .await
                .map_err_into_server_error(|| "get registration progress")?;

            Ok(RegistrationUpdateProgress {
                latest_timestamp,
                update_count,
            })
        }
    })
}
