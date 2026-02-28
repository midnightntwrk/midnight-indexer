// This file is part of midnight-indexer.
// Copyright (C) 2025-2026 Midnight Foundation
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
        ApiResult, ContextExt, OptionExt, ResultExt,
        v4::{
            CardanoNetworkId, CardanoRewardAddress, HexEncoded,
            block::{Block, BlockOffset},
            contract_action::{ContractAction, ContractActionOffset},
            dust::DustGenerationStatus,
            spo::{
                CommitteeMember, EpochInfo, EpochPerf, FirstValidEpoch, PoolMetadata,
                PresenceEvent, RegisteredStat, RegisteredTotals, Spo, SpoComposite, SpoIdentity,
                StakeShare,
            },
            system_parameters::{DParameterChange, TermsAndConditionsChange},
            transaction::{Transaction, TransactionOffset},
        },
    },
};
use async_graphql::{Context, Object};
use fastrace::trace;
use indexer_common::domain::LedgerVersion;
use std::marker::PhantomData;

const DEFAULT_PERFORMANCE_LIMIT: i64 = 20;

/// GraphQL queries.
pub struct Query<S> {
    _s: PhantomData<S>,
}

impl<S> Default for Query<S> {
    fn default() -> Self {
        Self { _s: PhantomData }
    }
}

#[Object]
impl<S> Query<S>
where
    S: Storage,
{
    /// Find a block for the given optional offset; if not present, the latest block is returned.
    #[trace(properties = { "offset": "{offset:?}" })]
    pub async fn block(
        &self,
        cx: &Context<'_>,
        offset: Option<BlockOffset>,
    ) -> ApiResult<Option<Block<S>>> {
        let storage = cx.get_storage::<S>();

        let block = match offset {
            Some(BlockOffset::Hash(hash)) => {
                let hash = hash
                    .hex_decode()
                    .map_err_into_client_error(|| "invalid block hash")?;

                storage
                    .get_block_by_hash(hash)
                    .await
                    .map_err_into_server_error(|| format!("get block by hash {hash}"))?
            }

            Some(BlockOffset::Height(height)) => storage
                .get_block_by_height(height)
                .await
                .map_err_into_server_error(|| format!("get block by height {height}"))?,

            None => storage
                .get_latest_block()
                .await
                .map_err_into_server_error(|| "get latest block")?,
        };

        Ok(block.map(Into::into))
    }

    /// Find transactions for the given offset.
    #[trace(properties = { "offset": "{offset:?}" })]
    async fn transactions(
        &self,
        cx: &Context<'_>,
        offset: TransactionOffset,
    ) -> ApiResult<Vec<Transaction<S>>> {
        let storage = cx.get_storage::<S>();

        match offset {
            TransactionOffset::Hash(hash) => {
                let hash = hash
                    .hex_decode()
                    .map_err_into_client_error(|| "invalid transaction hash")?;

                let transactions = storage
                    .get_transactions_by_hash(hash)
                    .await
                    .map_err_into_server_error(|| format!("get transactions by hash {hash}"))?
                    .into_iter()
                    .map(Into::into)
                    .collect::<Vec<_>>();

                Ok(transactions)
            }

            TransactionOffset::Identifier(identifier) => {
                let identifier = identifier
                    .hex_decode()
                    .map_err_into_client_error(|| "invalid transaction identifier")?;

                let transactions = storage
                    .get_transactions_by_identifier(&identifier)
                    .await
                    .map_err_into_server_error(|| {
                        format!("get transactions by identifier {identifier}")
                    })?
                    .into_iter()
                    .map(Into::into)
                    .collect::<Vec<_>>();

                Ok(transactions)
            }
        }
    }

    /// Find a contract action for the given address and optional offset.
    #[trace(properties = { "address": "{address}", "offset": "{offset:?}" })]
    async fn contract_action(
        &self,
        cx: &Context<'_>,
        address: HexEncoded,
        offset: Option<ContractActionOffset>,
    ) -> ApiResult<Option<ContractAction<S>>> {
        let storage = cx.get_storage::<S>();

        let address = &address
            .hex_decode()
            .map_err_into_client_error(|| "invalid address")?;

        let contract_action = match offset {
            Some(ContractActionOffset::BlockOffset(BlockOffset::Hash(hash))) => {
                let hash = hash
                    .hex_decode()
                    .map_err_into_client_error(|| "invalid offset")?;

                storage
                    .get_contract_action_by_address_and_block_hash(address, hash)
                    .await
                    .map_err_into_server_error(|| {
                        format!("get contract action by address {address} and block hash {hash}")
                    })?
            }

            Some(ContractActionOffset::BlockOffset(BlockOffset::Height(height))) => storage
                .get_contract_action_by_address_and_block_height(address, height)
                .await
                .map_err_into_server_error(|| {
                    format!("get contract action by address {address} and block height {height}")
                })?,

            Some(ContractActionOffset::TransactionOffset(TransactionOffset::Hash(hash))) => {
                let hash = hash
                    .hex_decode()
                    .map_err_into_client_error(|| "invalid offset")?;

                storage
                    .get_contract_action_by_address_and_transaction_hash(address, hash)
                    .await
                    .map_err_into_server_error(|| {
                        format!(
                            "get contract action by address {address} and transaction hash {hash}"
                        )
                    })?
            }

            Some(ContractActionOffset::TransactionOffset(TransactionOffset::Identifier(
                identifier,
            ))) => {
                let identifier = identifier
                    .hex_decode()
                    .map_err_into_client_error(|| "invalid identifier")?;

                storage
                    .get_contract_action_by_address_and_transaction_identifier(
                        address,
                        &identifier,
                    )
                    .await
                    .map_err_into_server_error(|| format!("get contract action by address {address} and transaction identifier {identifier}"))?
            }

            None => storage
                .get_latest_contract_action_by_address(address)
                .await
                .map_err_into_server_error(|| {
                    format!("get latest contract action by address {address}")
                })?,
        };

        Ok(contract_action.map(Into::into))
    }

    /// Get DUST generation status for specific Cardano reward addresses.
    #[trace]
    async fn dust_generation_status(
        &self,
        cx: &Context<'_>,
        cardano_reward_addresses: Vec<CardanoRewardAddress>,
    ) -> ApiResult<Vec<DustGenerationStatus>> {
        // DOS protection: limit to 10 reward addresses.
        (cardano_reward_addresses.len() <= 10)
            .then_some(())
            .some_or_client_error(|| "maximum of ten reward addresses allowed")?;

        let storage = cx.get_storage::<S>();
        let network_id = cx.get_network_id();
        let expected_cardano_network = CardanoNetworkId::from(network_id);

        // Convert Bech32 CardanoRewardAddress to binary, validating network.
        let address = cardano_reward_addresses
            .into_iter()
            .map(|key| key.decode_for_network(expected_cardano_network))
            .collect::<Result<Vec<_>, _>>()
            .map_err_into_client_error(|| "invalid Cardano reward address")?;

        let status_list = storage
            .get_dust_generation_status(&address, LedgerVersion::LATEST)
            .await
            .map_err_into_server_error(|| "get DUST generation status")?;

        Ok(status_list
            .into_iter()
            .map(|s| (s, network_id).into())
            .collect())
    }

    /// Get the full history of D-parameter changes for governance auditability.
    #[trace]
    async fn d_parameter_history(&self, cx: &Context<'_>) -> ApiResult<Vec<DParameterChange>> {
        let storage = cx.get_storage::<S>();

        let history = storage
            .get_d_parameter_history()
            .await
            .map_err_into_server_error(|| "get D-parameter history")?;

        Ok(history.into_iter().map(DParameterChange::from).collect())
    }

    /// Get the full history of Terms and Conditions changes for governance auditability.
    #[trace]
    async fn terms_and_conditions_history(
        &self,
        cx: &Context<'_>,
    ) -> ApiResult<Vec<TermsAndConditionsChange>> {
        let storage = cx.get_storage::<S>();

        let history = storage
            .get_terms_and_conditions_history()
            .await
            .map_err_into_server_error(|| "get Terms and Conditions history")?;

        Ok(history
            .into_iter()
            .map(TermsAndConditionsChange::from)
            .collect())
    }

    /// List SPO identities with pagination.
    #[trace]
    async fn spo_identities(
        &self,
        cx: &Context<'_>,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> ApiResult<Vec<SpoIdentity>> {
        let storage = cx.get_storage::<S>();
        let limit = limit.unwrap_or(50).clamp(1, 500) as i64;
        let offset = offset.unwrap_or(0).max(0) as i64;

        let identities = storage
            .get_spo_identities(limit, offset)
            .await
            .map_err_into_server_error(|| "get SPO identities")?;

        Ok(identities.into_iter().map(Into::into).collect())
    }

    /// Get SPO identity by pool ID.
    #[trace]
    async fn spo_identity_by_pool_id(
        &self,
        cx: &Context<'_>,
        pool_id_hex: String,
    ) -> ApiResult<Option<SpoIdentity>> {
        let pool_id = normalize_hex(&pool_id_hex);
        let storage = cx.get_storage::<S>();

        let identity = storage
            .get_spo_identity_by_pool_id(&pool_id)
            .await
            .map_err_into_server_error(|| "get SPO identity by pool ID")?;

        Ok(identity.map(Into::into))
    }

    /// Get total count of SPOs.
    #[trace]
    async fn spo_count(&self, cx: &Context<'_>) -> ApiResult<Option<i64>> {
        let storage = cx.get_storage::<S>();

        let count = storage
            .get_spo_count()
            .await
            .map_err_into_server_error(|| "get SPO count")?;

        Ok(Some(count))
    }

    /// Get pool metadata by pool ID.
    #[trace]
    async fn pool_metadata(
        &self,
        cx: &Context<'_>,
        pool_id_hex: String,
    ) -> ApiResult<Option<PoolMetadata>> {
        let pool_id = normalize_hex(&pool_id_hex);
        let storage = cx.get_storage::<S>();

        let metadata = storage
            .get_pool_metadata(&pool_id)
            .await
            .map_err_into_server_error(|| "get pool metadata")?;

        Ok(metadata.map(Into::into))
    }

    /// List pool metadata with pagination.
    #[trace]
    async fn pool_metadata_list(
        &self,
        cx: &Context<'_>,
        limit: Option<i32>,
        offset: Option<i32>,
        with_name_only: Option<bool>,
    ) -> ApiResult<Vec<PoolMetadata>> {
        let storage = cx.get_storage::<S>();
        let limit = limit.unwrap_or(50).clamp(1, 500) as i64;
        let offset = offset.unwrap_or(0).max(0) as i64;
        let with_name_only = with_name_only.unwrap_or(false);

        let metadata = storage
            .get_pool_metadata_list(limit, offset, with_name_only)
            .await
            .map_err_into_server_error(|| "get pool metadata list")?;

        Ok(metadata.into_iter().map(Into::into).collect())
    }

    /// Get SPO with metadata by pool ID.
    #[trace]
    async fn spo_by_pool_id(
        &self,
        cx: &Context<'_>,
        pool_id_hex: String,
    ) -> ApiResult<Option<Spo>> {
        let pool_id = normalize_hex(&pool_id_hex);
        let storage = cx.get_storage::<S>();

        let spo = storage
            .get_spo_by_pool_id(&pool_id)
            .await
            .map_err_into_server_error(|| "get SPO by pool ID")?;

        Ok(spo.map(Into::into))
    }

    /// List SPOs with optional search.
    #[trace]
    async fn spo_list(
        &self,
        cx: &Context<'_>,
        limit: Option<i32>,
        offset: Option<i32>,
        search: Option<String>,
    ) -> ApiResult<Vec<Spo>> {
        let storage = cx.get_storage::<S>();
        let limit = limit.unwrap_or(20).clamp(1, 200) as i64;
        let offset = offset.unwrap_or(0).max(0) as i64;
        let search_ref = search.as_deref().and_then(|s| {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });

        let spos = storage
            .get_spo_list(limit, offset, search_ref)
            .await
            .map_err_into_server_error(|| "get SPO list")?;

        Ok(spos.into_iter().map(Into::into).collect())
    }

    /// Get composite SPO data (identity + metadata + performance).
    #[trace]
    async fn spo_composite_by_pool_id(
        &self,
        cx: &Context<'_>,
        pool_id_hex: String,
    ) -> ApiResult<Option<SpoComposite>> {
        let pool_id = normalize_hex(&pool_id_hex);
        let storage = cx.get_storage::<S>();

        let composite = storage
            .get_spo_composite_by_pool_id(&pool_id, DEFAULT_PERFORMANCE_LIMIT)
            .await
            .map_err_into_server_error(|| "get SPO composite by pool ID")?;

        Ok(composite.map(Into::into))
    }

    /// Get SPO identifiers ordered by performance.
    #[trace]
    async fn stake_pool_operators(
        &self,
        cx: &Context<'_>,
        limit: Option<i32>,
    ) -> ApiResult<Vec<String>> {
        let storage = cx.get_storage::<S>();
        let limit = limit.unwrap_or(20).clamp(1, 100) as i64;

        let ids = storage
            .get_stake_pool_operator_ids(limit)
            .await
            .map_err_into_server_error(|| "get stake pool operators")?;

        Ok(ids)
    }

    /// Get latest SPO performance entries.
    #[trace]
    async fn spo_performance_latest(
        &self,
        cx: &Context<'_>,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> ApiResult<Vec<EpochPerf>> {
        let storage = cx.get_storage::<S>();
        let limit = limit
            .unwrap_or(DEFAULT_PERFORMANCE_LIMIT as i32)
            .clamp(1, 500) as i64;
        let offset = offset.unwrap_or(0).max(0) as i64;

        let perfs = storage
            .get_spo_performance_latest(limit, offset)
            .await
            .map_err_into_server_error(|| "get SPO performance latest")?;

        Ok(perfs.into_iter().map(Into::into).collect())
    }

    /// Get SPO performance by SPO key.
    #[trace]
    async fn spo_performance_by_spo_sk(
        &self,
        cx: &Context<'_>,
        spo_sk_hex: String,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> ApiResult<Vec<EpochPerf>> {
        let spo_sk = normalize_hex(&spo_sk_hex);
        let storage = cx.get_storage::<S>();
        let limit = limit.unwrap_or(100).clamp(1, 500) as i64;
        let offset = offset.unwrap_or(0).max(0) as i64;

        let perfs = storage
            .get_spo_performance_by_spo_sk(&spo_sk, limit, offset)
            .await
            .map_err_into_server_error(|| "get SPO performance by SPO key")?;

        Ok(perfs.into_iter().map(Into::into).collect())
    }

    /// Get epoch performance for all SPOs.
    #[trace]
    async fn epoch_performance(
        &self,
        cx: &Context<'_>,
        epoch: i64,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> ApiResult<Vec<EpochPerf>> {
        let storage = cx.get_storage::<S>();
        let limit = limit.unwrap_or(100).clamp(1, 500) as i64;
        let offset = offset.unwrap_or(0).max(0) as i64;

        let perfs = storage
            .get_epoch_performance(epoch, limit, offset)
            .await
            .map_err_into_server_error(|| "get epoch performance")?;

        Ok(perfs.into_iter().map(Into::into).collect())
    }

    /// Get current epoch information.
    #[trace]
    async fn current_epoch_info(&self, cx: &Context<'_>) -> ApiResult<Option<EpochInfo>> {
        let storage = cx.get_storage::<S>();

        let info = storage
            .get_current_epoch_info()
            .await
            .map_err_into_server_error(|| "get current epoch info")?;

        Ok(info.map(Into::into))
    }

    /// Get epoch utilization (produced/expected ratio).
    #[trace]
    async fn epoch_utilization(&self, cx: &Context<'_>, epoch: i32) -> ApiResult<Option<f64>> {
        let storage = cx.get_storage::<S>();

        let utilization = storage
            .get_epoch_utilization(epoch as i64)
            .await
            .map_err_into_server_error(|| "get epoch utilization")?;

        Ok(utilization)
    }

    /// Get committee membership for an epoch.
    #[trace]
    async fn committee(&self, cx: &Context<'_>, epoch: i64) -> ApiResult<Vec<CommitteeMember>> {
        let storage = cx.get_storage::<S>();

        let members = storage
            .get_committee(epoch)
            .await
            .map_err_into_server_error(|| "get committee")?;

        Ok(members.into_iter().map(Into::into).collect())
    }

    /// Get cumulative registration totals for an epoch range.
    #[trace]
    async fn registered_totals_series(
        &self,
        cx: &Context<'_>,
        from_epoch: i64,
        to_epoch: i64,
    ) -> ApiResult<Vec<RegisteredTotals>> {
        let storage = cx.get_storage::<S>();

        let totals = storage
            .get_registered_totals_series(from_epoch, to_epoch)
            .await
            .map_err_into_server_error(|| "get registered totals series")?;

        Ok(totals.into_iter().map(Into::into).collect())
    }

    /// Get registration statistics for an epoch range.
    #[trace]
    async fn registered_spo_series(
        &self,
        cx: &Context<'_>,
        from_epoch: i64,
        to_epoch: i64,
    ) -> ApiResult<Vec<RegisteredStat>> {
        let storage = cx.get_storage::<S>();

        let stats = storage
            .get_registered_spo_series(from_epoch, to_epoch)
            .await
            .map_err_into_server_error(|| "get registered SPO series")?;

        Ok(stats.into_iter().map(Into::into).collect())
    }

    /// Get raw presence events for an epoch range.
    #[trace]
    async fn registered_presence(
        &self,
        cx: &Context<'_>,
        from_epoch: i64,
        to_epoch: i64,
    ) -> ApiResult<Vec<PresenceEvent>> {
        let storage = cx.get_storage::<S>();

        let events = storage
            .get_registered_presence(from_epoch, to_epoch)
            .await
            .map_err_into_server_error(|| "get registered presence")?;

        Ok(events.into_iter().map(Into::into).collect())
    }

    /// Get first valid epoch for each SPO identity.
    #[trace]
    async fn registered_first_valid_epochs(
        &self,
        cx: &Context<'_>,
        upto_epoch: Option<i64>,
    ) -> ApiResult<Vec<FirstValidEpoch>> {
        let storage = cx.get_storage::<S>();

        let epochs = storage
            .get_registered_first_valid_epochs(upto_epoch)
            .await
            .map_err_into_server_error(|| "get registered first valid epochs")?;

        Ok(epochs.into_iter().map(Into::into).collect())
    }

    /// Get stake distribution with search and ordering.
    #[trace]
    async fn stake_distribution(
        &self,
        cx: &Context<'_>,
        limit: Option<i32>,
        offset: Option<i32>,
        search: Option<String>,
        order_by_stake_desc: Option<bool>,
    ) -> ApiResult<Vec<StakeShare>> {
        let storage = cx.get_storage::<S>();
        let limit = limit.unwrap_or(50).clamp(1, 500) as i64;
        let offset = offset.unwrap_or(0).max(0) as i64;
        let search_ref = search.as_deref().and_then(|s| {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });
        let order_desc = order_by_stake_desc.unwrap_or(true);

        let (shares, _total) = storage
            .get_stake_distribution(limit, offset, search_ref, order_desc)
            .await
            .map_err_into_server_error(|| "get stake distribution")?;

        Ok(shares.into_iter().map(Into::into).collect())
    }
}

/// Normalize hex string by stripping 0x prefix and lowercasing.
fn normalize_hex(input: &str) -> String {
    let s = input
        .strip_prefix("0x")
        .unwrap_or(input)
        .strip_prefix("0X")
        .unwrap_or(input);
    s.to_ascii_lowercase()
}
