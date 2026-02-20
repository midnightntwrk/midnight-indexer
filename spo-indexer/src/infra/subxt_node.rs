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
        CandidateKeys, CandidateRegistration, Epoch, EpochCommitteeResponse, PoolMetadata,
        SPORegistrationResponse, SidechainStatusResponse, Validator,
    },
    utils::remove_hex_prefix,
};
use blockfrost::{BlockfrostAPI, BlockfrostError};
use http::header::USER_AGENT;
use indexer_common::error::BoxError;
use reqwest::Client as HttpClient;
use secrecy::{ExposeSecret, SecretString};
use serde_json::value::RawValue;
use std::collections::HashMap;
use subxt::{
    PolkadotConfig,
    backend::{
        legacy::LegacyRpcMethods,
        rpc::reconnecting_rpc_client::{ExponentialBackoff, HeaderMap, RpcClient},
    },
};
use thiserror::Error;

const SLOT_PER_EPOCH_KEY: &str = "3eaeb1cee77dc09baac326e5a1d29726f38178a5f54bee65a8446a55b585f261";
pub const SLOT_DURATION: u32 = 6000;

/// Config for node connection.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Config {
    pub url: String,

    pub blockfrost_id: SecretString,

    #[serde(with = "humantime_serde")]
    pub reconnect_max_delay: std::time::Duration,

    pub reconnect_max_attempts: usize,
}

/// A [Node] implementation based on subxt.
#[derive(Clone)]
pub struct SPOClient {
    pub epoch_duration: u32,
    pub slots_per_epoch: u32,

    rpc_client: RpcClient,
    blockfrost: BlockfrostAPI,
    http: HttpClient,
    blockfrost_id: SecretString,
}

// We will try to eliminate the 0x from any hex string out of this function.
impl SPOClient {
    /// Create a new [SPOClient] with the given [Config].
    pub async fn new(config: Config) -> Result<Self, SPOClientError> {
        let retry_policy = ExponentialBackoff::from_millis(10)
            .max_delay(config.reconnect_max_delay)
            .take(config.reconnect_max_attempts);
        let user_agent = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")).parse()?;
        let headers = HeaderMap::from_iter([(USER_AGENT, user_agent)]);
        let rpc_client = RpcClient::builder()
            .set_headers(headers)
            .retry_policy(retry_policy)
            .build(&config.url)
            .await
            .map_err(|error| SPOClientError::Subtx(error.into()))?;
        let blockfrost =
            BlockfrostAPI::new(config.blockfrost_id.expose_secret(), Default::default());
        let http = HttpClient::builder()
            .user_agent("midnight-spo-indexer/1.0")
            .build()
            .map_err(|error| SPOClientError::UnexpectedResponse(error.to_string()))?;
        let (epoch_duration, slots_per_epoch) = get_epoch_duration(&rpc_client).await?;

        Ok(Self {
            rpc_client,
            blockfrost,
            http,
            epoch_duration,
            slots_per_epoch,
            blockfrost_id: config.blockfrost_id,
        })
    }

    pub async fn get_sidechain_status(&self) -> Result<SidechainStatusResponse, SPOClientError> {
        let raw_response = self
            .rpc_client
            .request("sidechain_getStatus".to_owned(), None)
            .await
            .map_err(|error| {
                SPOClientError::RpcCall("sidechain_getStatus".to_owned(), error.to_string())
            })?;

        let response: SidechainStatusResponse = serde_json::from_str(raw_response.get())
            .map_err(|error| SPOClientError::UnexpectedResponse(error.to_string()))?;

        Ok(response)
    }

    pub async fn get_first_epoch_num(
        &self,
        storage: &impl crate::domain::storage::Storage,
    ) -> Result<u32, SPOClientError> {
        let current_epoch = self.get_current_epoch().await?;
        let block_timestamp = storage
            .get_block_timestamp(1)
            .await
            .map_err(|error| SPOClientError::UnexpectedResponse(error.to_string()))?
            .ok_or_else(|| {
                SPOClientError::UnexpectedResponse(
                    "block #1 timestamp not found in database".to_owned(),
                )
            })?;

        let num_epochs: u64 =
            (current_epoch.ends_at as u64 - block_timestamp as u64) / (self.epoch_duration as u64);

        Ok(current_epoch.epoch_no - num_epochs as u32)
    }

    pub async fn get_current_epoch(&self) -> Result<Epoch, SPOClientError> {
        let sidechain_status = self.get_sidechain_status().await?;
        let epoch = Epoch {
            epoch_no: sidechain_status.sidechain.epoch,
            starts_at: sidechain_status.sidechain.next_epoch_timestamp - self.epoch_duration as i64,
            ends_at: sidechain_status.sidechain.next_epoch_timestamp,
        };

        Ok(epoch)
    }

    pub async fn get_spo_registrations(
        &self,
        epoch_number: u32,
    ) -> Result<SPORegistrationResponse, SPOClientError> {
        let rpc_params = RawValue::from_string(format!("[{epoch_number}]")).map_err(|error| {
            SPOClientError::UnexpectedResponse(format!("failed to create RPC params: {error}"))
        })?;

        let raw_response = self
            .rpc_client
            .request(
                "systemParameters_getAriadneParameters".to_owned(),
                Some(rpc_params),
            )
            .await
            .map_err(|error| {
                SPOClientError::RpcCall(
                    "systemParameters_getAriadneParameters".to_owned(),
                    error.to_string(),
                )
            })?;

        let mut reg_response: SPORegistrationResponse = serde_json::from_str(raw_response.get())
            .map_err(|error| SPOClientError::UnexpectedResponse(error.to_string()))?;
        let mut response: HashMap<String, Vec<CandidateRegistration>> = HashMap::new();

        for (key, registrations) in reg_response.clone().candidate_registrations {
            let key = remove_hex_prefix(&key).to_owned();

            let cleaned_registrations = registrations
                .into_iter()
                .map(|reg| CandidateRegistration {
                    sidechain_pub_key: remove_hex_prefix(&reg.sidechain_pub_key).to_owned(),
                    sidechain_account_id: reg.sidechain_account_id,
                    mainchain_pub_key: remove_hex_prefix(&reg.mainchain_pub_key).to_owned(),
                    cross_chain_pub_key: remove_hex_prefix(&reg.cross_chain_pub_key).to_owned(),
                    keys: CandidateKeys {
                        aura: remove_hex_prefix(&reg.keys.aura).to_owned(),
                        gran: remove_hex_prefix(&reg.keys.gran).to_owned(),
                    },
                    sidechain_signature: remove_hex_prefix(&reg.sidechain_signature).to_owned(),
                    mainchain_signature: remove_hex_prefix(&reg.mainchain_signature).to_owned(),
                    cross_chain_signature: remove_hex_prefix(&reg.cross_chain_signature).to_owned(),

                    utxo: reg.utxo,
                    is_valid: reg.is_valid,
                    invalid_reasons: reg.invalid_reasons,
                })
                .collect::<Vec<_>>();

            response.insert(key, cleaned_registrations);
        }

        reg_response.candidate_registrations = response;

        Ok(reg_response)
    }

    pub async fn get_committee(&self, epoch_number: u32) -> Result<Vec<Validator>, SPOClientError> {
        let rpc_params = RawValue::from_string(format!("[{epoch_number}]")).map_err(|error| {
            SPOClientError::UnexpectedResponse(format!("failed to create RPC params: {error}"))
        })?;

        let raw_response = self
            .rpc_client
            .request("sidechain_getEpochCommittee".to_owned(), Some(rpc_params))
            .await
            .map_err(|error| {
                SPOClientError::RpcCall("sidechain_getEpochCommittee".to_owned(), error.to_string())
            });

        let Ok(raw_response) = raw_response else {
            return Ok(vec![]);
        };

        let response: EpochCommitteeResponse = serde_json::from_str(raw_response.get())
            .map_err(|error| SPOClientError::UnexpectedResponse(error.to_string()))?;

        let committee = response
            .committee
            .iter()
            .enumerate()
            .map(|(index, pk)| Validator {
                epoch_no: response.sidechain_epoch,
                position: index as u64,
                sidechain_pubkey: remove_hex_prefix(&pk.sidechain_pub_key).to_owned(),
            })
            .collect::<Vec<_>>();

        Ok(committee)
    }

    pub async fn get_pool_metadata(&self, pool_id: String) -> Result<PoolMetadata, SPOClientError> {
        let raw_meta = self.blockfrost.pools_metadata(&pool_id).await?;
        let meta = PoolMetadata {
            pool_id,
            hex_id: remove_hex_prefix(&raw_meta.hex).to_owned(),
            name: raw_meta.name.unwrap_or_default(),
            ticker: raw_meta.ticker.unwrap_or_default(),
            homepage_url: raw_meta.homepage.unwrap_or_default(),
            url: raw_meta.url.unwrap_or_default(),
        };

        Ok(meta)
    }

    /// Minimal pool stake data from Blockfrost /pools/{pool_id}.
    pub async fn get_pool_data(&self, pool_id: &str) -> Result<PoolStakeData, SPOClientError> {
        let base = self.blockfrost_base_url();
        let url = format!("{base}/pools/{pool_id}");
        let resp = self
            .http
            .get(&url)
            .header("project_id", self.blockfrost_id.expose_secret())
            .send()
            .await
            .map_err(|error| SPOClientError::UnexpectedResponse(error.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            let txt = resp.text().await.unwrap_or_default();
            return Err(SPOClientError::UnexpectedResponse(format!(
                "blockfrost GET /pools failed: {status} {txt}"
            )));
        }
        let v: serde_json::Value = resp
            .json()
            .await
            .map_err(|error| SPOClientError::UnexpectedResponse(error.to_string()))?;
        Ok(PoolStakeData::from_json(&v))
    }

    fn blockfrost_base_url(&self) -> &'static str {
        let id = self.blockfrost_id.expose_secret();
        if id.starts_with("mainnet") {
            "https://cardano-mainnet.blockfrost.io/api/v0"
        } else if id.starts_with("preprod") {
            "https://cardano-preprod.blockfrost.io/api/v0"
        } else if id.starts_with("preview") {
            "https://cardano-preview.blockfrost.io/api/v0"
        } else if id.starts_with("testnet") {
            "https://cardano-testnet.blockfrost.io/api/v0"
        } else {
            // Default to preview.
            "https://cardano-preview.blockfrost.io/api/v0"
        }
    }
}

#[derive(Debug, Clone)]
pub struct PoolStakeData {
    pub live_stake: Option<String>,
    pub active_stake: Option<String>,
    pub live_delegators: Option<i64>,
    pub live_saturation: Option<f64>,
    pub declared_pledge: Option<String>,
    pub live_pledge: Option<String>,
}

impl PoolStakeData {
    fn from_json(v: &serde_json::Value) -> Self {
        Self {
            live_stake: v
                .get("live_stake")
                .and_then(|x| x.as_str().map(|s| s.to_owned())),
            active_stake: v
                .get("active_stake")
                .and_then(|x| x.as_str().map(|s| s.to_owned())),
            live_delegators: v.get("live_delegators").and_then(|x| x.as_i64()),
            live_saturation: v.get("live_saturation").and_then(|x| x.as_f64()),
            declared_pledge: v
                .get("declared_pledge")
                .and_then(|x| x.as_str().map(|s| s.to_owned())),
            live_pledge: v
                .get("live_pledge")
                .and_then(|x| x.as_str().map(|s| s.to_owned())),
        }
    }
}

async fn get_epoch_duration(rpc_client: &RpcClient) -> Result<(u32, u32), SPOClientError> {
    let legacy_rpc = LegacyRpcMethods::<PolkadotConfig>::new(rpc_client.clone().into());
    let storage_key =
        hex::decode(SLOT_PER_EPOCH_KEY).expect("SLOT_PER_EPOCH_KEY constant should be valid hex");

    let res = legacy_rpc
        .state_get_storage(&storage_key, None)
        .await
        .map_err(|error| SPOClientError::Subtx(error.into()))?;
    let raw_bytes = res.ok_or_else(|| {
        SPOClientError::UnexpectedResponse("slots per epoch storage value not found".to_owned())
    })?;
    let raw_response: [u8; 4] = raw_bytes.try_into().map_err(|_| {
        SPOClientError::UnexpectedResponse("slots per epoch should be 4 bytes".to_owned())
    })?;
    let slots_per_epoch = u32::from_le_bytes(raw_response);

    Ok((SLOT_DURATION * slots_per_epoch, slots_per_epoch))
}

#[derive(Debug, Error)]
pub enum SPOClientError {
    #[error("cannot create reconnecting subxt RPC client")]
    Subtx(#[source] BoxError),

    #[error("cannot make rpc call {0}. Error: {1}")]
    RpcCall(String, String),

    #[error("api call error")]
    Blockfrost(#[from] BlockfrostError),

    #[error("unexpected error {0}")]
    UnexpectedResponse(String),

    #[error("cannot create HTTP header")]
    InvalidHeaderValue(#[from] InvalidHeaderValue),
}
