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

mod query;

use crate::{
    domain::{self, AsBytesExt, HexEncoded, Storage, ZswapStateCache},
    infra::api::v2::query::Query,
};
use async_graphql::{EmptyMutation, EmptySubscription, OneofObject, Schema, SimpleObject, scalar};
use async_graphql_axum::{GraphQL, GraphQLSubscription};
use axum::{Router, routing::post_service};
use derive_more::Debug;
use indexer_common::domain::{NetworkId, ProtocolVersion, Subscriber, ZswapStateStorage};
use serde::{Deserialize, Serialize};
use std::{
    marker::PhantomData,
    sync::{Arc, atomic::AtomicBool},
};

/// A transaction with its relevant data.
#[derive(Debug, Clone, SimpleObject)]
struct Transaction<S>
where
    S: Storage,
{
    /// The transaction hash.
    hash: HexEncoded,

    /// The protocol version.
    protocol_version: u32,

    /// The transaction apply stage.
    apply_stage: ApplyStage,

    /// The transaction identifiers.
    #[debug(skip)]
    identifiers: Vec<HexEncoded>,

    /// The raw transaction content.
    #[debug(skip)]
    raw: HexEncoded,

    /// The merkle-tree root.
    #[debug(skip)]
    merkle_tree_root: HexEncoded,

    #[graphql(skip)]
    id: u64,

    #[graphql(skip)]
    block_hash: HexEncoded,

    #[graphql(skip)]
    #[debug(skip)]
    _s: PhantomData<S>,
}

// #[ComplexObject]
// impl<S> Transaction<S>
// where
//     S: Storage,
// {
//     /// The block for this transaction.
//     async fn block(&self, cx: &Context<'_>) -> async_graphql::Result<Block<S>> {
//         Query::<S>::default()
//             .block(cx, Some(BlockOffset::Hash(self.block_hash.to_owned())))
//             .await?
//             .ok_or_else(|| {
//                 async_graphql::Error::new(format!(
//                     "no block for tx {:?} with block hash {:?}",
//                     self.hash, self.block_hash
//                 ))
//             })
//     }

//     /// The contract actions.
//     async fn contract_actions(
//         &self,
//         cx: &Context<'_>,
//     ) -> async_graphql::Result<Vec<ContractAction<S>>> {
//         let contract_actions = cx
//             .get_storage::<S>()
//             .get_contract_actions_by_transaction_id(self.id)
//             .await
//             .internal("cannot get contract actions by transactions id")?;

//         Ok(contract_actions.into_iter().map(Into::into).collect())
//     }
// }

impl<S> From<domain::Transaction> for Transaction<S>
where
    S: Storage,
{
    fn from(value: domain::Transaction) -> Self {
        let domain::Transaction {
            id,
            hash,
            block_hash,
            protocol_version: ProtocolVersion(protocol_version),
            apply_stage,
            identifiers,
            raw,
            merkle_tree_root,
            ..
        } = value;

        Self {
            hash: hash.hex_encode(),
            protocol_version,
            apply_stage: apply_stage.into(),
            identifiers: identifiers
                .into_iter()
                .map(|identifier| identifier.hex_encode())
                .collect::<Vec<_>>(),
            raw: raw.hex_encode(),
            merkle_tree_root: merkle_tree_root.hex_encode(),
            id,
            block_hash: block_hash.hex_encode(),
            _s: PhantomData,
        }
    }
}

impl<S> From<&Transaction<S>> for Transaction<S>
where
    S: Storage,
{
    fn from(value: &Transaction<S>) -> Self {
        value.to_owned()
    }
}

/// The apply stage of a transaction: Success, PartialSuccess (only guaranteed coins) or Failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyStage(indexer_common::domain::ApplyStage);

scalar!(ApplyStage);

impl From<indexer_common::domain::ApplyStage> for ApplyStage {
    fn from(apply_stage: indexer_common::domain::ApplyStage) -> Self {
        Self(apply_stage)
    }
}

/// Either a hash or an identifier to query transactions.
#[derive(Debug, OneofObject)]
enum TransactionOffset {
    Hash(HexEncoded),
    Identifier(HexEncoded),
}

pub fn make_app<S, B, Z>(
    network_id: NetworkId,
    zswap_state_cache: ZswapStateCache,
    storage: S,
    zswap_state_storage: Z,
    subscriber: B,
    max_complexity: usize,
    max_depth: usize,
) -> Router<Arc<AtomicBool>>
where
    S: Storage,
    B: Subscriber,
    Z: ZswapStateStorage,
{
    let schema = Schema::build(Query::<S>::default(), EmptyMutation, EmptySubscription)
        .data(network_id)
        .data(zswap_state_cache)
        .data(storage)
        .data(zswap_state_storage)
        .data(subscriber)
        .limit_complexity(max_complexity)
        .limit_depth(max_depth)
        .limit_recursive_depth(max_depth)
        .finish();

    Router::new()
        .route("/graphql", post_service(GraphQL::new(schema.clone())))
        .route_service("/graphql/ws", GraphQLSubscription::new(schema))
}
