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

mod v0_20_0;
mod v0_21_0;
mod v0_22_0;

// To see how this is generated, look in build.rs
include!(concat!(env!("OUT_DIR"), "/generated_runtime.rs"));

use crate::{
    domain::{DParameter, DustRegistrationEvent, TermsAndConditions},
    infra::subxt_node::SubxtNodeError,
};
use indexer_common::domain::{
    BlockHash, ByteVec, NodeVersion, ProtocolVersion, SerializedContractAddress,
    SerializedContractState,
};
use subxt::{OnlineClient, SubstrateConfig, blocks::Extrinsics, events::Events};

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
    match protocol_version.node_version()? {
        NodeVersion::V0_20 => v0_20_0::make_block_details(extrinsics, events, authorities).await,
        NodeVersion::V0_21 => v0_21_0::make_block_details(extrinsics, events, authorities).await,
        NodeVersion::V0_22 => v0_22_0::make_block_details(extrinsics, events, authorities).await,
    }
}

/// Fetch authorities depending on the given protocol version.
pub async fn fetch_authorities(
    block_hash: BlockHash,
    protocol_version: ProtocolVersion,
    online_client: &OnlineClient<SubstrateConfig>,
) -> Result<Option<Vec<[u8; 32]>>, SubxtNodeError> {
    match protocol_version.node_version()? {
        NodeVersion::V0_20 => v0_20_0::fetch_authorities(block_hash, online_client).await,
        NodeVersion::V0_21 => v0_21_0::fetch_authorities(block_hash, online_client).await,
        NodeVersion::V0_22 => v0_22_0::fetch_authorities(block_hash, online_client).await,
    }
}

/// Decode slot depending on the given protocol version.
pub fn decode_slot(slot: &[u8], protocol_version: ProtocolVersion) -> Result<u64, SubxtNodeError> {
    match protocol_version.node_version()? {
        NodeVersion::V0_20 => v0_20_0::decode_slot(slot),
        NodeVersion::V0_21 => v0_21_0::decode_slot(slot),
        NodeVersion::V0_22 => v0_22_0::decode_slot(slot),
    }
}

/// Get contract state depending on the given protocol version.
pub async fn get_contract_state(
    address: SerializedContractAddress,
    block_hash: BlockHash,
    protocol_version: ProtocolVersion,
    online_client: &OnlineClient<SubstrateConfig>,
) -> Result<SerializedContractState, SubxtNodeError> {
    match protocol_version.node_version()? {
        NodeVersion::V0_20 => v0_20_0::get_contract_state(address, block_hash, online_client).await,
        NodeVersion::V0_21 => v0_21_0::get_contract_state(address, block_hash, online_client).await,
        NodeVersion::V0_22 => v0_22_0::get_contract_state(address, block_hash, online_client).await,
    }
}

pub async fn get_zswap_state_root(
    block_hash: BlockHash,
    protocol_version: ProtocolVersion,
    online_client: &OnlineClient<SubstrateConfig>,
) -> Result<Vec<u8>, SubxtNodeError> {
    match protocol_version.node_version()? {
        NodeVersion::V0_20 => v0_20_0::get_zswap_state_root(block_hash, online_client).await,
        NodeVersion::V0_21 => v0_21_0::get_zswap_state_root(block_hash, online_client).await,
        NodeVersion::V0_22 => v0_22_0::get_zswap_state_root(block_hash, online_client).await,
    }
}

/// Get the pure ledger state root (without StorableLedgerState wrapping) at the given block.
pub async fn get_ledger_state_root(
    block_hash: BlockHash,
    protocol_version: ProtocolVersion,
    online_client: &OnlineClient<SubstrateConfig>,
) -> Result<Option<Vec<u8>>, SubxtNodeError> {
    match protocol_version.node_version()? {
        NodeVersion::V0_20 => v0_20_0::get_ledger_state_root(block_hash, online_client).await,
        NodeVersion::V0_21 => v0_21_0::get_ledger_state_root(block_hash, online_client).await,
        NodeVersion::V0_22 => v0_22_0::get_ledger_state_root(block_hash, online_client).await,
    }
}

// TODO: This does not return the cost in DUST/SPEC, but some substrate weight based cost; this
// needs to be replaced by getting the read cost from Node events. See PM-20973.
/// Get cost for the given serialized transaction depending on the given protocol version.
pub async fn get_transaction_cost(
    transaction: impl AsRef<[u8]>,
    block_hash: BlockHash,
    protocol_version: ProtocolVersion,
    online_client: &OnlineClient<SubstrateConfig>,
) -> Result<u128, SubxtNodeError> {
    match protocol_version.node_version()? {
        NodeVersion::V0_20 => {
            v0_20_0::get_transaction_cost(transaction.as_ref(), block_hash, online_client).await
        }

        NodeVersion::V0_21 => {
            v0_21_0::get_transaction_cost(transaction.as_ref(), block_hash, online_client).await
        }

        NodeVersion::V0_22 => {
            v0_22_0::get_transaction_cost(transaction.as_ref(), block_hash, online_client).await
        }
    }
}

/// Get D-Parameter depending on the given protocol version.
pub async fn get_d_parameter(
    block_hash: BlockHash,
    protocol_version: ProtocolVersion,
    online_client: &OnlineClient<SubstrateConfig>,
) -> Result<DParameter, SubxtNodeError> {
    match protocol_version.node_version()? {
        NodeVersion::V0_20 => v0_20_0::get_d_parameter(block_hash, online_client).await,
        NodeVersion::V0_21 => v0_21_0::get_d_parameter(block_hash, online_client).await,
        NodeVersion::V0_22 => v0_22_0::get_d_parameter(block_hash, online_client).await,
    }
}

/// Fetch genesis cNight registrations from pallet storage.
/// At genesis, Substrate does not emit events (Parity PR #5463), so we query
/// the cNightObservation.Mappings storage directly at block 0.
pub async fn fetch_genesis_cnight_registrations(
    block_hash: BlockHash,
    protocol_version: ProtocolVersion,
    online_client: &OnlineClient<SubstrateConfig>,
) -> Result<Vec<DustRegistrationEvent>, SubxtNodeError> {
    match protocol_version.node_version()? {
        NodeVersion::V0_20 => {
            v0_20_0::fetch_genesis_cnight_registrations(block_hash, online_client).await
        }

        NodeVersion::V0_21 => {
            v0_21_0::fetch_genesis_cnight_registrations(block_hash, online_client).await
        }

        NodeVersion::V0_22 => {
            v0_22_0::fetch_genesis_cnight_registrations(block_hash, online_client).await
        }
    }
}

/// Get Terms and Conditions depending on the given protocol version.
pub async fn get_terms_and_conditions(
    block_hash: BlockHash,
    protocol_version: ProtocolVersion,
    online_client: &OnlineClient<SubstrateConfig>,
) -> Result<Option<TermsAndConditions>, SubxtNodeError> {
    match protocol_version.node_version()? {
        NodeVersion::V0_20 => v0_20_0::get_terms_and_conditions(block_hash, online_client).await,
        NodeVersion::V0_21 => v0_21_0::get_terms_and_conditions(block_hash, online_client).await,
        NodeVersion::V0_22 => v0_22_0::get_terms_and_conditions(block_hash, online_client).await,
    }
}
