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

//! GraphQL types for the `Contract` top-level type and related structures.

use crate::{
    domain::storage::Storage,
    infra::api::{
        ApiResult, ContextExt, OptionExt, ResultExt,
        v4::{
            contract_action::ContractAction,
            HexEncoded,
        },
    },
};
use async_graphql::{ComplexObject, Context, Description, Enum, SimpleObject};

use indexer_common::domain::{
    ledger::{ContractState, MaintenanceAuthority as DomainMaintenanceAuthority},
    LedgerVersion,
};

/// Kind of a contract maintenance verifying key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum, Description)]
pub enum ContractMaintenanceVerifyingKeyKind {
    /// Schnorr signing key.
    Schnorr,
    /// ECDSA signing key.
    Ecdsa,
}

/// A committee member's verifying key in a contract's maintenance authority.
#[derive(Debug, Clone, SimpleObject, Description)]
pub struct ContractMaintenanceVerifyingKey {
    /// The kind of the verifying key (Schnorr or ECDSA).
    pub kind: ContractMaintenanceVerifyingKeyKind,
    /// The serialized verifying key bytes, hex-encoded.
    pub key: HexEncoded,
}

impl From<indexer_common::domain::ledger::CommitteeMember> for ContractMaintenanceVerifyingKey {
    fn from(member: indexer_common::domain::ledger::CommitteeMember) -> Self {
        Self {
            kind: match member.kind {
                indexer_common::domain::ledger::VerifyingKeyKind::Schnorr => {
                    ContractMaintenanceVerifyingKeyKind::Schnorr
                }
                indexer_common::domain::ledger::VerifyingKeyKind::Ecdsa => {
                    ContractMaintenanceVerifyingKeyKind::Ecdsa
                }
            },
            key: HexEncoded(const_hex::encode(&member.verifying_key)),
        }
    }
}

/// The maintenance authority of a contract.
#[derive(Debug, Clone, SimpleObject, Description)]
pub struct ContractMaintenanceAuthority {
    /// The committee members' verifying keys.
    pub committee: Vec<ContractMaintenanceVerifyingKey>,
    /// The threshold for the multisig.
    pub threshold: i32,
    /// The counter for the multisig.
    pub counter: i32,
}

impl From<DomainMaintenanceAuthority> for ContractMaintenanceAuthority {
    fn from(authority: DomainMaintenanceAuthority) -> Self {
        Self {
            committee: authority
                .committee
                .into_iter()
                .map(ContractMaintenanceVerifyingKey::from)
                .collect(),
            threshold: authority.threshold as i32,
            counter: authority.counter as i32,
        }
    }
}

/// The contract as the topmost concept, with point-in-time state, maintenance authority,
/// and a bounded recent-actions sub-query.
///
/// Use the `contract(address, offset)` query to retrieve a `Contract`.
#[derive(Debug, SimpleObject)]
#[graphql(complex)]
pub struct Contract<S>
where
    S: Storage,
{
    /// The contract address (hex-encoded).
    address: HexEncoded,
    /// The contract address (domain type, used for storage queries).
    #[graphql(skip)]
    address_bytes: indexer_common::domain::SerializedContractAddress,
    /// The serialized contract state (hex-encoded, derived from the resolving contract action).
    #[graphql(skip)]
    state: indexer_common::domain::SerializedContractState,
    /// The block height used to resolve the as-of state.
    #[graphql(skip)]
    block_height: u32,
    #[graphql(skip)]
    _s: std::marker::PhantomData<S>,
}

impl<S> Contract<S>
where
    S: Storage,
{
    /// Construct a `Contract` from the resolving contract action and block height.
    pub fn new(contract_action: crate::domain::ContractAction, block_height: u32) -> Self {
        let addr_bytes = contract_action.address.clone();
        Self {
            address: HexEncoded(const_hex::encode(addr_bytes.as_ref())),
            address_bytes: addr_bytes,
            state: contract_action.state,
            block_height,
            _s: std::marker::PhantomData,
        }
    }
}

#[ComplexObject]
impl<S> Contract<S>
where
    S: Storage,
{
    /// The serialized contract state as of the queried block offset.
    /// This is the `state` blob of the latest contract action at or before the offset.
    async fn state(&self) -> HexEncoded {
        HexEncoded(const_hex::encode(self.state.as_ref()))
    }

    /// The maintenance authority of the contract, derived by deserializing the `state` blob.
    async fn maintenance_authority(&self, _cx: &Context<'_>) -> ApiResult<Option<ContractMaintenanceAuthority>> {
        let state_bytes = self.state.as_ref();

        // Use the latest ledger version for deserialization.
        // V8 and V9 states are stored separately; using LATEST (V9) for any state
        //blob will return an error for V8 states, which is acceptable since V8 is
        //deprecated and V9 is the current ledger version.
        match ContractState::deserialize(state_bytes, LedgerVersion::LATEST) {
            Ok(contract_state) => {
                Ok(contract_state
                    .maintenance_authority()
                    .map(ContractMaintenanceAuthority::from))
            }
            Err(error) => {
                log::warn!("failed to deserialize contract state for maintenance authority: {}", error);
                Ok(None)
            }
        }
    }

    /// Recent contract actions for this contract, ordered by action ID descending.
    ///
    /// Use `limit` to bound the number of results (default 20, max 100).
    async fn actions(
        &self,
        cx: &Context<'_>,
        limit: Option<i32>,
    ) -> ApiResult<Vec<ContractAction<S>>> {
        let storage = cx.get_storage::<S>();
        let limit = limit.unwrap_or(20).max(1).min(100) as u32;

        let actions = storage
            .get_recent_contract_actions_by_address(&self.address_bytes, None, limit)
            .await
            .map_err_into_server_error(|| {
                format!(
                    "get recent contract actions for {}",
                    const_hex::encode(self.address.as_ref())
                )
            })?;

        Ok(actions.into_iter().map(|a| a.into()).collect())
    }
}
