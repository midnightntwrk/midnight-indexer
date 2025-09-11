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

use crate::infra::subxt_node::SubxtNodeError;
use indexer_common::domain::{
    BlockHash, ByteVec, PROTOCOL_VERSION_000_016_000, ProtocolVersion,
    ledger::{SerializedContractAddress, SerializedContractState},
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

/// Infrastructure representation of DUST registration event from the NativeTokenObservation pallet.
#[derive(Debug, Clone)]
pub(super) enum DustRegistrationEvent {
    Registration {
        cardano_address: Vec<u8>,
        dust_address: Vec<u8>,
    },
    Deregistration {
        cardano_address: Vec<u8>,
        dust_address: Vec<u8>,
    },
    MappingAdded {
        cardano_address: Vec<u8>,
        dust_address: String,
        utxo_id: String,
    },
    MappingRemoved {
        cardano_address: Vec<u8>,
        dust_address: String,
        utxo_id: String,
    },
}

impl TryFrom<DustRegistrationEvent> for crate::domain::DustRegistrationEvent {
    type Error = SubxtNodeError;

    fn try_from(event: DustRegistrationEvent) -> Result<Self, Self::Error> {
        use indexer_common::domain::{ByteArray, ByteVec};

        Ok(match event {
            DustRegistrationEvent::Registration {
                cardano_address,
                dust_address,
            } => crate::domain::DustRegistrationEvent::Registration {
                cardano_address: ByteVec::from(cardano_address),
                dust_address: ByteArray::try_from(dust_address).map_err(|_| {
                    SubxtNodeError::InvalidDustAddress(
                        "registration: DUST address must be 32 bytes".into(),
                    )
                })?,
            },
            DustRegistrationEvent::Deregistration {
                cardano_address,
                dust_address,
            } => crate::domain::DustRegistrationEvent::Deregistration {
                cardano_address: ByteVec::from(cardano_address),
                dust_address: ByteArray::try_from(dust_address).map_err(|_| {
                    SubxtNodeError::InvalidDustAddress(
                        "deregistration: DUST address must be 32 bytes".into(),
                    )
                })?,
            },
            DustRegistrationEvent::MappingAdded {
                cardano_address,
                dust_address,
                utxo_id,
            } => {
                // dust_address and utxo_id are hex-encoded strings
                let dust_addr_bytes = const_hex::decode(&dust_address).map_err(|e| {
                    SubxtNodeError::InvalidDustAddress(format!(
                        "mapping added: invalid hex encoding for DUST address: {}",
                        e
                    ))
                })?;

                let utxo_id_bytes = const_hex::decode(&utxo_id).map_err(|e| {
                    SubxtNodeError::InvalidDustAddress(format!(
                        "mapping added: invalid hex encoding for UTXO ID: {}",
                        e
                    ))
                })?;

                crate::domain::DustRegistrationEvent::MappingAdded {
                    cardano_address: ByteVec::from(cardano_address),
                    dust_address: ByteArray::try_from(dust_addr_bytes).map_err(|e| {
                        SubxtNodeError::InvalidDustAddress(format!(
                            "mapping added: DUST address must be 32 bytes: {:?}",
                            e
                        ))
                    })?,
                    utxo_id: ByteVec::from(utxo_id_bytes),
                }
            }
            DustRegistrationEvent::MappingRemoved {
                cardano_address,
                dust_address,
                utxo_id,
            } => {
                // dust_address and utxo_id are hex-encoded strings
                let dust_addr_bytes = const_hex::decode(&dust_address).map_err(|e| {
                    SubxtNodeError::InvalidDustAddress(format!(
                        "mapping removed: invalid hex encoding for DUST address: {}",
                        e
                    ))
                })?;

                let utxo_id_bytes = const_hex::decode(&utxo_id).map_err(|e| {
                    SubxtNodeError::InvalidDustAddress(format!(
                        "mapping removed: invalid hex encoding for UTXO ID: {}",
                        e
                    ))
                })?;

                crate::domain::DustRegistrationEvent::MappingRemoved {
                    cardano_address: ByteVec::from(cardano_address),
                    dust_address: ByteArray::try_from(dust_addr_bytes).map_err(|e| {
                        SubxtNodeError::InvalidDustAddress(format!(
                            "mapping removed: DUST address must be 32 bytes: {:?}",
                            e
                        ))
                    })?,
                    utxo_id: ByteVec::from(utxo_id_bytes),
                }
            }
        })
    }
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
    if protocol_version.is_compatible(PROTOCOL_VERSION_000_016_000) {
        make_block_details_runtime_0_16(extrinsics, events, authorities).await
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
    if protocol_version.is_compatible(PROTOCOL_VERSION_000_016_000) {
        fetch_authorities_runtime_0_16(block_hash, online_client).await
    } else {
        Err(SubxtNodeError::InvalidProtocolVersion(protocol_version))
    }
}

/// Decode slot depending on the given protocol version.
pub fn decode_slot(slot: &[u8], protocol_version: ProtocolVersion) -> Result<u64, SubxtNodeError> {
    if protocol_version.is_compatible(PROTOCOL_VERSION_000_016_000) {
        decode_slot_runtime_0_16(slot)
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
    if protocol_version.is_compatible(PROTOCOL_VERSION_000_016_000) {
        get_contract_state_runtime_0_16(address, block_hash, online_client).await
    } else {
        Err(SubxtNodeError::InvalidProtocolVersion(protocol_version))
    }
}

pub async fn get_zswap_state_root(
    block_hash: BlockHash,
    protocol_version: ProtocolVersion,
    online_client: &OnlineClient<SubstrateConfig>,
) -> Result<Vec<u8>, SubxtNodeError> {
    if protocol_version.is_compatible(PROTOCOL_VERSION_000_016_000) {
        get_zswap_state_root_runtime_0_16(block_hash, online_client).await
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
    if protocol_version.is_compatible(PROTOCOL_VERSION_000_016_000) {
        get_transaction_cost_runtime_0_16(transaction.as_ref(), block_hash, online_client).await
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
                    Call, Event, midnight, midnight_system, timestamp, native_token_observation,
                    runtime_types::pallet_partner_chains_session::pallet::Event::NewSession,
                };

                let calls = extrinsics
                    .iter()
                    .map(|extrinsic| {
                        let call = extrinsic.as_root_extrinsic::<Call>().map_err(Box::new)?;
                        Ok(call)
                    })
                    .filter_ok(|call|
                        matches!(
                            call,
                            Call::Timestamp(_) | Call::Midnight(_) | Call::MidnightSystem(_)
                        )
                    )
                    .collect::<Result<Vec<_>, SubxtNodeError>>()?;

                let timestamp = calls.iter().find_map(|call| match call {
                    Call::Timestamp(timestamp::Call::set { now }) => Some(*now),
                    _ => None,
                });

                let mut transactions: Vec<Transaction> = calls
                    .into_iter()
                    .filter_map(|call| match call {
                        Call::Midnight(
                            midnight::Call::send_mn_transaction { midnight_tx }
                        ) => {
                            Some(Transaction::Regular(midnight_tx.into()))
                        }

                        Call::MidnightSystem(
                            midnight_system::Call::send_mn_system_transaction { midnight_system_tx }
                        ) => {
                            Some(Transaction::System(midnight_system_tx.into()))
                        }

                        _ => None,
                    })
                    .collect();

                // Also collect system transactions from events (e.g., CNightGeneratesDust)
                // and DUST registration events from NativeTokenObservation pallet
                let mut dust_registration_events = Vec::new();

                for event in events.iter().flatten() {
                    let event = event.as_root_event::<Event>();
                    match event {
                        Ok(Event::Session(NewSession { .. })) => {
                            *authorities = None;
                        }
                        Ok(Event::MidnightSystem(midnight_system::Event::SystemTransactionApplied(e))) => {
                            // System transactions created by the node (not from extrinsics)
                            transactions.push(Transaction::System(e.serialized_system_transaction.clone().into()));
                        }
                        Ok(Event::NativeTokenObservation(native_event)) => {
                            // Handle DUST registration events
                            match native_event {
                                native_token_observation::Event::Registration(reg) => {
                                    dust_registration_events.push(DustRegistrationEvent::Registration {
                                        cardano_address: reg.cardano_address.0.clone(),
                                        dust_address: reg.dust_address.clone(),
                                    });
                                }
                                native_token_observation::Event::Deregistration(dereg) => {
                                    dust_registration_events.push(DustRegistrationEvent::Deregistration {
                                        cardano_address: dereg.cardano_address.0.clone(),
                                        dust_address: dereg.dust_address.clone(),
                                    });
                                }
                                native_token_observation::Event::MappingAdded(mapping) => {
                                    dust_registration_events.push(DustRegistrationEvent::MappingAdded {
                                        cardano_address: mapping.cardano_address.0.clone(),
                                        dust_address: mapping.dust_address.clone(),
                                        utxo_id: mapping.utxo_id.clone(),
                                    });
                                }
                                native_token_observation::Event::MappingRemoved(mapping) => {
                                    dust_registration_events.push(DustRegistrationEvent::MappingRemoved {
                                        cardano_address: mapping.cardano_address.0.clone(),
                                        dust_address: mapping.dust_address.clone(),
                                        utxo_id: mapping.utxo_id.clone(),
                                    });
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }

                Ok(BlockDetails {
                    timestamp,
                    transactions,
                    dust_registration_events,
                })
            }
        }
    };
}

make_block_details!(runtime_0_16);

macro_rules! fetch_authorities {
    ($module:ident) => {
        paste::paste! {
            async fn [<fetch_authorities_ $module>](
                block_hash: BlockHash,
                online_client: &OnlineClient<SubstrateConfig>,
            ) -> Result<Option<Vec<[u8; 32]>>, SubxtNodeError> {
                let authorities = online_client
                    .storage()
                    .at(H256(block_hash.0))
                    .fetch(&$module::storage().aura().authorities())
                    .await
                    .map_err(Box::new)?
                    .map(|authorities| authorities.0.into_iter().map(|public| public.0).collect());

                Ok(authorities)
            }
        }
    };
}

fetch_authorities!(runtime_0_16);

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

decode_slot!(runtime_0_16);

macro_rules! get_contract_state {
    ($module:ident) => {
        paste::paste! {
            async fn [<get_contract_state_ $module>](
                address: SerializedContractAddress,
                block_hash: BlockHash,
                online_client: &OnlineClient<SubstrateConfig>,
            ) -> Result<SerializedContractState, SubxtNodeError> {
                // This returns the serialized contract state.
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

get_contract_state!(runtime_0_16);

macro_rules! get_zswap_state_root {
    ($module:ident) => {
        paste::paste! {
            async fn [<get_zswap_state_root_ $module>](
                block_hash: BlockHash,
                online_client: &OnlineClient<SubstrateConfig>,
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

get_zswap_state_root!(runtime_0_16);

macro_rules! get_transaction_cost {
    ($module:ident) => {
        paste::paste! {
            async fn [<get_transaction_cost_ $module>](
                transaction: &[u8],
                block_hash: BlockHash,
                online_client: &OnlineClient<SubstrateConfig>,
            ) -> Result<u128, SubxtNodeError> {
                let get_transaction_cost = $module::apis()
                    .midnight_runtime_api()
                    .get_transaction_cost(transaction.to_owned());

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

get_transaction_cost!(runtime_0_16);
