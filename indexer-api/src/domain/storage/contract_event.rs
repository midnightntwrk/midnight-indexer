// This file is part of midnight-indexer.
// Copyright (C) Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::domain::{ContractEventRow, storage::NoopStorage};
use futures::{Stream, stream};
use std::num::NonZeroU32;

/// Filter that the chain-indexer storage layer accepts for both query and
/// subscription paths. Mirrors the GraphQL `ContractEventFilter` input.
#[derive(Debug, Clone)]
pub struct ContractEventFilter {
    /// Required: the emitting contract address.
    pub contract_address: Vec<u8>,
    /// Optional: filter by event variant name (per `LEDGER_EVENT_VARIANT`).
    /// Empty/None means no type filter; non-empty narrows to those variants.
    pub variants: Option<Vec<&'static str>>,
    /// Optional: prefix-match on indexed-field rows in `contract_event_indexed_fields`.
    /// AND semantics across multiple entries.
    pub field_prefixes: Vec<FieldPrefix>,
    /// Optional: lower bound on block height.
    pub from_block: Option<u32>,
    /// Optional: upper bound on block height.
    pub to_block: Option<u32>,
    /// Optional: narrow to events emitted from transactions with this hash.
    pub transaction_hash: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct FieldPrefix {
    pub field_name: String,
    pub prefix: Vec<u8>,
}

#[trait_variant::make(Send)]
pub trait ContractEventStorage
where
    Self: Clone + Send + Sync + 'static,
{
    /// Paginated query: returns contract events matching the filter, with an
    /// optional limit/offset window for pagination. Ordered by `id` ascending.
    async fn get_contract_events(
        &self,
        filter: ContractEventFilter,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<ContractEventRow>, sqlx::Error>;

    /// Streaming subscription: returns contract events matching the filter,
    /// starting at the given event `id` (inclusive). On `filter.to_block`, the
    /// stream stops once the chain has advanced past that block — see
    /// `dust_nullifier_transactions` / `shielded_nullifier_transactions`
    /// bounded-subscription pattern.
    async fn get_contract_events_after_id(
        &self,
        filter: ContractEventFilter,
        after_id: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<ContractEventRow, sqlx::Error>> + Send;

    /// Batched lookup keyed on `contract_action_id` for the
    /// `ContractCall.contractEvents` nested field DataLoader (#1162).
    /// Returns `(contract_action_id, ContractEventRow)` pairs covering every
    /// requested key; pre-#1162 (no contract_action_id column populated) this
    /// returns an empty Vec.
    async fn get_contract_events_by_contract_action_ids(
        &self,
        ids: &[u64],
    ) -> Result<Vec<(u64, ContractEventRow)>, sqlx::Error>;
}

#[allow(unused_variables)]
impl ContractEventStorage for NoopStorage {
    async fn get_contract_events(
        &self,
        filter: ContractEventFilter,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<ContractEventRow>, sqlx::Error> {
        Ok(vec![])
    }

    async fn get_contract_events_after_id(
        &self,
        filter: ContractEventFilter,
        after_id: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<ContractEventRow, sqlx::Error>> + Send {
        stream::empty()
    }

    async fn get_contract_events_by_contract_action_ids(
        &self,
        ids: &[u64],
    ) -> Result<Vec<(u64, ContractEventRow)>, sqlx::Error> {
        Ok(vec![])
    }
}
