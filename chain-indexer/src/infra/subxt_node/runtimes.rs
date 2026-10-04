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

mod v0_22_0;
mod v1_0_300;
mod v2_0_0;
mod v2_1_0;

// To see how this is generated, look in build.rs
include!(concat!(env!("OUT_DIR"), "/generated_runtime.rs"));

use crate::{
    domain::{DParameter, DustRegistrationEvent, TermsAndConditions},
    infra::subxt_node::SubxtNodeError,
};
use indexer_common::{
    domain::{ByteVec, NodeVersion},
    error::BoxError,
};
use parity_scale_codec::Decode;
use subxt::{
    SubstrateConfig,
    client::{ClientAtBlock, OfflineClientAtBlockT},
    ext::{frame_decode, scale_decode::IntoVisitor},
    runtime_apis::Payload,
    storage::Address,
};

/// Runtime specific block details.
pub struct BlockDetails {
    pub timestamp: Option<u64>,
    pub transactions: Vec<Transaction>,
    pub dust_registration_events: Vec<DustRegistrationEvent>,
    /// c2m-bridge events. Only populated for node 2.0+, where the
    /// `c2m-bridge` pallet exists in the runtime metadata. Empty for earlier
    /// node versions (the pallet did not yet exist there).
    pub bridge_events: Vec<indexer_common::domain::bridge::BridgeEvent>,
}

/// Runtime specific (serialized) transaction.
#[derive(Debug)]
#[cfg_attr(test, derive(Clone))]
pub enum Transaction {
    Regular(ByteVec),
    System(ByteVec),
}

impl Transaction {
    /// The serialized transaction.
    pub fn bytes(&self) -> &ByteVec {
        let (Self::Regular(bytes) | Self::System(bytes)) = self;
        bytes
    }
}

/// Decode block details from a block's serialized extrinsics and its serialized `System.Events`
/// value, against the given client's metadata.
pub async fn decode_block_details<C: OfflineClientAtBlockT<SubstrateConfig>>(
    node_version: NodeVersion,
    client: &ClientAtBlock<SubstrateConfig, C>,
    extrinsics: Vec<Vec<u8>>,
    events: Vec<u8>,
) -> Result<BlockDetails, SubxtNodeError> {
    match node_version {
        NodeVersion::V0_22 => v0_22_0::decode_block_details(client, extrinsics, events).await,
        NodeVersion::V1_0 => v1_0_300::decode_block_details(client, extrinsics, events).await,
        NodeVersion::V2_0 => v2_0_0::decode_block_details(client, extrinsics, events).await,
        NodeVersion::V2_1 => v2_1_0::decode_block_details(client, extrinsics, events).await,
    }
}

/// Decode an Aura authority set, a SCALE-encoded sequence of 32-byte public keys. The encoding is
/// fixed by `sp_consensus_aura`, so it is the same in every runtime.
pub fn decode_authorities(mut authorities: &[u8]) -> Result<Vec<[u8; 32]>, SubxtNodeError> {
    Ok(Vec::<[u8; 32]>::decode(&mut authorities)?)
}

/// Decode slot depending on the given protocol version.
pub fn decode_slot(slot: &[u8], node_version: NodeVersion) -> Result<u64, SubxtNodeError> {
    match node_version {
        NodeVersion::V0_22 => v0_22_0::decode_slot(slot),
        NodeVersion::V1_0 => v1_0_300::decode_slot(slot),
        NodeVersion::V2_0 => v2_0_0::decode_slot(slot),
        NodeVersion::V2_1 => v2_1_0::decode_slot(slot),
    }
}

/// Decode the serialized result of the `get_zswap_state_root` runtime API call.
pub fn decode_zswap_merkle_tree_root<C: OfflineClientAtBlockT<SubstrateConfig>>(
    node_version: NodeVersion,
    client: &ClientAtBlock<SubstrateConfig, C>,
    result: &[u8],
) -> Result<Vec<u8>, SubxtNodeError> {
    match node_version {
        NodeVersion::V0_22 => v0_22_0::decode_zswap_merkle_tree_root(client, result),
        NodeVersion::V1_0 => v1_0_300::decode_zswap_merkle_tree_root(client, result),
        NodeVersion::V2_0 => v2_0_0::decode_zswap_merkle_tree_root(client, result),
        NodeVersion::V2_1 => v2_1_0::decode_zswap_merkle_tree_root(client, result),
    }
}

/// Decode the serialized result of the `get_ledger_state_root` runtime API call.
pub fn decode_ledger_state_root<C: OfflineClientAtBlockT<SubstrateConfig>>(
    node_version: NodeVersion,
    client: &ClientAtBlock<SubstrateConfig, C>,
    result: &[u8],
) -> Result<Option<Vec<u8>>, SubxtNodeError> {
    match node_version {
        NodeVersion::V0_22 => v0_22_0::decode_ledger_state_root(client, result),
        NodeVersion::V1_0 => v1_0_300::decode_ledger_state_root(client, result),
        NodeVersion::V2_0 => v2_0_0::decode_ledger_state_root(client, result),
        NodeVersion::V2_1 => v2_1_0::decode_ledger_state_root(client, result),
    }
}

/// Decode the serialized result of the `get_d_parameter` runtime API call.
pub fn decode_d_parameter<C: OfflineClientAtBlockT<SubstrateConfig>>(
    node_version: NodeVersion,
    client: &ClientAtBlock<SubstrateConfig, C>,
    result: &[u8],
) -> Result<DParameter, SubxtNodeError> {
    match node_version {
        NodeVersion::V0_22 => v0_22_0::decode_d_parameter(client, result),
        NodeVersion::V1_0 => v1_0_300::decode_d_parameter(client, result),
        NodeVersion::V2_0 => v2_0_0::decode_d_parameter(client, result),
        NodeVersion::V2_1 => v2_1_0::decode_d_parameter(client, result),
    }
}

/// Decode genesis cNight registrations from the serialized key-value pairs of the cNight
/// observation pallet's mapping storage.
pub fn decode_genesis_cnight_registrations<C: OfflineClientAtBlockT<SubstrateConfig>>(
    node_version: NodeVersion,
    client: &ClientAtBlock<SubstrateConfig, C>,
    mappings: &[(Vec<u8>, Vec<u8>)],
) -> Result<Vec<DustRegistrationEvent>, SubxtNodeError> {
    match node_version {
        NodeVersion::V0_22 => v0_22_0::decode_genesis_cnight_registrations(client, mappings),
        NodeVersion::V1_0 => v1_0_300::decode_genesis_cnight_registrations(client, mappings),
        NodeVersion::V2_0 => v2_0_0::decode_genesis_cnight_registrations(client, mappings),
        NodeVersion::V2_1 => v2_1_0::decode_genesis_cnight_registrations(client, mappings),
    }
}

/// Decode the serialized result of the `get_terms_and_conditions` runtime API call.
pub fn decode_terms_and_conditions<C: OfflineClientAtBlockT<SubstrateConfig>>(
    node_version: NodeVersion,
    client: &ClientAtBlock<SubstrateConfig, C>,
    result: &[u8],
) -> Result<Option<TermsAndConditions>, SubxtNodeError> {
    match node_version {
        NodeVersion::V0_22 => v0_22_0::decode_terms_and_conditions(client, result),
        NodeVersion::V1_0 => v1_0_300::decode_terms_and_conditions(client, result),
        NodeVersion::V2_0 => v2_0_0::decode_terms_and_conditions(client, result),
        NodeVersion::V2_1 => v2_1_0::decode_terms_and_conditions(client, result),
    }
}

/// Decode a runtime API call's serialized result against the client's metadata.
fn decode_call_result<P: Payload, C: OfflineClientAtBlockT<SubstrateConfig>>(
    client: &ClientAtBlock<SubstrateConfig, C>,
    payload: &P,
    mut result: &[u8],
) -> Result<P::ReturnType, BoxError> {
    let metadata = client.metadata_ref();
    let value = frame_decode::runtime_apis::decode_runtime_api_response(
        payload.trait_name(),
        payload.method_name(),
        &mut result,
        metadata,
        metadata.types(),
        P::ReturnType::into_visitor(),
    )
    .map_err(|error| format!("{error:?}"))?;

    Ok(value)
}

/// Decode a serialized storage value of the given storage entry against the client's metadata.
fn decode_storage_value<A: Address, C: OfflineClientAtBlockT<SubstrateConfig>>(
    client: &ClientAtBlock<SubstrateConfig, C>,
    address: &A,
    mut value: &[u8],
) -> Result<A::Value, BoxError> {
    let metadata = client.metadata_ref();
    let value = frame_decode::storage::decode_storage_value(
        address.pallet_name(),
        address.entry_name(),
        &mut value,
        metadata,
        metadata.types(),
        A::Value::into_visitor(),
    )
    .map_err(|error| format!("{error:?}"))?;

    Ok(value)
}

/// Decode the key parts of a serialized storage key of the given storage entry against the
/// client's metadata.
fn decode_storage_key<A: Address, C: OfflineClientAtBlockT<SubstrateConfig>>(
    client: &ClientAtBlock<SubstrateConfig, C>,
    address: &A,
    key: &[u8],
) -> Result<A::KeyParts, BoxError> {
    let metadata = client.metadata_ref();
    let decoded_key = frame_decode::storage::decode_storage_key(
        address.pallet_name(),
        address.entry_name(),
        &mut &*key,
        metadata,
        metadata.types(),
    )
    .map_err(|error| format!("{error:?}"))?;
    let key_parts =
        frame_decode::storage::decode_storage_key_values(key, &decoded_key, metadata.types())
            .map_err(|error| format!("{error:?}"))?;

    Ok(key_parts)
}
