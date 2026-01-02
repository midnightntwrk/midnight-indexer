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

//! System parameters domain types for chain-indexer.

use indexer_common::domain::{BlockHash, TcDocumentHash};

/// D-Parameter from the node RPC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DParameter {
    pub num_permissioned_candidates: u16,
    pub num_registered_candidates: u16,
}

/// Terms and Conditions from the node RPC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermsAndConditions {
    pub hash: TcDocumentHash,
    pub url: String,
}

/// System parameters change detected during block processing.
#[derive(Debug, Clone)]
pub struct SystemParametersChange {
    pub block_height: u32,
    pub block_hash: BlockHash,
    pub timestamp: u64,
    pub d_parameter: Option<DParameter>,
    pub terms_and_conditions: Option<TermsAndConditions>,
}

// TODO(PM-21070): Uncomment when integrating with node that has system-parameters pallet.
/*
use serde::Deserialize;

/// RPC response for systemParameters_getDParameter.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DParameterRpcResponse {
    pub num_permissioned_candidates: u16,
    pub num_registered_candidates: u16,
}

impl From<DParameterRpcResponse> for DParameter {
    fn from(rpc: DParameterRpcResponse) -> Self {
        DParameter {
            num_permissioned_candidates: rpc.num_permissioned_candidates,
            num_registered_candidates: rpc.num_registered_candidates,
        }
    }
}

/// RPC response for systemParameters_getTermsAndConditions.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TermsAndConditionsRpcResponse {
    pub hash: String,
    pub url: String,
}

impl TryFrom<TermsAndConditionsRpcResponse> for TermsAndConditions {
    type Error = hex::FromHexError;

    fn try_from(rpc: TermsAndConditionsRpcResponse) -> Result<Self, Self::Error> {
        let hash_str = rpc.hash.strip_prefix("0x").unwrap_or(&rpc.hash);
        let hash_bytes = hex::decode(hash_str)?;
        let hash = TcDocumentHash::try_from(hash_bytes)
            .map_err(|_| hex::FromHexError::InvalidStringLength)?;

        Ok(TermsAndConditions {
            hash,
            url: rpc.url,
        })
    }
}
*/
