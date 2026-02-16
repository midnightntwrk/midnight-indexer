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
    domain::{DParameter, DustRegistrationEvent, TermsAndConditions},
    infra::subxt_node::{
        SubxtNodeError,
        runtimes::{BlockDetails, Transaction},
    },
};
use futures::{TryStreamExt, stream};
use indexer_common::domain::{
    BlockHash, ByteVec, DustPublicKey, SerializedContractAddress, SerializedContractState,
    TermsAndConditionsHash,
};
use itertools::Itertools;
use parity_scale_codec::Decode;
use subxt::{OnlineClient, SubstrateConfig, blocks::Extrinsics, events::Events, utils::H256};

pub async fn make_block_details(
    extrinsics: Extrinsics<SubstrateConfig, OnlineClient<SubstrateConfig>>,
    events: Events<SubstrateConfig>,
    authorities: &mut Option<Vec<[u8; 32]>>,
) -> Result<BlockDetails, SubxtNodeError> {
    use super::runtime_0_22_0::{
        Call, Event,
        runtime_types::{
            pallet_cnight_observation::pallet::Event as CnightObservationEvent,
            pallet_midnight::pallet::Call::send_mn_transaction,
            pallet_midnight_system::pallet::{
                Call::send_mn_system_transaction, Event::SystemTransactionApplied,
            },
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

    let transactions = calls
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
    let mut system_transactions_from_events = vec![];

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
            // These come from inherents which execute BEFORE regular transactions,
            // so they must be prepended to maintain correct execution order.
            Event::MidnightSystem(SystemTransactionApplied(transaction_applied)) => {
                system_transactions_from_events.push(Transaction::System(ByteVec::from(
                    transaction_applied.serialized_system_transaction,
                )));
            }

            // DUST registration events from NativeTokenObservation pallet.
            Event::CNightObservation(native_token_event) => match native_token_event {
                CnightObservationEvent::Registration(event) => {
                    dust_registration_events.push(DustRegistrationEvent::Registration {
                        cardano_address: event.cardano_reward_address.0.into(),
                        dust_address: event.dust_public_key.0.0.into(),
                    });
                }

                CnightObservationEvent::Deregistration(event) => {
                    dust_registration_events.push(DustRegistrationEvent::Deregistration {
                        cardano_address: event.cardano_reward_address.0.into(),
                        dust_address: event.dust_public_key.0.0.into(),
                    });
                }

                CnightObservationEvent::MappingAdded(event) => {
                    dust_registration_events.push(DustRegistrationEvent::MappingAdded {
                        cardano_address: event.cardano_reward_address.0.into(),
                        dust_address: event.dust_public_key.0.0.into(),
                        utxo_id: event.utxo_tx_hash.0.as_ref().into(),
                        utxo_index: event.utxo_index.into(),
                    });
                }

                CnightObservationEvent::MappingRemoved(event) => {
                    dust_registration_events.push(DustRegistrationEvent::MappingRemoved {
                        cardano_address: event.cardano_reward_address.0.into(),
                        dust_address: event.dust_public_key.0.0.into(),
                        utxo_id: event.utxo_tx_hash.0.as_ref().into(),
                        utxo_index: event.utxo_index.into(),
                    });
                }

                _ => {}
            },

            _ => {}
        }
    }

    // Prepend system transactions from events (inherents) before regular transactions.
    // In Substrate, inherents execute before regular transactions in a block.
    system_transactions_from_events.extend(transactions);
    let transactions = system_transactions_from_events;

    Ok(BlockDetails {
        timestamp,
        transactions,
        dust_registration_events,
    })
}

pub async fn fetch_authorities(
    block_hash: BlockHash,
    online_client: &OnlineClient<SubstrateConfig>,
) -> Result<Option<Vec<[u8; 32]>>, SubxtNodeError> {
    let authorities = online_client
        .storage()
        .at(H256(block_hash.0))
        .fetch(&super::runtime_0_22_0::storage().aura().authorities())
        .await
        .map_err(|error| SubxtNodeError::FetchAuthorities(error.into()))?
        .map(|authorities| authorities.0.into_iter().map(|public| public.0).collect());

    Ok(authorities)
}

pub fn decode_slot(mut slot: &[u8]) -> Result<u64, SubxtNodeError> {
    let slot = super::runtime_0_22_0::runtime_types::sp_consensus_slots::Slot::decode(&mut slot)
        .map(|x| x.0)?;
    Ok(slot)
}

pub async fn get_contract_state(
    address: SerializedContractAddress,
    block_hash: BlockHash,
    online_client: &OnlineClient<SubstrateConfig>,
) -> Result<SerializedContractState, SubxtNodeError> {
    // This returns the serialized contract state.
    let get_state = super::runtime_0_22_0::apis()
        .midnight_runtime_api()
        .get_contract_state(address.as_slice().into());

    let state = online_client
        .runtime_api()
        .at(H256(block_hash.0))
        .call(get_state)
        .await
        .map_err(|error| {
            SubxtNodeError::GetContractState(address.clone(), block_hash, error.into())
        })?
        .map_err(|error| {
            SubxtNodeError::GetContractState(address, block_hash, format!("{error:?}").into())
        })?
        .into();

    Ok(state)
}

pub async fn get_zswap_state_root(
    block_hash: BlockHash,
    online_client: &OnlineClient<SubstrateConfig>,
) -> Result<Vec<u8>, SubxtNodeError> {
    let get_zswap_state_root = super::runtime_0_22_0::apis()
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

pub async fn get_transaction_cost(
    transaction: &[u8],
    block_hash: BlockHash,
    online_client: &OnlineClient<SubstrateConfig>,
) -> Result<u128, SubxtNodeError> {
    let get_transaction_cost = super::runtime_0_22_0::apis()
        .midnight_runtime_api()
        .get_transaction_cost(transaction.to_owned());

    let cost = online_client
        .runtime_api()
        .at(H256(block_hash.0))
        .call(get_transaction_cost)
        .await
        .map_err(|error| SubxtNodeError::GetTransactionCost(error.into()))?
        .map_err(|error| SubxtNodeError::GetTransactionCost(format!("{error:?}").into()))?;

    Ok(cost as u128)
}

pub async fn get_d_parameter(
    block_hash: BlockHash,
    online_client: &OnlineClient<SubstrateConfig>,
) -> Result<DParameter, SubxtNodeError> {
    let get_d_param = super::runtime_0_22_0::apis()
        .system_parameters_api()
        .get_d_parameter();

    let d_parameter = online_client
        .runtime_api()
        .at(H256(block_hash.0))
        .call(get_d_param)
        .await
        .map_err(|error| SubxtNodeError::GetDParameter(error.into()))?;

    Ok(DParameter {
        num_permissioned_candidates: d_parameter.num_permissioned_candidates,
        num_registered_candidates: d_parameter.num_registered_candidates,
    })
}

pub async fn fetch_genesis_cnight_registrations(
    block_hash: BlockHash,
    online_client: &OnlineClient<SubstrateConfig>,
) -> Result<Vec<DustRegistrationEvent>, SubxtNodeError> {
    let query = super::runtime_0_22_0::storage()
        .c_night_observation()
        .mappings_iter();
    let mappings = online_client
        .storage()
        .at(H256(block_hash.0))
        .iter(query)
        .await
        .map_err(|error| SubxtNodeError::FetchGenesisCnightRegistrations(error.into()))?;

    mappings
        .map_ok(|kv| {
            // A registration is valid only if there is exactly one mapping entry.
            let events = if kv.value.len() == 1 {
                let entry = &kv.value[0];
                let cardano_address = entry.cardano_reward_address.0.into();
                let dust_address = DustPublicKey::from(entry.dust_public_key.0.0.clone());
                let utxo_id = entry.utxo_tx_hash.0.as_ref().into();
                let utxo_index = entry.utxo_index.into();

                vec![
                    DustRegistrationEvent::Registration {
                        cardano_address,
                        dust_address: dust_address.clone(),
                    },
                    DustRegistrationEvent::MappingAdded {
                        cardano_address,
                        dust_address,
                        utxo_id,
                        utxo_index,
                    },
                ]
            } else {
                vec![]
            };
            stream::iter(events.into_iter().map(Ok::<_, subxt::Error>))
        })
        .try_flatten()
        .try_collect()
        .await
        .map_err(|error| SubxtNodeError::FetchGenesisCnightRegistrations(error.into()))
}

pub async fn get_ledger_state_root(
    block_hash: BlockHash,
    online_client: &OnlineClient<SubstrateConfig>,
) -> Result<Option<Vec<u8>>, SubxtNodeError> {
    let get_ledger_state_root = super::runtime_0_22_0::apis()
        .midnight_runtime_api()
        .get_ledger_state_root();

    let root = online_client
        .runtime_api()
        .at(H256(block_hash.0))
        .call(get_ledger_state_root)
        .await
        .map_err(|error| SubxtNodeError::GetLedgerStateRoot(error.into()))?
        .map_err(|error| SubxtNodeError::GetLedgerStateRoot(format!("{error:?}").into()))?;

    Ok(Some(root))
}

pub async fn get_terms_and_conditions(
    block_hash: BlockHash,
    online_client: &OnlineClient<SubstrateConfig>,
) -> Result<Option<TermsAndConditions>, SubxtNodeError> {
    let get_tc = super::runtime_0_22_0::apis()
        .system_parameters_api()
        .get_terms_and_conditions();

    let tc = online_client
        .runtime_api()
        .at(H256(block_hash.0))
        .call(get_tc)
        .await
        .map_err(|error| SubxtNodeError::GetTermsAndConditions(error.into()))?;

    Ok(tc.map(|response| {
        let hash = TermsAndConditionsHash::from(response.hash.0);
        let url = String::from_utf8_lossy(&response.url).to_string();
        TermsAndConditions { hash, url }
    }))
}
