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
mod v1_0_0;

// To see how this is generated, look in build.rs
include!(concat!(env!("OUT_DIR"), "/generated_runtime.rs"));

use crate::{
    domain::{DParameter, DustRegistrationEvent, TermsAndConditions},
    infra::subxt_node::{OnlineClientAtBlock, SubxtNodeError},
};
use indexer_common::domain::{
    ByteVec, NodeVersion, SerializedContractAddress, SerializedContractState,
};

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
    authorities: &mut Option<Vec<[u8; 32]>>,
    node_version: NodeVersion,
    block: &OnlineClientAtBlock,
) -> Result<BlockDetails, SubxtNodeError> {
    // TODO Replace this often repeated pattern with a macro?
    match node_version {
        NodeVersion::V0_22 => v0_22_0::make_block_details(authorities, block).await,
        NodeVersion::V1_0 => v1_0_0::make_block_details(authorities, block).await,
    }
}

/// Fetch authorities depending on the given protocol version.
pub async fn fetch_authorities(
    node_version: NodeVersion,
    block: &OnlineClientAtBlock,
) -> Result<Vec<[u8; 32]>, SubxtNodeError> {
    match node_version {
        NodeVersion::V0_22 => v0_22_0::fetch_authorities(block).await,
        NodeVersion::V1_0 => v1_0_0::fetch_authorities(block).await,
    }
}

/// Decode slot depending on the given protocol version.
pub fn decode_slot(slot: &[u8], node_version: NodeVersion) -> Result<u64, SubxtNodeError> {
    match node_version {
        NodeVersion::V0_22 => v0_22_0::decode_slot(slot),
        NodeVersion::V1_0 => v1_0_0::decode_slot(slot),
    }
}

/// Get contract state depending on the given protocol version.
///
/// Hotfix (mainnet incident 2026-07-20): at a runtime-upgrade *enactment* block the `MNSV`
/// protocol-version header digest lags the block's live runtime by one block (the block is
/// authored under the previous runtime while the new `:code` is already active in its state, which
/// is what runtime-API calls execute against). The digest-selected module is therefore one version
/// behind, so subxt rejects the call with "the static Runtime API address used is not compatible
/// with the live chain" — but only when the enactment block contains a contract action (mainnet
/// block 1,774,491; preprod's enactment block had none, which is why it was unaffected).
///
/// subxt validates each static module against the block's live metadata, so at most one module can
/// succeed for a given block. We therefore try the digest-selected module first (the fast path, no
/// behavior change for ordinary blocks) and, only on failure, fall back to the other module(s);
/// this changes behavior solely at an enactment block. The durable fix is to select the module from
/// the block's live runtime version rather than the header digest.
pub async fn get_contract_state(
    address: SerializedContractAddress,
    node_version: NodeVersion,
    block: &OnlineClientAtBlock,
) -> Result<SerializedContractState, SubxtNodeError> {
    // Fast path: the module selected from the block's protocol-version digest.
    let result = get_contract_state_for(node_version, address.clone(), block).await;
    if result.is_ok() {
        return result;
    }

    // Boundary fallback: retry with the other runtime module(s). Only the module whose static
    // metadata matches the block's live runtime passes subxt's compatibility check, so a success
    // here is the correct state and any real (e.g. transient) failure still surfaces below.
    for version in [NodeVersion::V0_22, NodeVersion::V1_0] {
        if version == node_version {
            continue;
        }
        if let Ok(state) = get_contract_state_for(version, address.clone(), block).await {
            return Ok(state);
        }
    }

    result
}

async fn get_contract_state_for(
    node_version: NodeVersion,
    address: SerializedContractAddress,
    block: &OnlineClientAtBlock,
) -> Result<SerializedContractState, SubxtNodeError> {
    match node_version {
        NodeVersion::V0_22 => v0_22_0::get_contract_state(address, block).await,
        NodeVersion::V1_0 => v1_0_0::get_contract_state(address, block).await,
    }
}

pub async fn get_zswap_merkle_tree_root(
    node_version: NodeVersion,
    block: &OnlineClientAtBlock,
) -> Result<Vec<u8>, SubxtNodeError> {
    match node_version {
        NodeVersion::V0_22 => v0_22_0::get_zswap_merkle_tree_root(block).await,
        NodeVersion::V1_0 => v1_0_0::get_zswap_merkle_tree_root(block).await,
    }
}

/// Get the pure ledger state root (without StorableLedgerState wrapping) at the given block.
pub async fn get_ledger_state_root(
    node_version: NodeVersion,
    block: &OnlineClientAtBlock,
) -> Result<Option<Vec<u8>>, SubxtNodeError> {
    match node_version {
        NodeVersion::V0_22 => v0_22_0::get_ledger_state_root(block).await,
        NodeVersion::V1_0 => v1_0_0::get_ledger_state_root(block).await,
    }
}

/// Get D-Parameter depending on the given protocol version.
pub async fn get_d_parameter(
    node_version: NodeVersion,
    block: &OnlineClientAtBlock,
) -> Result<DParameter, SubxtNodeError> {
    match node_version {
        NodeVersion::V0_22 => v0_22_0::get_d_parameter(block).await,
        NodeVersion::V1_0 => v1_0_0::get_d_parameter(block).await,
    }
}

/// Fetch genesis cNight registrations from pallet storage.
/// At genesis, Substrate does not emit events (Parity PR #5463), so we query
/// the cNightObservation.Mappings storage directly at block 0.
pub async fn fetch_genesis_cnight_registrations(
    node_version: NodeVersion,
    block: &OnlineClientAtBlock,
) -> Result<Vec<DustRegistrationEvent>, SubxtNodeError> {
    match node_version {
        NodeVersion::V0_22 => v0_22_0::fetch_genesis_cnight_registrations(block).await,
        NodeVersion::V1_0 => v1_0_0::fetch_genesis_cnight_registrations(block).await,
    }
}

/// Get Terms and Conditions depending on the given protocol version.
pub async fn get_terms_and_conditions(
    node_version: NodeVersion,
    block: &OnlineClientAtBlock,
) -> Result<Option<TermsAndConditions>, SubxtNodeError> {
    match node_version {
        NodeVersion::V0_22 => v0_22_0::get_terms_and_conditions(block).await,
        NodeVersion::V1_0 => v1_0_0::get_terms_and_conditions(block).await,
    }
}
