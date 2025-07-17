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

#[subxt::subxt(runtime_metadata_path = "../.node/0.13.2-rc.1/metadata.scale")]
mod runtime_0_13 {}

use crate::infra::subxt_node::SubxtNodeError;
use indexer_common::domain::{
    BlockHash, PROTOCOL_VERSION_000_013_000, ProtocolVersion, RawContractAddress, RawContractState,
    TransactionHash, UnshieldedUtxo,
};
use itertools::Itertools;
use parity_scale_codec::Decode;
use std::collections::HashMap;
use subxt::{OnlineClient, SubstrateConfig, blocks::Extrinsics, events::Events, utils::H256};

/// Runtime specific block details.
pub struct BlockDetails {
    pub timestamp: Option<u64>,
    pub raw_transactions: Vec<Vec<u8>>,
    pub created_unshielded_utxos_by_hash: HashMap<TransactionHash, Vec<UnshieldedUtxo>>,
    pub spent_unshielded_utxos_by_hash: HashMap<TransactionHash, Vec<UnshieldedUtxo>>,
}

/// Make block details depending on the given protocol version.
pub async fn make_block_details(
    extrinsics: Extrinsics<SubstrateConfig, OnlineClient<SubstrateConfig>>,
    events: Events<SubstrateConfig>,
    authorities: &mut Option<Vec<[u8; 32]>>,
    protocol_version: ProtocolVersion,
) -> Result<BlockDetails, SubxtNodeError> {
    // TODO Replace this often repeated pattern with a macro?
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
    address: RawContractAddress,
    block_hash: BlockHash,
    protocol_version: ProtocolVersion,
) -> Result<RawContractState, SubxtNodeError> {
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

/// Get transaction cost depending on the given protocol version.
pub async fn get_transaction_cost(
    online_client: &OnlineClient<SubstrateConfig>,
    raw_transaction: &[u8],
    block_hash: BlockHash,
    protocol_version: ProtocolVersion,
) -> Result<u128, SubxtNodeError> {
    if protocol_version.is_compatible(PROTOCOL_VERSION_000_013_000) {
        get_transaction_cost_runtime_0_13(online_client, raw_transaction, block_hash).await
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
                            Some(midnight_tx.into())
                        }

                        _ => None,
                    })
                    .collect();

                let mut created_unshielded_utxos_by_hash = HashMap::new();
                let mut spent_unshielded_utxos_by_hash = HashMap::new();

                let mut tx_hash = None;

                for event in events.iter().flatten() {
                    if let Ok(root_event) = event.as_root_event::<Event>() {
                        match root_event {
                            Event::Session(partner_chains_session::Event::NewSession { .. }) => {
                                *authorities = None;
                            }

                            Event::Midnight(midnight::Event::TxApplied(tx_applied)) => {
                                tx_hash = Some(tx_applied.tx_hash);
                            }

                            Event::Midnight(midnight::Event::TxPartialSuccess(tx_partial)) => {
                                tx_hash = Some(tx_partial.tx_hash);
                            }

                            Event::Midnight(midnight::Event::UnshieldedTokens(event_data)) => {
                                // Use transaction hash from preceding TxApplied/TxPartialSuccess
                                // events or fallback hash [0u8; 32] for system transactions (block
                                // rewards, minting) that create UTXOs without transaction context.
                                let transaction_hash = tx_hash.unwrap_or([0u8; 32]).into();

                                if !event_data.created.is_empty() {
                                    let created = event_data.created
                                        .into_iter()
                                        .map(|utxo| UnshieldedUtxo {
                                            value: utxo.value,
                                            owner: utxo.address.into(),
                                            token_type: utxo.token_type.into(),
                                            intent_hash: utxo.intent_hash.into(),
                                            output_index: utxo.output_no,
                                        })
                                        .collect();

                                    created_unshielded_utxos_by_hash.insert(
                                        transaction_hash,
                                        created
                                    );
                                }

                                if !event_data.spent.is_empty() {
                                    let spent = event_data.spent
                                        .into_iter()
                                        .map(|utxo| UnshieldedUtxo {
                                            value: utxo.value,
                                            owner: utxo.address.into(),
                                            token_type: utxo.token_type.into(),
                                            intent_hash: utxo.intent_hash.into(),
                                            output_index: utxo.output_no,
                                        })
                                        .collect();

                                    spent_unshielded_utxos_by_hash.insert(transaction_hash, spent);
                                }

                                // Reset to prevent stale hash in subsequent events.
                                tx_hash = None;
                            }

                            _ => {}
                        }
                    }
                }

                Ok(BlockDetails {
                    timestamp,
                    raw_transactions,
                    created_unshielded_utxos_by_hash,
                    spent_unshielded_utxos_by_hash,
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
                    .await
                    .map_err(Box::new)?
                    .fetch(&$module::storage().aura().authorities())
                    .await
                    .map_err(Box::new)?
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
                address: RawContractAddress,
                block_hash: BlockHash,
            ) -> Result<RawContractState, SubxtNodeError> {
                let get_state = $module::apis()
                    .midnight_runtime_api()
                    .get_contract_state(address.into());

                let state = online_client
                    .runtime_api()
                    .at(H256(block_hash.0))
                    .call(get_state)
                    .await
                    .map_err(Box::new)?
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
                    .await
                    .map_err(Box::new)?
                    .map_err(|error| SubxtNodeError::GetZswapStateRoot(format!("{error:?}")))?;

                Ok(root)

            }
        }
    };
}

get_zswap_state_root!(runtime_0_13);

macro_rules! get_transaction_cost {
    ($module:ident) => {
        paste::paste! {
            async fn [<get_transaction_cost_ $module>](
                online_client: &OnlineClient<SubstrateConfig>,
                raw_transaction: &[u8],
                block_hash: BlockHash,
            ) -> Result<u128, SubxtNodeError> {
                let get_transaction_cost = $module::apis()
                    .midnight_runtime_api()
                    .get_transaction_cost(raw_transaction.to_owned());

                let (storage_cost, gas_cost) = online_client
                    .runtime_api()
                    .at(H256(block_hash.0))
                    .call(get_transaction_cost)
                    .await
                    .map_err(Box::new)?
                    .map_err(|error| SubxtNodeError::GetTransactionCost(format!("{error:?}")))?;

                // Combine storage cost and gas cost for total fee
                // StorageCost = u128, GasCost = u64
                let total_cost = storage_cost.saturating_add(gas_cost as u128);
                Ok(total_cost)
            }
        }
    };
}

get_transaction_cost!(runtime_0_13);
