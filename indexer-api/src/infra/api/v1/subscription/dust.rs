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
        ApiError, ApiResult, ContextExt, InnerApiError, ResultExt,
        v1::dust::{
            AddressType, DustCommitmentEvent, DustGenerationEvent, DustNullifierTransactionEvent,
            RegistrationAddress, RegistrationUpdateEvent,
        },
    },
};
use async_graphql::{Context, InputObject, Subscription, async_stream::try_stream};
use futures::{Stream, StreamExt};
use std::{marker::PhantomData, num::NonZeroU32, pin::pin};

/// Registration address input for GraphQL.
#[derive(Debug, Clone, InputObject)]
pub struct RegistrationAddressInput {
    /// Type of address.
    #[graphql(name = "type")]
    pub address_type: AddressType,
    /// Address value.
    pub value: String,
}

impl From<RegistrationAddressInput> for RegistrationAddress {
    fn from(input: RegistrationAddressInput) -> Self {
        RegistrationAddress {
            address_type: input.address_type,
            value: input.value,
        }
    }
}

impl From<RegistrationAddressInput> for domain::dust::RegistrationAddress {
    fn from(input: RegistrationAddressInput) -> Self {
        Self {
            address_type: input.address_type.into(),
            value: input.value,
        }
    }
}

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
        dust_address: String,
        from_generation_index: Option<i32>,
        from_merkle_index: Option<i32>,
        only_active: Option<bool>,
    ) -> Result<impl Stream<Item = ApiResult<DustGenerationEvent>> + use<'a, S>, ApiError> {
        let storage = cx.get_storage::<S>();
        let batch_size = NonZeroU32::new(100).unwrap();

        // Default to 0 to start from the beginning of the generation history
        let from_generation_index = from_generation_index.unwrap_or(0);
        // Default to 0 to include all merkle tree updates from the start
        let from_merkle_index = from_merkle_index.unwrap_or(0);
        // Default to true to show only currently active (non-destroyed) generations
        let only_active = only_active.unwrap_or(true);

        let stream = try_stream! {
            let dust_stream = storage
                .get_dust_generations(
                    &dust_address,
                    from_generation_index as i64,
                    from_merkle_index as i64,
                    only_active,
                    batch_size,
                )
                .await
                .map_err_into_server_error(|| "start DUST generations stream")?;
            let mut dust_stream = pin!(dust_stream);

            while let Some(result) = dust_stream.next().await {
                let event = result
                    .map_err_into_server_error(|| "get next DUST generation event")?;
                yield event.into();
            }
        };

        Ok(stream)
    }

    /// Stream regular transactions containing DUST nullifiers.
    async fn dust_nullifier_transactions<'a>(
        &self,
        cx: &'a Context<'a>,
        prefixes: Vec<String>,
        min_prefix_length: i32,
        from_block: Option<i32>,
    ) -> Result<impl Stream<Item = ApiResult<DustNullifierTransactionEvent>> + use<'a, S>, ApiError>
    {
        // DOS protection: limit to 10 prefixes
        if prefixes.len() > 10 {
            return Err(ApiError::Client(InnerApiError(
                "Maximum 10 prefixes allowed per request".to_string(),
                None,
            )));
        }

        // Validate minimum prefix length
        if min_prefix_length < 8 {
            return Err(ApiError::Client(InnerApiError(
                "Minimum prefix length must be at least 8".to_string(),
                None,
            )));
        }

        let storage = cx.get_storage::<S>();
        let batch_size = NonZeroU32::new(100).unwrap();
        // Default to 0 to start from the genesis block
        let from_block = from_block.unwrap_or(0);

        let stream = try_stream! {
            let nullifier_stream = storage
                .get_dust_nullifier_transactions(&prefixes, min_prefix_length, from_block, batch_size)
                .await
                .map_err_into_server_error(|| "start DUST nullifier transactions stream")?;
            let mut nullifier_stream = pin!(nullifier_stream);

            while let Some(result) = nullifier_stream.next().await {
                let event = result
                    .map_err_into_server_error(|| "get next DUST nullifier transaction event")?;
                yield event.into();
            }
        };

        Ok(stream)
    }

    /// Stream DUST commitments with merkle tree updates, filtered by prefix.
    async fn dust_commitments<'a>(
        &self,
        cx: &'a Context<'a>,
        commitment_prefixes: Vec<String>,
        start_index: i32,
        min_prefix_length: i32,
    ) -> Result<impl Stream<Item = ApiResult<DustCommitmentEvent>> + use<'a, S>, ApiError> {
        // DOS protection: limit to 10 prefixes
        if commitment_prefixes.len() > 10 {
            return Err(ApiError::Client(InnerApiError(
                "Maximum 10 commitment prefixes allowed per request".to_string(),
                None,
            )));
        }

        // Validate minimum prefix length
        if min_prefix_length < 8 {
            return Err(ApiError::Client(InnerApiError(
                "Minimum prefix length must be at least 8".to_string(),
                None,
            )));
        }

        let storage = cx.get_storage::<S>();
        let batch_size = NonZeroU32::new(100).unwrap();

        let stream = try_stream! {
            let commitment_stream = storage
                .get_dust_commitments(&commitment_prefixes, start_index, min_prefix_length, batch_size)
                .await
                .map_err_into_server_error(|| "start DUST commitments stream")?;
            let mut commitment_stream = pin!(commitment_stream);

            while let Some(result) = commitment_stream.next().await {
                let event = result
                    .map_err_into_server_error(|| "get next DUST commitment event")?;
                yield event.into();
            }
        };

        Ok(stream)
    }

    /// Stream registration changes for multiple address types.
    async fn registration_updates<'a>(
        &self,
        cx: &'a Context<'a>,
        addresses: Vec<RegistrationAddressInput>,
    ) -> Result<impl Stream<Item = ApiResult<RegistrationUpdateEvent>> + use<'a, S>, ApiError> {
        // DOS protection: limit to 100 addresses
        if addresses.len() > 100 {
            return Err(ApiError::Client(InnerApiError(
                "Maximum 100 addresses allowed per request".to_string(),
                None,
            )));
        }

        let storage = cx.get_storage::<S>();
        let batch_size = NonZeroU32::new(100).unwrap();

        let addresses: Vec<domain::dust::RegistrationAddress> =
            addresses.into_iter().map(Into::into).collect();

        let stream = try_stream! {
            let registration_stream = storage
                .get_registration_updates(&addresses, batch_size)
                .await
                .map_err_into_server_error(|| "start registration updates stream")?;
            let mut registration_stream = pin!(registration_stream);

            while let Some(result) = registration_stream.next().await {
                let event = result
                    .map_err_into_server_error(|| "get next registration update event")?;
                yield event.into();
            }
        };

        Ok(stream)
    }
}
