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

#[subxt::subxt(runtime_metadata_path = "../.node/0.13.0-alpha.1/metadata.scale")]
mod runtime_0_13 {}

use crate::infra::node::SubxtNodeError;
use indexer_common::domain::{
    BlockHash, ContractAddress, ContractState, PROTOCOL_VERSION_000_013_000, ProtocolVersion,
};
use itertools::Itertools;
use parity_scale_codec::Decode;
use subxt::{OnlineClient, SubstrateConfig, blocks::Extrinsics, events::Events, utils::H256};

pub type RuntimeUnshieldedUtxoInfo = crate::infra::node::runtimes::runtime_0_12::runtime_types::pallet_midnight::pallet::UnshieldedUtxoInfo;

/// Runtime specific block details.
pub struct BlockDetails {
    pub timestamp: Option<u64>,
    pub raw_transactions: Vec<Vec<u8>>,
    pub created_unshielded_utxos_info: HashMap<[u8; 32], Vec<RuntimeUnshieldedUtxoInfo>>,
    pub spent_unshielded_utxos_info: HashMap<[u8; 32], Vec<RuntimeUnshieldedUtxoInfo>>,
}

/// Make block details depending on the given protocol version.
pub async fn make_block_details(
    extrinsics: Extrinsics<SubstrateConfig, OnlineClient<SubstrateConfig>>,
    events: Events<SubstrateConfig>,
    authorities: &mut Option<Vec<[u8; 32]>>,
    protocol_version: ProtocolVersion,
) -> Result<BlockDetails, SubxtNodeError> {
    if protocol_version.is_compatible(PROTOCOL_VERSION_000_013_000) {
        make_block_details_runtime_0_13(extrinsics, events, authorities).await
    } else {
        Err(SubxtNodeError::InvalidProtocolVersion(protocol_version))
    }
}

/// Fetch authorities depending on the given protocol version.
pub async fn fetch_authorities(
    online_client: &OnlineClient<SubstrateConfig>,
    protocol_version: ProtocolVersion,
) -> Result<Option<Vec<[u8; 32]>>, SubxtNodeError> {
    if protocol_version.is_compatible(PROTOCOL_VERSION_000_013_000) {
        fetch_authorities_runtime_0_13(online_client).await
    } else {
        Err(SubxtNodeError::InvalidProtocolVersion(protocol_version))
    }
}

/// Decode slot depending on the given protocol version.
pub fn decode_slot(slot: &[u8], protocol_version: ProtocolVersion) -> Result<u64, SubxtNodeError> {
    if protocol_version.is_compatible(PROTOCOL_VERSION_000_013_000) {
        decode_slot_runtime_0_13(slot)
    } else {
        Err(SubxtNodeError::InvalidProtocolVersion(protocol_version))
    }
}

/// Get contract state depending on the given protocol version.
pub async fn get_contract_state(
    online_client: &OnlineClient<SubstrateConfig>,
    address: &ContractAddress,
    block_hash: BlockHash,
    protocol_version: ProtocolVersion,
) -> Result<ContractState, SubxtNodeError> {
    if protocol_version.is_compatible(PROTOCOL_VERSION_000_013_000) {
        get_contract_state_runtime_0_13(online_client, address, block_hash).await
    } else {
        Err(SubxtNodeError::InvalidProtocolVersion(protocol_version))
    }
}

pub async fn get_zswap_state_root(
    online_client: &OnlineClient<SubstrateConfig>,
    block_hash: BlockHash,
    protocol_version: ProtocolVersion,
) -> Result<Vec<u8>, SubxtNodeError> {
    if protocol_version.is_compatible(PROTOCOL_VERSION_000_013_000) {
        get_zswap_state_root_runtime_0_13(online_client, block_hash).await
    } else {
        Err(SubxtNodeError::InvalidProtocolVersion(protocol_version))
    }
}

macro_rules! make_block_details {
    ($module:ident) => {
        paste::paste! {
            async fn [<make_block_details_ $module>](
                extrinsics: Extrinsics<SubstrateConfig, OnlineClient<SubstrateConfig>>,
                events: Events<SubstrateConfig>,
                authorities: &mut Option<Vec<[u8; 32]>>,
            ) -> Result<BlockDetails, SubxtNodeError> {
                use self::$module::{
                    midnight,
                    runtime_types::pallet_midnight::pallet::UnshieldedEventType,
                    runtime_types::pallet_partner_chains_session::pallet as partner_chains_session,
                    timestamp, Call, Event,
                };

                let calls = extrinsics
                    .iter()
                    .map(|extrinsic| {
                        let call = extrinsic.as_root_extrinsic::<Call>().map_err(Box::new)?;
                        Ok(call)
                    })
                    .filter_ok(|call| matches!(call, Call::Midnight(_) | Call::Timestamp(_)))
                    .collect::<Result<Vec<_>, SubxtNodeError>>()?;

                let timestamp = calls.iter().find_map(|call| match call {
                    Call::Timestamp(timestamp::Call::set { now }) => Some(*now),
                    _ => None,
                });

                let raw_transactions = calls
                    .into_iter()
                    .filter_map(|call| match call {
                        Call::Midnight(midnight::Call::send_mn_transaction { midnight_tx }) => {
                            Some(midnight_tx)
                        }
                        _ => None,
                    })
                    .collect();

                let new_session = events
                let mut created_unshielded_utxos_info: HashMap<[u8; 32], Vec<RuntimeUnshieldedUtxoInfo>> = HashMap::new();
                let mut spent_unshielded_utxos_info: HashMap<[u8; 32], Vec<RuntimeUnshieldedUtxoInfo>> = HashMap::new();

                events
                    .iter()
                    .map(|event| event.and_then(|event| event.as_root_event::<Event>()))
                    .filter_map_ok(|event| match event {
                        Event::Session(partner_chains_session::Event::NewSession { .. }) => {
                            Some(())
                        }

                        _ => None,
                    })
                    .next()
                    .transpose()
                    .map_err(Box::new)?
                    .is_some();
                if new_session {
                    // Trigger fetching the authorities next time.
                    *authorities = None;
                }
                    .filter_map(|event_details_res| {
                        match event_details_res {
                            Ok(details) => match details.as_root_event::<Event>() {
                                Ok(root_event) => Some(Ok(root_event)),
                                Err(e) => Some(Err(SubxtNodeError::from(e)))
                            },
                            Err(e) => Some(Err(SubxtNodeError::from(e))),
                        }
                    })
                    .filter_map(Result::ok)
                    .for_each(|event| {
                        match event {
                            Event::Midnight(midnight::Event::TxApplied(details)) => {
                                apply_stages.insert(details.tx_hash, ApplyStage::Success);
                            }
                            Event::Midnight(midnight::Event::TxOnlyGuaranteedApplied(details)) => {
                                apply_stages.insert(details.tx_hash, ApplyStage::PartialSuccess);
                            }
                            Event::Midnight(midnight::Event::UnshieldedTokens(event_data)) => {
                                let is_created = matches!(event_data.event_type, UnshieldedEventType::Created);
                                if is_created {
                                    created_unshielded_utxos_info.insert(event_data.tx_hash, event_data.utxos);
                                } else {
                                    spent_unshielded_utxos_info.insert(event_data.tx_hash, event_data.utxos);
                                }
                            }
                            Event::Session(partner_chains_session::Event::NewSession { .. }) => {
                                *authorities = None;
                            }
                             _ => {}
                        }
                    });

                Ok(BlockDetails {
                    timestamp,
                    raw_transactions,
                    created_unshielded_utxos_info,
                    spent_unshielded_utxos_info,
                })
            }
        }
    };
}

make_block_details!(runtime_0_13);

macro_rules! fetch_authorities {
    ($module:ident) => {
        paste::paste! {
            async fn [<fetch_authorities_ $module>](
                online_client: &OnlineClient<SubstrateConfig>,
            ) -> Result<Option<Vec<[u8; 32]>>, SubxtNodeError> {
                let authorities = online_client
                    .storage()
                    .at_latest()
                    .await.map_err(Box::new)?
                    .fetch(&$module::storage().aura().authorities())
                    .await.map_err(Box::new)?
                    .map(|authorities| authorities.0.into_iter().map(|public| public.0).collect());

                Ok(authorities)
            }
        }
    };
}

fetch_authorities!(runtime_0_13);

macro_rules! decode_slot {
    ($module:ident) => {
        paste::paste! {
            fn [<decode_slot_ $module>](mut slot: &[u8]) -> Result<u64, SubxtNodeError> {
                let slot = $module::runtime_types::sp_consensus_slots::Slot::decode(&mut slot)
                    .map(|x| x.0)?;
                Ok(slot)
            }
        }
    };
}

decode_slot!(runtime_0_13);

macro_rules! get_contract_state {
    ($module:ident) => {
        paste::paste! {
            async fn [<get_contract_state_ $module>](
                online_client: &OnlineClient<SubstrateConfig>,
                address: &ContractAddress,
                block_hash: BlockHash,
            ) -> Result<ContractState, SubxtNodeError> {
                let get_state = $module::apis()
                    .midnight_runtime_api()
                    .get_contract_state(address.as_ref().to_owned());

                let state = online_client
                    .runtime_api()
                    .at(H256(block_hash.0))
                    .call(get_state)
                    .await.map_err(Box::new)?
                    .map_err(|error| SubxtNodeError::GetContractState(format!("{error:?}")))?
                    .into();

                Ok(state)
            }
        }
    };
}

get_contract_state!(runtime_0_13);

macro_rules! get_zswap_state_root {
    ($module:ident) => {
        paste::paste! {
            async fn [<get_zswap_state_root_ $module>](
                online_client: &OnlineClient<SubstrateConfig>,
                block_hash: BlockHash,
            ) -> Result<Vec<u8>, SubxtNodeError> {
                let get_zswap_state_root = $module::apis()
                    .midnight_runtime_api()
                    .get_zswap_state_root();

                let root = online_client
                    .runtime_api()
                    .at(H256(block_hash.0))
                    .call(get_zswap_state_root)
                    .await.map_err(Box::new)?
                    .map_err(|error| SubxtNodeError::GetZswapStateRoot(format!("{error:?}")))?;

                Ok(root)

            }
        }
    };
}

get_zswap_state_root!(runtime_0_13);

#[cfg(test)]
mod tests {
    use crate::infra::node::runtimes::get_contract_state;
    use anyhow::Context;
    use indexer_common::{
        domain::{BlockHash, ContractAddress, ProtocolVersion},
        error::BoxError,
    };
    use subxt::{
        OnlineClient, SubstrateConfig,
        backend::{BackendExt, legacy::LegacyRpcMethods, rpc::RpcClient},
        utils::H256,
    };

    #[tokio::test]
    #[ignore = "only to be run manually"]
    async fn test() -> Result<(), BoxError> {
        // wss://rpc.qanet.dev.midnight.network:443
        // wss://rpc.testnet-02.midnight.network:443
        let rpc_client = RpcClient::from_url("wss://rpc.qanet.dev.midnight.network:443").await?;
        let online_client =
            OnlineClient::<SubstrateConfig>::from_rpc_client(rpc_client.clone()).await?;

        let hash = "50457371e70aa856742ea94de650b00d58501629c290666eb3b561dc205c4541";
        let hash = BlockHash::try_from(const_hex::decode(hash).unwrap()).unwrap();

        let genesis_hash = online_client.genesis_hash();

        // Version must be greater or equal 15.
        let metadata = online_client
            .backend()
            .metadata_at_version(15, H256(hash.0))
            .await
            .context("get metadata")?;

        let legacy_rpc_methods = LegacyRpcMethods::<SubstrateConfig>::new(rpc_client.clone());
        let runtime_version = legacy_rpc_methods
            .state_get_runtime_version(Some(H256(hash.0)))
            .await
            .context("get runtime version")?;
        let runtime_version = subxt::client::RuntimeVersion {
            spec_version: runtime_version.spec_version,
            transaction_version: runtime_version.transaction_version,
        };

        let client = OnlineClient::<SubstrateConfig>::from_rpc_client_with(
            genesis_hash,
            runtime_version,
            metadata,
            rpc_client,
        )?;

        let address = "0102006e23ed3a34a8cf08ae9641aa12f86ff65c13e01c39e3057cff28723e9c86bba9";
        let address = ContractAddress::from(const_hex::decode(address).unwrap());

        let state = get_contract_state(&client, &address, hash, ProtocolVersion(8000)).await;
        assert!(state.is_ok());

        Ok(())
    }
}
