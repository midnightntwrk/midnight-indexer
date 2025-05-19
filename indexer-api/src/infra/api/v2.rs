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

// mod mutation;
// mod query;
// mod subscription;

// use crate::{
//     domain::{self, AsBytesExt, HexEncoded, NoopStorage, Storage, ZswapStateCache},
//     infra::api::{
//         v2::{mutation::Mutation, query::Query, subscription::Subscription},
//         ContextExt,
//     },
// };
// use anyhow::Context as AnyhowContext;
// use async_graphql::{
//     scalar, ComplexObject, Context, Interface, OneofObject, Schema, SchemaBuilder, SimpleObject,
//     Union,
// };
// use async_graphql_axum::{GraphQL, GraphQLSubscription};
// use axum::{routing::post_service, Router};
// use derive_more::derive::From;
// use indexer_common::{
//     domain::{NetworkId, NoopSubscriber, ProtocolVersion, Subscriber},
//     error::StdErrorExt,
// };
// use serde::{Deserialize, Serialize};
// use std::marker::PhantomData;
// use tracing::error;

// /// A block with its relevant data.
// #[derive(Debug, SimpleObject)]
// #[graphql(complex)]
// struct Block<S>
// where
//     S: Storage,
// {
//     /// The block hash.
//     hash: HexEncoded,

//     /// The block height (number).
//     height: u32,

//     /// The protocol version.
//     protocol_version: u32,

//     /// The UNIX timestamp.
//     timestamp: u64,

//     /// The block author.
//     author: Option<HexEncoded>,

//     /// The transactions.
//     transactions: Vec<Transaction<S>>,

//     #[graphql(skip)]
//     parent_hash: HexEncoded,

//     #[graphql(skip)]
//     _s: PhantomData<S>,
// }

// #[ComplexObject]
// impl<S> Block<S>
// where
//     S: Storage,
// {
//     /// The parent of this block.
//     async fn parent(&self, cx: &Context<'_>) -> async_graphql::Result<Option<Block<S>>> {
//         let storage = cx.get_storage::<S>()?;

//         let parent_hash = self
//             .parent_hash
//             .hex_decode()
//             .context("decode parent hash")?;
//         let block = storage
//             .get_block_by_hash(&parent_hash)
//             .await
//             .context("get block by hash")?;

//         Ok(block.map(Into::into))
//     }
// }

// impl<S> From<domain::Block> for Block<S>
// where
//     S: Storage,
// {
//     fn from(value: domain::Block) -> Self {
//         let domain::Block {
//             hash,
//             height,
//             protocol_version: ProtocolVersion(protocol_version),
//             author,
//             timestamp,
//             transactions,
//             parent_hash,
//             ..
//         } = value;

//         Block {
//             hash: hash.hex_encode(),
//             height,
//             protocol_version,
//             author: author.map(|author| author.hex_encode()),
//             timestamp,
//             transactions: transactions.into_iter().map(Into::into).collect::<Vec<_>>(),
//             parent_hash: parent_hash.hex_encode(),
//             _s: PhantomData,
//         }
//     }
// }

// /// Either a hash or a height to query for a [crate::infra::api::query::Block].
// #[derive(Debug, OneofObject)]
// enum BlockOffsetInput {
//     Hash(HexEncoded),
//     Height(u32),
// }

// impl BlockOffsetInput {
//     /// Resolves the block height from the given offset by querying storage
//     async fn resolve_height<S>(&self, storage: &S) -> async_graphql::Result<u32>
//     where
//         S: Storage,
//     {
//         match self {
//             BlockOffsetInput::Hash(ref hash) => {
//                 let hash = hash.hex_decode().context("decode hash")?;
//                 let block = storage
//                     .get_block_by_hash(&hash)
//                     .await
//                     .inspect_err(|error| {
//                         error!(error = error.as_chain(), "cannot get block by hash")
//                     })?
//                     .ok_or_else(|| {
//                         async_graphql::Error::new(format!("block with hash {hash:?} not found"))
//                     })?;
//                 Ok(block.height)
//             }

//             BlockOffsetInput::Height(height) => {
//                 storage
//                     .get_block_by_height(*height)
//                     .await
//                     .inspect_err(|error| {
//                         error!(error = error.as_chain(), "cannot get block by height")
//                     })?
//                     .ok_or_else(|| {
//                         async_graphql::Error::new(format!("block with height {} not found",
// height))                     })?;
//                 Ok(*height)
//             }
//         }
//     }
// }

// async fn into_from_height(
//     offset: Option<BlockOffsetInput>,
//     storage: &impl Storage,
// ) -> async_graphql::Result<u32> {
//     match offset {
//         Some(offset) => offset.resolve_height(storage).await,

//         None => {
//             let latest_block = storage
//                 .get_latest_block()
//                 .await
//                 .inspect_err(|error| error!(error = error.as_chain(), "cannot get latest
// block"))?;             let height = latest_block.map(|block| block.height).unwrap_or(1);
//             Ok(height)
//         }
//     }
// }

// /// A transaction with its relevant data.
// #[derive(Debug, SimpleObject)]
// #[graphql(complex)]
// struct Transaction<S>
// where
//     S: Storage,
// {
//     /// The transaction hash.
//     hash: HexEncoded,

//     /// The protocol version.
//     protocol_version: u32,

//     /// The transaction apply stage.
//     apply_stage: ApplyStage,

//     /// The transaction identifiers.
//     identifiers: Vec<HexEncoded>,

//     /// The raw transaction content.
//     raw: HexEncoded,

//     /// The contract actions.
//     contract_actions: Vec<ContractAction>,

//     #[graphql(skip)]
//     block_hash: HexEncoded,

//     #[graphql(skip)]
//     _s: PhantomData<S>,
// }

// #[ComplexObject]
// impl<S> Transaction<S>
// where
//     S: Storage,
// {
//     /// The block for this transaction.
//     async fn block(&self, cx: &Context<'_>) -> async_graphql::Result<Block<S>> {
//         Query::<S>::default()
//             .block(cx, Some(BlockOffsetInput::Hash(self.block_hash.to_owned())))
//             .await?
//             .ok_or_else(|| {
//                 async_graphql::Error::new(format!(
//                     "no block for tx {:?} with block hash {:?}",
//                     self.hash, self.block_hash
//                 ))
//             })
//     }
// }

// impl<S> From<domain::Transaction> for Transaction<S>
// where
//     S: Storage,
// {
//     fn from(value: domain::Transaction) -> Self {
//         let domain::Transaction {
//             hash,
//             block_hash,
//             protocol_version: ProtocolVersion(protocol_version),
//             apply_stage,
//             identifiers,
//             raw,
//             contract_actions,
//             ..
//         } = value;

//         Self {
//             hash: hash.hex_encode(),
//             block_hash: block_hash.hex_encode(),
//             protocol_version,
//             apply_stage: apply_stage.into(),
//             identifiers: identifiers
//                 .into_iter()
//                 .map(|identifier| identifier.hex_encode())
//                 .collect::<Vec<_>>(),
//             raw: raw.hex_encode(),
//             contract_actions: contract_actions
//                 .into_iter()
//                 .map(Into::into)
//                 .collect::<Vec<_>>(),
//             _s: PhantomData,
//         }
//     }
// }

// /// Either a hash or an identifier to query for a [Transaction].
// #[derive(Debug, OneofObject)]
// enum TransactionOffset {
//     Hash(HexEncoded),
//     Identifier(HexEncoded),
// }

// /// Wrapper around [indexer_common::domain::ApplyStage] for the purpose of turning it into a
// /// `Scalar`.
// #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, From)]
// struct ApplyStage(pub indexer_common::domain::ApplyStage);

// scalar!(ApplyStage);

// /// A contract action.
// #[derive(Debug, Interface)]
// #[allow(clippy::duplicated_attributes)]
// #[graphql(
//     field(name = "address", ty = "HexEncoded"),
//     field(name = "state", ty = "HexEncoded")
// )]
// enum ContractAction {
//     /// A contract call.
//     Call(ContractCall),

//     /// A contract deployment.
//     Deploy(ContractDeploy),

//     /// A contract update.
//     Update(ContractUpdate),
// }

// /// A contract call.
// #[derive(Debug, SimpleObject)]
// struct ContractCall {
//     address: HexEncoded,
//     state: HexEncoded,
//     entry_point: HexEncoded,
// }

// /// A contract deployment.
// #[derive(Debug, SimpleObject)]
// struct ContractDeploy {
//     address: HexEncoded,
//     state: HexEncoded,
// }

// /// A contract update.
// #[derive(Debug, SimpleObject)]
// struct ContractUpdate {
//     address: HexEncoded,
//     state: HexEncoded,
// }

// impl From<domain::ContractAction> for ContractAction {
//     fn from(action: domain::ContractAction) -> Self {
//         let domain::ContractAction {
//             address,
//             state,
//             attributes,
//             ..
//         } = action;

//         match attributes {
//             domain::ContractAttributes::Call { entry_point } => {
//                 ContractAction::Call(ContractCall {
//                     address: address.hex_encode(),
//                     state: state.hex_encode(),
//                     entry_point: entry_point.hex_encode(),
//                 })
//             }

//             domain::ContractAttributes::Deploy => ContractAction::Deploy(ContractDeploy {
//                 address: address.hex_encode(),
//                 state: state.hex_encode(),
//             }),

//             domain::ContractAttributes::Update => ContractAction::Update(ContractUpdate {
//                 address: address.hex_encode(),
//                 state: state.hex_encode(),
//             }),
//         }
//     }
// }

// /// Either a [BlockOffsetInput] or a [TransactionOffset] to query for a [ContractCallOrDeploy].
// #[derive(Debug, OneofObject)]
// enum ContractOffset {
//     BlockOffsetInput(BlockOffsetInput),
//     TransactionOffset(TransactionOffset),
// }

// #[derive(Debug, Union)]
// enum WalletSyncEvent<S: Storage> {
//     ProgressUpdate(ProgressUpdate),
//     ViewingUpdate(ViewingUpdate<S>),
// }

// #[derive(Debug, SimpleObject)]
// struct ProgressUpdate {
//     /// Last processed index of a relevant transaction for the wallet
//     synced: u64,

//     /// Last index of all processed transactions
//     total: u64,
// }

// #[derive(Debug, SimpleObject)]
// struct ViewingUpdate<S: Storage> {
//     /// Update end index
//     index: u64,

//     /// Relevant transaction for the wallet and (maybe) a collapsed Merkle-Tree update
//     update: Vec<ZswapChainStateUpdate<S>>,
// }

// #[derive(Debug, Union)]
// enum ZswapChainStateUpdate<S: Storage> {
//     MerkleTreeCollapsedUpdate(MerkleTreeCollapsedUpdate),
//     RelevantTransaction(RelevantTransaction<S>),
// }

// #[derive(Debug, SimpleObject)]
// struct MerkleTreeCollapsedUpdate {
//     /// The protocol version.
//     protocol_version: u32,

//     /// Hex-encoded Merkle-Tree update
//     update: HexEncoded,

//     /// Start index
//     start: u64,

//     /// End index
//     end: u64,
// }

// impl From<domain::MerkleTreeCollapsedUpdate> for MerkleTreeCollapsedUpdate {
//     fn from(value: domain::MerkleTreeCollapsedUpdate) -> Self {
//         let domain::MerkleTreeCollapsedUpdate {
//             protocol_version: ProtocolVersion(protocol_version),
//             update,
//             start_index,
//             end_index,
//         } = value;

//         Self {
//             protocol_version,
//             update,
//             start: start_index,
//             end: end_index,
//         }
//     }
// }

// #[derive(Debug, SimpleObject)]
// struct RelevantTransaction<S: Storage> {
//     /// Relevant transaction for the wallet
//     transaction: Transaction<S>,

//     /// Start index
//     start: u64,

//     /// End index
//     end: u64,
// }

// impl<S> From<domain::Transaction> for RelevantTransaction<S>
// where
//     S: Storage,
// {
//     fn from(value: domain::Transaction) -> Self {
//         Self {
//             start: value.start_index,
//             end: value.end_index,
//             transaction: value.into(),
//         }
//     }
// }

// /// Export the GraphQL schema in SDL format.
// pub fn export_schema() -> String {
//     //Once traits with async functions are object safe, `NoopStorage` can be replaced with
//     // `<Box<dyn Storage>`.
//     schema_builder::<NoopStorage, NoopSubscriber>()
//         .finish()
//         .sdl()
// }

// pub fn make_app<S, B>(
//     network_id: NetworkId,
//     zswap_state_cache: ZswapStateCache,
//     storage: S,
//     subscriber: B,
//     max_complexity: usize,
//     max_depth: usize,
// ) -> Router
// where
//     S: Storage,
//     B: Subscriber,
// {
//     let schema = schema_builder::<S, B>()
//         .data(network_id)
//         .data(zswap_state_cache)
//         .data(storage)
//         .data(subscriber)
//         .limit_complexity(max_complexity)
//         .limit_depth(max_depth)
//         .limit_recursive_depth(max_depth)
//         .finish();

//     Router::new()
//         .route("/graphql", post_service(GraphQL::new(schema.clone())))
//         .route_service("/graphql/ws", GraphQLSubscription::new(schema))
// }

// fn schema_builder<S, B>() -> SchemaBuilder<Query<S>, Mutation<S>, Subscription<S, B>>
// where
//     S: Storage,
//     B: Subscriber,
// {
//     Schema::build(
//         Query::<S>::default(),
//         Mutation::<S>::default(),
//         Subscription::<S, B>::default(),
//     )
// }
