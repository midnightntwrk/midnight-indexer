// This file is part of midnight-indexer.
// Copyright (C) Midnight Foundation
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

use super::runtime_2_0_0 as runtime;
use crate::{
    domain::{
        BlockRef, DParameter, DustRegistrationEvent, TermsAndConditions,
        extrinsic::{Applied, EventIndex, Phase},
    },
    infra::subxt_node::{
        OnlineClientAtBlock, SubxtNodeError,
        runtimes::{
            AtBlock, BlockDetails, CallExt, EventExt, SystemTransaction, Transaction, transactions,
        },
    },
};
use futures::TryStreamExt;
use indexer_common::domain::{
    ByteVec, DustPublicKey, TermsAndConditionsHash, TransactionHash,
    bridge::{BridgeEvent, BridgeRecipient},
};
use parity_scale_codec::Decode;
use runtime::runtime_types::{
    frame_system::pallet::Event::{ExtrinsicFailed, ExtrinsicSuccess},
    midnight_node_runtime::{RuntimeCall, RuntimeEvent},
    pallet_midnight::pallet::{
        Call::{send_mn_transaction, set_tx_size_weight},
        Event::{TxApplied, TxPartialSuccess},
    },
    pallet_midnight_system::pallet::{
        Call::send_mn_system_transaction, Event::SystemTransactionApplied,
    },
};
use subxt::{SubstrateConfig, client::OfflineClientAtBlockT, error::RuntimeApiError};

pub async fn make_block_details(
    authorities: &mut Option<Vec<[u8; 32]>>,
    client: &AtBlock<impl OfflineClientAtBlockT<SubstrateConfig>>,
    block: BlockRef,
    extrinsics: Vec<Vec<u8>>,
    events: Vec<u8>,
) -> Result<BlockDetails, SubxtNodeError> {
    use runtime::{
        Call, Event,
        runtime_types::{
            pallet_c2m_bridge::pallet::Event as C2MBridgeEvent,
            pallet_cnight_observation::pallet::Event as CnightObservationEvent,
            pallet_partner_chains_session::pallet::Event::NewSession,
        },
    };

    let calls = client
        .extrinsics()
        .from_bytes(extrinsics)
        .await
        .iter()
        .map(|extrinsic| {
            extrinsic
                .map_err(|error| SubxtNodeError::GetNextExtrinsic(error.into()))?
                .decode_call_data_as::<Call>()
                .map_err(|error| SubxtNodeError::DecodeExtrinsicAsCall(error.into()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let events = client
        .events()
        .from_bytes(events)
        .iter()
        .enumerate()
        .map(|(index, event)| {
            let event = event.map_err(|error| SubxtNodeError::GetNextEvent(error.into()))?;
            let phase = Phase::from(event.phase());
            let event = event
                .decode_as::<Event>()
                .map_err(|error| SubxtNodeError::DecodeEvent(error.into()))?;
            Ok((phase, index as EventIndex, event))
        })
        .collect::<Result<Vec<_>, SubxtNodeError>>()?;

    let timestamp = calls.iter().find_map(CallExt::timestamp);
    let transactions = transactions(block, calls, &events);

    let mut dust_registration_events = vec![];
    let mut bridge_events = vec![];

    for (phase, index, event) in events {
        let mut push_dust_registration =
            |event| dust_registration_events.push((phase, index, event));
        let mut push_bridge_event = |event| bridge_events.push((phase, index, event));

        match event {
            Event::Session(NewSession { .. }) => {
                *authorities = None;
            }

            // DUST registration events from NativeTokenObservation pallet.
            Event::CNightObservation(native_token_event) => match native_token_event {
                CnightObservationEvent::Registration(event) => {
                    push_dust_registration(DustRegistrationEvent::Registration {
                        cardano_stake_key: event.cardano_reward_address.0.into(),
                        dust_address: event.dust_public_key.0.0.into(),
                    });
                }

                CnightObservationEvent::Deregistration(event) => {
                    push_dust_registration(DustRegistrationEvent::Deregistration {
                        cardano_stake_key: event.cardano_reward_address.0.into(),
                        dust_address: event.dust_public_key.0.0.into(),
                    });
                }

                CnightObservationEvent::MappingAdded(event) => {
                    push_dust_registration(DustRegistrationEvent::MappingAdded {
                        cardano_stake_key: event.cardano_reward_address.0.into(),
                        dust_address: event.dust_public_key.0.0.into(),
                        utxo_id: event.utxo_id.tx_hash.0.as_ref().into(),
                        utxo_index: event.utxo_id.index.0.into(),
                    });
                }

                CnightObservationEvent::MappingRemoved(event) => {
                    push_dust_registration(DustRegistrationEvent::MappingRemoved {
                        cardano_stake_key: event.cardano_reward_address.0.into(),
                        dust_address: event.dust_public_key.0.0.into(),
                        utxo_id: event.utxo_id.tx_hash.0.as_ref().into(),
                        utxo_index: event.utxo_id.index.0.into(),
                    });
                }

                _ => {}
            },

            // c2m-bridge events (node 2.0.0-alpha.1 + introduced this pallet
            // via PR #1386 et al.). Pallet ships inert; events only fire after
            // governance enables it (set MainChainScripts + data checkpoint).
            //
            // Matching shape (see indexer-common::domain::bridge::BridgeEvent):
            //   UserTransfer            { mc_tx_hash, amount, recipient, midnight_tx_hash }
            //   ReserveTransfer         { mc_tx_hash, amount, midnight_tx_hash }
            //   InvalidTransfer         { mc_tx_hash, amount, midnight_tx_hash }
            //   UnapprovedTransfer      { mc_tx_hash, amount, recipient, midnight_tx_hash }
            //   SubminimalFlushTransfer { amount, count, midnight_tx_hash }
            Event::C2MBridge(bridge_event) => match bridge_event {
                C2MBridgeEvent::UserTransfer {
                    mc_tx_hash,
                    amount,
                    recipient,
                    midnight_tx_hash,
                } => {
                    let recipient = BridgeRecipient::new(recipient.0.0)?;
                    push_bridge_event(BridgeEvent::UserTransfer {
                        mc_tx_hash: mc_tx_hash.0.into(),
                        amount,
                        recipient,
                        midnight_tx_hash: midnight_tx_hash.into(),
                    });
                }
                C2MBridgeEvent::ReserveTransfer {
                    mc_tx_hash,
                    amount,
                    midnight_tx_hash,
                } => {
                    push_bridge_event(BridgeEvent::ReserveTransfer {
                        mc_tx_hash: mc_tx_hash.0.into(),
                        amount,
                        midnight_tx_hash: midnight_tx_hash.into(),
                    });
                }
                C2MBridgeEvent::InvalidTransfer {
                    mc_tx_hash,
                    amount,
                    midnight_tx_hash,
                } => {
                    push_bridge_event(BridgeEvent::InvalidTransfer {
                        mc_tx_hash: mc_tx_hash.0.into(),
                        amount,
                        midnight_tx_hash: midnight_tx_hash.into(),
                    });
                }
                C2MBridgeEvent::UnapprovedTransfer {
                    mc_tx_hash,
                    amount,
                    recipient,
                    midnight_tx_hash,
                } => {
                    let recipient = BridgeRecipient::new(recipient.0.0)?;
                    push_bridge_event(BridgeEvent::UnapprovedTransfer {
                        mc_tx_hash: mc_tx_hash.0.into(),
                        amount,
                        recipient,
                        midnight_tx_hash: midnight_tx_hash.into(),
                    });
                }
                C2MBridgeEvent::SubminimalFlushTransfer {
                    amount,
                    count,
                    midnight_tx_hash,
                } => {
                    push_bridge_event(BridgeEvent::SubminimalFlushTransfer {
                        amount,
                        count,
                        midnight_tx_hash: midnight_tx_hash.into(),
                    });
                }
            },

            _ => {}
        }
    }

    Ok(BlockDetails {
        timestamp,
        transactions,
        dust_registration_events,
        bridge_events,
    })
}

impl CallExt for runtime::Call {
    fn transaction(self) -> Option<Transaction> {
        use RuntimeCall::*;

        // Exhaustive, so that a new pallet or call does not compile until it is handled.
        match self {
            Midnight(send_mn_transaction { midnight_tx }) => {
                Some(Transaction::Regular(midnight_tx.into()))
            }
            MidnightSystem(send_mn_system_transaction { midnight_system_tx }) => {
                Some(Transaction::System(midnight_system_tx.into()))
            }
            Midnight(set_tx_size_weight { .. })
            | Timestamp(_)
            | CNightObservation(_)
            | System(_)
            | Grandpa(_)
            | SessionCommitteeManagement(_)
            | Preimage(_)
            | MultiBlockMigrations(_)
            | PalletSession(_)
            | Scheduler(_)
            | TxPause(_)
            | Beefy(_)
            | Bridge(_)
            | C2MBridge(_)
            | Council(_)
            | CouncilMembership(_)
            | TechnicalCommittee(_)
            | TechnicalCommitteeMembership(_)
            | FederatedAuthority(_)
            | FederatedAuthorityObservation(_)
            | SystemParameters(_) => None,
        }
    }

    fn timestamp(&self) -> Option<u64> {
        use RuntimeCall::Timestamp;

        match self {
            Timestamp(runtime::timestamp::Call::set { now }) => Some(*now),
            _ => None,
        }
    }
}

impl EventExt for runtime::Event {
    fn outcome(&self) -> Option<Result<Applied, String>> {
        use RuntimeEvent::*;

        match self {
            Midnight(TxApplied(details)) => Some(Ok(Applied::Fully {
                tx_hash: details.tx_hash.into(),
            })),
            Midnight(TxPartialSuccess(details)) => Some(Ok(Applied::Partially {
                tx_hash: details.tx_hash.into(),
            })),
            System(ExtrinsicFailed { dispatch_error, .. }) => {
                Some(Err(format!("{dispatch_error:?}")))
            }
            _ => None,
        }
    }

    fn is_success(&self) -> bool {
        matches!(self, RuntimeEvent::System(ExtrinsicSuccess { .. }))
    }
}

impl SystemTransaction for runtime::Event {
    fn transaction(&self) -> Option<(ByteVec, TransactionHash)> {
        match self {
            RuntimeEvent::MidnightSystem(SystemTransactionApplied(transaction_applied)) => Some((
                transaction_applied
                    .serialized_system_transaction
                    .clone()
                    .into(),
                transaction_applied.hash.into(),
            )),
            _ => None,
        }
    }
}

pub async fn fetch_authorities(
    block: &OnlineClientAtBlock,
) -> Result<Vec<[u8; 32]>, SubxtNodeError> {
    let authorities = block
        .storage()
        .entry(runtime::storage().aura().authorities())
        .map_err(|error| SubxtNodeError::FetchAuthorities(error.into()))?
        .fetch(())
        .await
        .map_err(|error| SubxtNodeError::FetchAuthorities(error.into()))?
        .decode()
        .map_err(|error| SubxtNodeError::DecodeAuthorities(error.into()))?;
    let authorities = authorities.0.into_iter().map(|a| a.0).collect();

    Ok(authorities)
}

pub fn decode_slot(mut slot: &[u8]) -> Result<u64, SubxtNodeError> {
    let slot = runtime::runtime_types::sp_consensus_slots::Slot::decode(&mut slot).map(|x| x.0)?;
    Ok(slot)
}

pub async fn get_zswap_merkle_tree_root(
    block: &OnlineClientAtBlock,
) -> Result<Vec<u8>, SubxtNodeError> {
    let get_zswap_state_root = runtime::runtime_apis()
        .midnight_runtime_api()
        .get_zswap_state_root();

    let root = block.runtime_apis().call(&get_zswap_state_root).await;

    let root = match root {
        // Retry with online client at parent block if codegen is incompatible which can happen for
        // runtime updates, because subxt uses next metadata whereas Node uses previous metadata.
        Err(RuntimeApiError::IncompatibleCodegen) => {
            let parent_hash = block
                .block_header()
                .await
                .map_err(|error| SubxtNodeError::GetBlockHeader(error.into()))?
                .parent_hash;
            let block = block
                .online_client()
                .at_block(parent_hash)
                .await
                .map_err(|error| SubxtNodeError::GetOnlineClientAt(parent_hash, error.into()))?;
            block.runtime_apis().call(get_zswap_state_root).await
        }

        other => other,
    };

    root.map_err(|error| SubxtNodeError::GetZswapStateRoot(error.into()))?
        .map_err(|error| SubxtNodeError::GetZswapStateRoot(format!("{error:?}").into()))
}

pub async fn get_ledger_state_root(
    block: &OnlineClientAtBlock,
) -> Result<Option<Vec<u8>>, SubxtNodeError> {
    let get_ledger_state_root = runtime::runtime_apis()
        .midnight_runtime_api()
        .get_ledger_state_root();

    let root = block
        .runtime_apis()
        .call(get_ledger_state_root)
        .await
        .map_err(|error| SubxtNodeError::GetLedgerStateRoot(error.into()))?
        .map_err(|error| SubxtNodeError::GetLedgerStateRoot(format!("{error:?}").into()))?;

    Ok(Some(root))
}

pub async fn get_d_parameter(block: &OnlineClientAtBlock) -> Result<DParameter, SubxtNodeError> {
    let get_d_param = runtime::runtime_apis()
        .system_parameters_api()
        .get_d_parameter();

    let d_parameter = block
        .runtime_apis()
        .call(get_d_param)
        .await
        .map_err(|error| SubxtNodeError::GetDParameter(error.into()))?;

    Ok(DParameter {
        num_permissioned_candidates: d_parameter.num_permissioned_candidates,
        num_registered_candidates: d_parameter.num_registered_candidates,
    })
}

pub async fn fetch_genesis_cnight_registrations(
    block: &OnlineClientAtBlock,
) -> Result<Vec<DustRegistrationEvent>, SubxtNodeError> {
    // In ledger 9 the cNight observation pallet stores registrations as a
    // double map `(cardano_reward_address, utxo_id) -> dust_public_key`
    // (was a single map to `Vec<MappingEntry>` in ledger 8). Each entry is one
    // registration: the two keys carry the Cardano address and the UTXO id, the
    // value carries the DUST public key.
    let query = runtime::storage().c_night_observation().mapping();
    block
        .storage()
        .entry(query)
        .map_err(|error| SubxtNodeError::FetchGenesisCnightRegistrations(error.into()))?
        .iter(())
        .await
        .map_err(|error| SubxtNodeError::FetchGenesisCnightRegistrations(error.into()))?
        .try_collect::<Vec<_>>()
        .await
        .map_err(|error| SubxtNodeError::FetchGenesisCnightRegistrations(error.into()))?
        .into_iter()
        .try_fold(vec![], |mut events, entry| {
            let (cardano_reward_address, utxo_id) =
                entry.key().and_then(|key| key.decode()).map_err(|error| {
                    SubxtNodeError::DecodeGenesisCnightRegistrationKey(error.into())
                })?;
            let dust_public_key = entry
                .value()
                .decode()
                .map_err(|error| SubxtNodeError::DecodeGenesisCnightRegistrations(error.into()))?;

            let cardano_stake_key = cardano_reward_address.0.into();
            let dust_address = DustPublicKey::from(dust_public_key.0.0);
            let utxo_index = utxo_id.index.0.into();
            let utxo_id = utxo_id.tx_hash.0.as_ref().into();

            events.push(DustRegistrationEvent::Registration {
                cardano_stake_key,
                dust_address: dust_address.clone(),
            });
            events.push(DustRegistrationEvent::MappingAdded {
                cardano_stake_key,
                dust_address,
                utxo_id,
                utxo_index,
            });

            Ok(events)
        })
}

pub async fn get_terms_and_conditions(
    block: &OnlineClientAtBlock,
) -> Result<Option<TermsAndConditions>, SubxtNodeError> {
    let get_tc = runtime::runtime_apis()
        .system_parameters_api()
        .get_terms_and_conditions();

    let tc = block
        .runtime_apis()
        .call(get_tc)
        .await
        .map_err(|error| SubxtNodeError::GetTermsAndConditions(error.into()))?;

    Ok(tc.map(|response| {
        let hash = TermsAndConditionsHash::from(response.hash.0);
        let url = String::from_utf8_lossy(&response.url).to_string();
        TermsAndConditions { hash, url }
    }))
}
