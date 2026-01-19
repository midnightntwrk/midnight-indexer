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
    domain::{
        spo::{
            CommitteeMember as DomainCommitteeMember, EpochInfo as DomainEpochInfo,
            EpochPerf as DomainEpochPerf, FirstValidEpoch as DomainFirstValidEpoch,
            PoolMetadata as DomainPoolMetadata, PresenceEvent as DomainPresenceEvent,
            RegisteredStat as DomainRegisteredStat, RegisteredTotals as DomainRegisteredTotals,
            Spo as DomainSpo, SpoComposite as DomainSpoComposite, SpoIdentity as DomainSpoIdentity,
            StakeShare as DomainStakeShare,
        },
        storage::Storage,
    },
    infra::api::{ApiResult, ContextExt, ResultExt},
};
use async_graphql::{Context, Object, SimpleObject};
use fastrace::trace;
use std::marker::PhantomData;

const DEFAULT_PERFORMANCE_LIMIT: i64 = 20;

/// SPO identity information.
#[derive(SimpleObject)]
#[graphql(rename_fields = "camelCase")]
pub struct SpoIdentity {
    pub pool_id_hex: String,
    pub mainchain_pubkey_hex: String,
    pub sidechain_pubkey_hex: String,
    pub aura_pubkey_hex: Option<String>,
    pub validator_class: String,
}

impl From<DomainSpoIdentity> for SpoIdentity {
    fn from(d: DomainSpoIdentity) -> Self {
        Self {
            pool_id_hex: d.pool_id_hex,
            mainchain_pubkey_hex: d.mainchain_pubkey_hex,
            sidechain_pubkey_hex: d.sidechain_pubkey_hex,
            aura_pubkey_hex: d.aura_pubkey_hex,
            validator_class: d.validator_class,
        }
    }
}

/// Pool metadata from Cardano.
#[derive(SimpleObject)]
#[graphql(rename_fields = "camelCase")]
pub struct PoolMetadata {
    pub pool_id_hex: String,
    pub hex_id: Option<String>,
    pub name: Option<String>,
    pub ticker: Option<String>,
    pub homepage_url: Option<String>,
    pub logo_url: Option<String>,
}

impl From<DomainPoolMetadata> for PoolMetadata {
    fn from(d: DomainPoolMetadata) -> Self {
        Self {
            pool_id_hex: d.pool_id_hex,
            hex_id: d.hex_id,
            name: d.name,
            ticker: d.ticker,
            homepage_url: d.homepage_url,
            logo_url: d.logo_url,
        }
    }
}

/// SPO with optional metadata.
#[derive(SimpleObject)]
#[graphql(rename_fields = "camelCase")]
pub struct Spo {
    pub pool_id_hex: String,
    pub validator_class: String,
    pub sidechain_pubkey_hex: String,
    pub aura_pubkey_hex: Option<String>,
    pub name: Option<String>,
    pub ticker: Option<String>,
    pub homepage_url: Option<String>,
    pub logo_url: Option<String>,
}

impl From<DomainSpo> for Spo {
    fn from(d: DomainSpo) -> Self {
        Self {
            pool_id_hex: d.pool_id_hex,
            validator_class: d.validator_class,
            sidechain_pubkey_hex: d.sidechain_pubkey_hex,
            aura_pubkey_hex: d.aura_pubkey_hex,
            name: d.name,
            ticker: d.ticker,
            homepage_url: d.homepage_url,
            logo_url: d.logo_url,
        }
    }
}

/// Composite SPO data (identity + metadata + performance).
#[derive(SimpleObject)]
#[graphql(rename_fields = "camelCase")]
pub struct SpoComposite {
    pub identity: Option<SpoIdentity>,
    pub metadata: Option<PoolMetadata>,
    pub performance: Vec<EpochPerf>,
}

impl From<DomainSpoComposite> for SpoComposite {
    fn from(d: DomainSpoComposite) -> Self {
        Self {
            identity: d.identity.map(Into::into),
            metadata: d.metadata.map(Into::into),
            performance: d.performance.into_iter().map(Into::into).collect(),
        }
    }
}

/// SPO performance for an epoch.
#[derive(SimpleObject)]
#[graphql(rename_fields = "camelCase")]
pub struct EpochPerf {
    pub epoch_no: i64,
    pub spo_sk_hex: String,
    pub produced: i64,
    pub expected: i64,
    pub identity_label: Option<String>,
    pub stake_snapshot: Option<String>,
    pub pool_id_hex: Option<String>,
    pub validator_class: Option<String>,
}

impl From<DomainEpochPerf> for EpochPerf {
    fn from(d: DomainEpochPerf) -> Self {
        Self {
            epoch_no: d.epoch_no,
            spo_sk_hex: d.spo_sk_hex,
            produced: d.produced,
            expected: d.expected,
            identity_label: d.identity_label,
            stake_snapshot: d.stake_snapshot,
            pool_id_hex: d.pool_id_hex,
            validator_class: d.validator_class,
        }
    }
}

/// Current epoch information.
#[derive(SimpleObject)]
#[graphql(rename_fields = "camelCase")]
pub struct EpochInfo {
    pub epoch_no: i64,
    pub duration_seconds: i64,
    pub elapsed_seconds: i64,
}

impl From<DomainEpochInfo> for EpochInfo {
    fn from(d: DomainEpochInfo) -> Self {
        Self {
            epoch_no: d.epoch_no,
            duration_seconds: d.duration_seconds,
            elapsed_seconds: d.elapsed_seconds,
        }
    }
}

/// Committee member for an epoch.
#[derive(SimpleObject)]
#[graphql(rename_fields = "camelCase")]
pub struct CommitteeMember {
    pub epoch_no: i64,
    pub position: i32,
    pub sidechain_pubkey_hex: String,
    pub expected_slots: i32,
    pub aura_pubkey_hex: Option<String>,
    pub pool_id_hex: Option<String>,
    pub spo_sk_hex: Option<String>,
}

impl From<DomainCommitteeMember> for CommitteeMember {
    fn from(d: DomainCommitteeMember) -> Self {
        Self {
            epoch_no: d.epoch_no,
            position: d.position,
            sidechain_pubkey_hex: d.sidechain_pubkey_hex,
            expected_slots: d.expected_slots,
            aura_pubkey_hex: d.aura_pubkey_hex,
            pool_id_hex: d.pool_id_hex,
            spo_sk_hex: d.spo_sk_hex,
        }
    }
}

/// Registration statistics for an epoch.
#[derive(SimpleObject)]
#[graphql(rename_fields = "camelCase")]
pub struct RegisteredStat {
    pub epoch_no: i64,
    pub federated_valid_count: i64,
    pub federated_invalid_count: i64,
    pub registered_valid_count: i64,
    pub registered_invalid_count: i64,
    pub dparam: Option<f64>,
}

impl From<DomainRegisteredStat> for RegisteredStat {
    fn from(d: DomainRegisteredStat) -> Self {
        Self {
            epoch_no: d.epoch_no,
            federated_valid_count: d.federated_valid_count,
            federated_invalid_count: d.federated_invalid_count,
            registered_valid_count: d.registered_valid_count,
            registered_invalid_count: d.registered_invalid_count,
            dparam: d.dparam,
        }
    }
}

/// Cumulative registration totals for an epoch.
#[derive(SimpleObject)]
#[graphql(rename_fields = "camelCase")]
pub struct RegisteredTotals {
    pub epoch_no: i64,
    pub total_registered: i64,
    pub newly_registered: i64,
}

impl From<DomainRegisteredTotals> for RegisteredTotals {
    fn from(d: DomainRegisteredTotals) -> Self {
        Self {
            epoch_no: d.epoch_no,
            total_registered: d.total_registered,
            newly_registered: d.newly_registered,
        }
    }
}

/// Presence event for an SPO in an epoch.
#[derive(SimpleObject)]
#[graphql(rename_fields = "camelCase")]
pub struct PresenceEvent {
    pub epoch_no: i64,
    pub id_key: String,
    pub source: String,
    pub status: Option<String>,
}

impl From<DomainPresenceEvent> for PresenceEvent {
    fn from(d: DomainPresenceEvent) -> Self {
        Self {
            epoch_no: d.epoch_no,
            id_key: d.id_key,
            source: d.source,
            status: d.status,
        }
    }
}

/// First valid epoch for an SPO identity.
#[derive(SimpleObject)]
#[graphql(rename_fields = "camelCase")]
pub struct FirstValidEpoch {
    pub id_key: String,
    pub first_valid_epoch: i64,
}

impl From<DomainFirstValidEpoch> for FirstValidEpoch {
    fn from(d: DomainFirstValidEpoch) -> Self {
        Self {
            id_key: d.id_key,
            first_valid_epoch: d.first_valid_epoch,
        }
    }
}

/// Stake share information for an SPO.
#[derive(SimpleObject)]
#[graphql(rename_fields = "camelCase")]
pub struct StakeShare {
    pub pool_id_hex: String,
    pub name: Option<String>,
    pub ticker: Option<String>,
    pub homepage_url: Option<String>,
    pub logo_url: Option<String>,
    pub live_stake: Option<String>,
    pub active_stake: Option<String>,
    pub live_delegators: Option<i64>,
    pub live_saturation: Option<f64>,
    pub declared_pledge: Option<String>,
    pub live_pledge: Option<String>,
    pub stake_share: Option<f64>,
}

impl From<DomainStakeShare> for StakeShare {
    fn from(d: DomainStakeShare) -> Self {
        Self {
            pool_id_hex: d.pool_id_hex,
            name: d.name,
            ticker: d.ticker,
            homepage_url: d.homepage_url,
            logo_url: d.logo_url,
            live_stake: d.live_stake,
            active_stake: d.active_stake,
            live_delegators: d.live_delegators,
            live_saturation: d.live_saturation,
            declared_pledge: d.declared_pledge,
            live_pledge: d.live_pledge,
            stake_share: d.stake_share,
        }
    }
}

/// SPO GraphQL queries.
pub struct SpoQuery<S> {
    _s: PhantomData<S>,
}

impl<S> Default for SpoQuery<S> {
    fn default() -> Self {
        Self { _s: PhantomData }
    }
}

#[Object]
impl<S> SpoQuery<S>
where
    S: Storage,
{
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
