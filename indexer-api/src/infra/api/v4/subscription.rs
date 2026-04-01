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

mod block;
mod contract_action;
mod dust_generations;
mod dust_ledger_events;
mod dust_nullifier_transactions;
mod shielded;
mod unshielded;
mod zswap_ledger_events;

use crate::{
    domain::{self, storage::Storage},
    infra::api::v4::{
        HexEncodable, HexEncoded,
        subscription::{
            block::BlockSubscription, contract_action::ContractActionSubscription,
            dust_generations::DustGenerationsSubscription,
            dust_ledger_events::DustLedgerEventsSubscription,
            dust_nullifier_transactions::DustNullifierTransactionsSubscription,
            shielded::ShieldedTransactionsSubscription,
            unshielded::UnshieldedTransactionsSubscription,
            zswap_ledger_events::ZswapLedgerEventsSubscription,
        },
    },
};
use async_graphql::{MergedSubscription, SimpleObject};
use derive_more::Debug;
use indexer_common::domain::Subscriber;

/// A collapsed merkle tree update shared across subscriptions.
#[derive(Debug, Clone, SimpleObject)]
pub struct CollapsedMerkleTree {
    /// The start index.
    pub start_index: u64,

    /// The end index.
    pub end_index: u64,

    /// The hex-encoded value.
    #[debug(skip)]
    pub update: HexEncoded,

    /// The protocol version.
    pub protocol_version: u32,
}

impl From<domain::MerkleTreeCollapsedUpdate> for CollapsedMerkleTree {
    fn from(value: domain::MerkleTreeCollapsedUpdate) -> Self {
        let domain::MerkleTreeCollapsedUpdate {
            start_index,
            end_index,
            update,
            protocol_version,
        } = value;

        Self {
            start_index,
            end_index,
            update: update.hex_encode(),
            protocol_version: protocol_version.into(),
        }
    }
}

#[derive(MergedSubscription)]
pub struct Subscription<S, B>(
    BlockSubscription<S, B>,
    ContractActionSubscription<S, B>,
    DustGenerationsSubscription<S, B>,
    DustLedgerEventsSubscription<S, B>,
    DustNullifierTransactionsSubscription<S, B>,
    ShieldedTransactionsSubscription<S, B>,
    UnshieldedTransactionsSubscription<S, B>,
    ZswapLedgerEventsSubscription<S, B>,
)
where
    S: Storage,
    B: Subscriber;

impl<S, B> Default for Subscription<S, B>
where
    S: Storage,
    B: Subscriber,
{
    fn default() -> Self {
        Subscription(
            BlockSubscription::default(),
            ContractActionSubscription::default(),
            DustGenerationsSubscription::default(),
            DustLedgerEventsSubscription::default(),
            DustNullifierTransactionsSubscription::default(),
            ShieldedTransactionsSubscription::default(),
            UnshieldedTransactionsSubscription::default(),
            ZswapLedgerEventsSubscription::default(),
        )
    }
}
