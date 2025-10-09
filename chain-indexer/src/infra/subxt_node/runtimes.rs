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

// To see how this is generated, look in build.rs
include!(concat!(env!("OUT_DIR"), "/generated_runtime.rs"));

use crate::{domain::DustRegistrationEvent, infra::subxt_node::SubxtNodeError};
use indexer_common::domain::{
    BlockHash, ByteVec, CardanoStakeKey, DustAddress, DustUtxoId, PROTOCOL_VERSION_000_017_000,
    ProtocolVersion, SerializedContractAddress, SerializedContractState,
};
use itertools::Itertools;
use parity_scale_codec::Decode;
use subxt::{OnlineClient, SubstrateConfig, blocks::Extrinsics, events::Events, utils::H256};

/// Runtime specific block details.
pub struct BlockDetails {
    pub timestamp: Option<u64>,
    pub transactions: Vec<Transaction>,
    pub dust_registration_events: Vec<DustRegistrationEvent>,
}

/// Runtime specific (serialized) transaction.
pub enum Transaction {
    Regular(ByteVec),
    System(ByteVec),
}

/// Make block details depending on the given protocol version.
pub async fn make_block_details(
    extrinsics: Extrinsics<SubstrateConfig, OnlineClient<SubstrateConfig>>,
    events: Events<SubstrateConfig>,
    authorities: &mut Option<Vec<[u8; 32]>>,
    protocol_version: ProtocolVersion,
) -> Result<BlockDetails, SubxtNodeError> {
    // TODO Replace this often repeated pattern with a macro?
    if protocol_version.is_compatible(PROTOCOL_VERSION_000_017_000) {
        make_block_details_runtime_0_17(extrinsics, events, authorities).await
    } else {
        Err(SubxtNodeError::InvalidProtocolVersion(protocol_version))
    }
}

/// Fetch authorities depending on the given protocol version.
pub async fn fetch_authorities(
    block_hash: BlockHash,
    protocol_version: ProtocolVersion,
    online_client: &OnlineClient<SubstrateConfig>,
) -> Result<Option<Vec<[u8; 32]>>, SubxtNodeError> {
    if protocol_version.is_compatible(PROTOCOL_VERSION_000_017_000) {
        fetch_authorities_runtime_0_17(block_hash, online_client).await
    } else {
        Err(SubxtNodeError::InvalidProtocolVersion(protocol_version))
    }
}

/// Decode slot depending on the given protocol version.
pub fn decode_slot(slot: &[u8], protocol_version: ProtocolVersion) -> Result<u64, SubxtNodeError> {
    if protocol_version.is_compatible(PROTOCOL_VERSION_000_017_000) {
        decode_slot_runtime_0_17(slot)
    } else {
        Err(SubxtNodeError::InvalidProtocolVersion(protocol_version))
    }
}

/// Get contract state depending on the given protocol version.
pub async fn get_contract_state(
    address: SerializedContractAddress,
    block_hash: BlockHash,
    protocol_version: ProtocolVersion,
    online_client: &OnlineClient<SubstrateConfig>,
) -> Result<SerializedContractState, SubxtNodeError> {
    if protocol_version.is_compatible(PROTOCOL_VERSION_000_017_000) {
        get_contract_state_runtime_0_17(address, block_hash, online_client).await
    } else {
        Err(SubxtNodeError::InvalidProtocolVersion(protocol_version))
    }
}

pub async fn get_zswap_state_root(
    block_hash: BlockHash,
    protocol_version: ProtocolVersion,
    online_client: &OnlineClient<SubstrateConfig>,
) -> Result<Vec<u8>, SubxtNodeError> {
    if protocol_version.is_compatible(PROTOCOL_VERSION_000_017_000) {
        get_zswap_state_root_runtime_0_17(block_hash, online_client).await
    } else {
        Err(SubxtNodeError::InvalidProtocolVersion(protocol_version))
    }
}

/// Get cost for the given serialized transaction depending on the given protocol version.
pub async fn get_transaction_cost(
    transaction: impl AsRef<[u8]>,
    block_hash: BlockHash,
    protocol_version: ProtocolVersion,
    online_client: &OnlineClient<SubstrateConfig>,
) -> Result<u128, SubxtNodeError> {
    if protocol_version.is_compatible(PROTOCOL_VERSION_000_017_000) {
        get_transaction_cost_runtime_0_17(transaction.as_ref(), block_hash, online_client).await
    } else {
        Err(SubxtNodeError::InvalidProtocolVersion(protocol_version))
    }
}

async fn make_block_details_runtime_0_17(
    extrinsics: Extrinsics<SubstrateConfig, OnlineClient<SubstrateConfig>>,
    events: Events<SubstrateConfig>,
    authorities: &mut Option<Vec<[u8; 32]>>,
) -> Result<BlockDetails, SubxtNodeError> {
    use self::runtime_0_17::{
        Call, Event,
        runtime_types::{
            pallet_midnight::pallet::Call::send_mn_transaction,
            pallet_midnight_system::pallet::{
                Call::send_mn_system_transaction, Event::SystemTransactionApplied,
            },
            pallet_native_token_observation::pallet::Event as NativeTokenObservationEvent,
            pallet_partner_chains_session::pallet::Event::NewSession,
        },
        timestamp,
    };

    let calls = extrinsics
        .iter()
        .map(|extrinsic| {
            let call = extrinsic
                .as_root_extrinsic::<Call>()
                .map_err(|error| SubxtNodeError::AsRootExtrinsic(error.into()))?;
            Ok(call)
        })
        .filter_ok(|call| {
            matches!(
                call,
                Call::Timestamp(_) | Call::Midnight(_) | Call::MidnightSystem(_)
            )
        })
        .collect::<Result<Vec<_>, SubxtNodeError>>()?;

    let timestamp = calls.iter().find_map(|call| match call {
        Call::Timestamp(timestamp::Call::set { now }) => Some(*now),
        _ => None,
    });

    let mut transactions = calls
        .into_iter()
        .filter_map(|call| match call {
            Call::Midnight(send_mn_transaction { midnight_tx }) => {
                Some(Transaction::Regular(midnight_tx.into()))
            }

            Call::MidnightSystem(send_mn_system_transaction { midnight_system_tx }) => {
                Some(Transaction::System(midnight_system_tx.into()))
            }

            _ => None,
        })
        .collect::<Vec<_>>();

    let mut dust_registration_events = vec![];

    for event_details in events.iter() {
        let event_details =
            event_details.map_err(|error| SubxtNodeError::GetNextEvent(error.into()))?;

        let event = event_details
            .as_root_event::<Event>()
            .map_err(|error| SubxtNodeError::AsRootEvent(error.into()))?;

        match event {
            Event::Session(NewSession { .. }) => {
                *authorities = None;
            }

            // System transaction created by the node (not from extrinsics).
            Event::MidnightSystem(SystemTransactionApplied(transaction_applied)) => {
                transactions.push(Transaction::System(ByteVec::from(
                    transaction_applied.serialized_system_transaction,
                )));
            }

            // DUST registration events from NativeTokenObservation pallet.
            Event::NativeTokenObservation(native_token_event) => match native_token_event {
                NativeTokenObservationEvent::Registration(event) => {
                    let cardano_address = CardanoStakeKey::from(event.cardano_address.0);
                    let dust_address_array: [u8; 32] = event
                        .dust_address
                        .try_into()
                        .map_err(|_| SubxtNodeError::InvalidDustAddress)?;

                    dust_registration_events.push(DustRegistrationEvent::Registration {
                        cardano_address,
                        dust_address: DustAddress::from(dust_address_array),
                    });
                }

                NativeTokenObservationEvent::Deregistration(event) => {
                    let cardano_address = CardanoStakeKey::from(event.cardano_address.0);
                    let dust_address_array: [u8; 32] = event
                        .dust_address
                        .try_into()
                        .map_err(|_| SubxtNodeError::InvalidDustAddress)?;

                    dust_registration_events.push(DustRegistrationEvent::Deregistration {
                        cardano_address,
                        dust_address: DustAddress::from(dust_address_array),
                    });
                }

                NativeTokenObservationEvent::MappingAdded(event) => {
                    let cardano_address = CardanoStakeKey::from(event.cardano_address.0);
                    let dust_address_bytes = const_hex::decode(&event.dust_address)
                        .map_err(|_| SubxtNodeError::InvalidDustAddress)?;
                    let utxo_id_bytes = const_hex::decode(&event.utxo_id)
                        .map_err(|_| SubxtNodeError::InvalidDustAddress)?;
                    let dust_address_array: [u8; 32] = dust_address_bytes
                        .try_into()
                        .map_err(|_| SubxtNodeError::InvalidDustAddress)?;

                    dust_registration_events.push(DustRegistrationEvent::MappingAdded {
                        cardano_address,
                        dust_address: DustAddress::from(dust_address_array),
                        utxo_id: DustUtxoId::from(utxo_id_bytes),
                    });
                }

                NativeTokenObservationEvent::MappingRemoved(event) => {
                    let cardano_address = CardanoStakeKey::from(event.cardano_address.0);
                    let dust_address_bytes = const_hex::decode(&event.dust_address)
                        .map_err(|_| SubxtNodeError::InvalidDustAddress)?;
                    let utxo_id_bytes = const_hex::decode(&event.utxo_id)
                        .map_err(|_| SubxtNodeError::InvalidDustAddress)?;
                    let dust_address_array: [u8; 32] = dust_address_bytes
                        .try_into()
                        .map_err(|_| SubxtNodeError::InvalidDustAddress)?;

                    dust_registration_events.push(DustRegistrationEvent::MappingRemoved {
                        cardano_address,
                        dust_address: DustAddress::from(dust_address_array),
                        utxo_id: DustUtxoId::from(utxo_id_bytes),
                    });
                }

                _ => {}
            },

            _ => {}
        }
    }

    Ok(BlockDetails {
        timestamp,
        transactions,
        dust_registration_events,
    })
}

async fn fetch_authorities_runtime_0_17(
    block_hash: BlockHash,
    online_client: &OnlineClient<SubstrateConfig>,
) -> Result<Option<Vec<[u8; 32]>>, SubxtNodeError> {
    let authorities = online_client
        .storage()
        .at(H256(block_hash.0))
        .fetch(&runtime_0_17::storage().aura().authorities())
        .await
        .map_err(|error| SubxtNodeError::FetchAuthorities(error.into()))?
        .map(|authorities| authorities.0.into_iter().map(|public| public.0).collect());

    Ok(authorities)
}

fn decode_slot_runtime_0_17(mut slot: &[u8]) -> Result<u64, SubxtNodeError> {
    let slot =
        runtime_0_17::runtime_types::sp_consensus_slots::Slot::decode(&mut slot).map(|x| x.0)?;
    Ok(slot)
}

async fn get_contract_state_runtime_0_17(
    address: SerializedContractAddress,
    block_hash: BlockHash,
    online_client: &OnlineClient<SubstrateConfig>,
) -> Result<SerializedContractState, SubxtNodeError> {
    // This returns the serialized contract state.
    let get_state = runtime_0_17::apis()
        .midnight_runtime_api()
        .get_contract_state(address.into());

    let state = online_client
        .runtime_api()
        .at(H256(block_hash.0))
        .call(get_state)
        .await
        .map_err(|error| SubxtNodeError::GetContractState(error.into()))?
        .map_err(|error| SubxtNodeError::GetContractState(format!("{error:?}").into()))?
        .into();

    Ok(state)
}

async fn get_zswap_state_root_runtime_0_17(
    block_hash: BlockHash,
    online_client: &OnlineClient<SubstrateConfig>,
) -> Result<Vec<u8>, SubxtNodeError> {
    let get_zswap_state_root = runtime_0_17::apis()
        .midnight_runtime_api()
        .get_zswap_state_root();

    let root = online_client
        .runtime_api()
        .at(H256(block_hash.0))
        .call(get_zswap_state_root)
        .await
        .map_err(|error| SubxtNodeError::GetZswapStateRoot(error.into()))?
        .map_err(|error| SubxtNodeError::GetZswapStateRoot(format!("{error:?}").into()))?;

    Ok(root)
}

async fn get_transaction_cost_runtime_0_17(
    transaction: &[u8],
    block_hash: BlockHash,
    online_client: &OnlineClient<SubstrateConfig>,
) -> Result<u128, SubxtNodeError> {
    let get_transaction_cost = runtime_0_17::apis()
        .midnight_runtime_api()
        .get_transaction_cost(transaction.to_owned());

    let (storage_cost, gas_cost) = online_client
        .runtime_api()
        .at(H256(block_hash.0))
        .call(get_transaction_cost)
        .await
        .map_err(|error| SubxtNodeError::GetTransactionCost(error.into()))?
        .map_err(|error| SubxtNodeError::GetTransactionCost(format!("{error:?}").into()))?;

    // Combine storage cost and gas cost for total fee
    // StorageCost = u128, GasCost = u64
    let total_cost = storage_cost.saturating_add(gas_cost as u128);
    Ok(total_cost)
}
