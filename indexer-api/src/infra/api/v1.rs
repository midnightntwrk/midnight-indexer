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

mod mutation;
mod query;
mod subscription;

use crate::{
    domain::{self, AsBytesExt, BlockHash, HexEncoded, NoopStorage, Storage, ZswapStateCache},
    infra::api::{
        ContextExt, OptionExt, ResultExt,
        v1::{mutation::Mutation, query::Query, subscription::Subscription},
    },
};
use anyhow::Context as AnyhowContext;
use async_graphql::{
    ComplexObject, Context, Enum, Interface, OneofObject, Schema, SchemaBuilder, SimpleObject,
    Union, scalar,
};
use async_graphql_axum::{GraphQL, GraphQLSubscription};
use axum::{Router, routing::post_service};
use derive_more::Debug;
use indexer_common::domain::{
    ByteVec, LedgerStateStorage, NetworkId, NoopLedgerStateStorage, NoopSubscriber,
    ProtocolVersion, SessionId, Subscriber,
};
use serde::{Deserialize, Serialize};
use std::{
    marker::PhantomData,
    sync::{Arc, atomic::AtomicBool},
};

/// A block with its relevant data.
#[derive(Debug, SimpleObject)]
#[graphql(complex)]
struct Block<S: Storage>
where
    S: Storage,
{
    /// The block hash.
    hash: HexEncoded,

    /// The block height.
    height: u32,

    /// The protocol version.
    protocol_version: u32,

    /// The UNIX timestamp.
    timestamp: u64,

    /// The block author.
    author: Option<HexEncoded>,

    #[graphql(skip)]
    id: u64,

    #[graphql(skip)]
    parent_hash: BlockHash,

    #[graphql(skip)]
    _s: PhantomData<S>,
}

#[ComplexObject]
impl<S> Block<S>
where
    S: Storage,
{
    /// The parent of this block.
    async fn parent(&self, cx: &Context<'_>) -> async_graphql::Result<Option<Block<S>>> {
        let block = cx
            .get_storage::<S>()
            .get_block_by_hash(self.parent_hash)
            .await
            .internal("cannot get block by hash")?;

        Ok(block.map(Into::into))
    }

    /// The transactions within this block.
    async fn transactions(&self, cx: &Context<'_>) -> async_graphql::Result<Vec<Transaction<S>>> {
        let transactions = cx
            .get_storage::<S>()
            .get_transactions_by_block_id(self.id)
            .await
            .internal("cannot get transactions by block id")?;

        Ok(transactions.into_iter().map(Into::into).collect())
    }
}

impl<S> From<domain::Block> for Block<S>
where
    S: Storage,
{
    fn from(value: domain::Block) -> Self {
        let domain::Block {
            id,
            hash,
            height,
            protocol_version: ProtocolVersion(protocol_version),
            author,
            timestamp,
            parent_hash,
        } = value;

        Block {
            hash: hash.hex_encode(),
            height,
            protocol_version,
            author: author.map(|author| author.hex_encode()),
            timestamp,
            id,
            parent_hash,
            _s: PhantomData,
        }
    }
}

/// Either a hash or a height to query a block.
#[derive(Debug, OneofObject)]
enum BlockOffset {
    Hash(HexEncoded),
    Height(u32),
}

/// A transaction with its relevant data.
#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex)]
struct Transaction<S>
where
    S: Storage,
{
    /// The transaction hash.
    hash: HexEncoded,

    /// The protocol version.
    protocol_version: u32,

    /// The result of applying a transaction to the ledger state.
    transaction_result: TransactionResult,

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
    block_hash: BlockHash,

    #[graphql(skip)]
    #[debug(skip)]
    _s: PhantomData<S>,
}

#[ComplexObject]
impl<S> Transaction<S>
where
    S: Storage,
{
    /// The block for this transaction.
    async fn block(&self, cx: &Context<'_>) -> async_graphql::Result<Block<S>> {
        let block = cx
            .get_storage::<S>()
            .get_block_by_hash(self.block_hash)
            .await?
            .internal(format!(
                "no block for tx {:?} with block hash {:?}",
                self.hash, self.block_hash
            ))?;

        Ok(block.into())
    }

    /// The contract actions.
    async fn contract_actions(
        &self,
        cx: &Context<'_>,
    ) -> async_graphql::Result<Vec<ContractAction<S>>> {
        let contract_actions = cx
            .get_storage::<S>()
            .get_contract_actions_by_transaction_id(self.id)
            .await
            .internal("cannot get contract actions by transactions id")?;

        Ok(contract_actions.into_iter().map(Into::into).collect())
    }
}

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
            transaction_result,
            identifiers,
            raw,
            merkle_tree_root,
            ..
        } = value;

        Self {
            hash: hash.hex_encode(),
            protocol_version,
            transaction_result: transaction_result.into(),
            identifiers: identifiers
                .into_iter()
                .map(|identifier| identifier.hex_encode())
                .collect::<Vec<_>>(),
            raw: raw.hex_encode(),
            merkle_tree_root: merkle_tree_root.hex_encode(),
            id,
            block_hash,
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

/// Either a hash or an identifier to query transactions.
#[derive(Debug, OneofObject)]
enum TransactionOffset {
    Hash(HexEncoded),
    Identifier(HexEncoded),
}

/// The result of applying a transaction to the ledger state.
/// In case of a partial success (status), there will be segments.
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct TransactionResult {
    pub status: TransactionResultStatus,
    pub segments: Option<Vec<Segment>>,
}

/// The status of the transaction result: success, partial success or failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Enum)]
pub enum TransactionResultStatus {
    Success,
    PartialSuccess,
    Failure,
}

/// One of many segments for a partially successful transaction result showing success for some
/// segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, SimpleObject)]
pub struct Segment {
    /// Segment ID.
    id: u16,

    /// Successful or not.
    success: bool,
}

impl From<indexer_common::domain::TransactionResult> for TransactionResult {
    fn from(transaction_result: indexer_common::domain::TransactionResult) -> Self {
        match transaction_result {
            indexer_common::domain::TransactionResult::Success => Self {
                status: TransactionResultStatus::Success,
                segments: None,
            },

            indexer_common::domain::TransactionResult::PartialSuccess(segments) => {
                let segments = segments
                    .into_iter()
                    .map(|(id, success)| Segment { id, success })
                    .collect();

                Self {
                    status: TransactionResultStatus::PartialSuccess,
                    segments: Some(segments),
                }
            }

            indexer_common::domain::TransactionResult::Failure => Self {
                status: TransactionResultStatus::Failure,
                segments: None,
            },
        }
    }
}

/// A contract action.
#[derive(Debug, Clone, Interface)]
#[allow(clippy::duplicated_attributes)]
#[graphql(
    field(name = "address", ty = "HexEncoded"),
    field(name = "state", ty = "HexEncoded"),
    field(name = "chain_state", ty = "HexEncoded"),
    field(name = "transaction", ty = "Transaction<S>")
)]
enum ContractAction<S: Storage> {
    /// A contract deployment.
    Deploy(ContractDeploy<S>),

    /// A contract call.
    Call(ContractCall<S>),

    /// A contract update.
    Update(ContractUpdate<S>),
}

/// A contract deployment.
#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex)]
struct ContractDeploy<S: Storage> {
    address: HexEncoded,

    state: HexEncoded,

    chain_state: HexEncoded,

    #[graphql(skip)]
    transaction_id: u64,

    #[graphql(skip)]
    _s: PhantomData<S>,
}

#[ComplexObject]
impl<S: Storage> ContractDeploy<S> {
    async fn transaction(&self, cx: &Context<'_>) -> async_graphql::Result<Transaction<S>> {
        get_transaction_by_id(self.transaction_id, cx).await
    }
}

/// A contract call.
#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex)]
struct ContractCall<S: Storage> {
    address: HexEncoded,

    state: HexEncoded,

    chain_state: HexEncoded,

    entry_point: HexEncoded,

    #[graphql(skip)]
    transaction_id: u64,

    #[graphql(skip)]
    raw_address: ByteVec,

    #[graphql(skip)]
    _s: PhantomData<S>,
}

#[ComplexObject]
impl<S: Storage> ContractCall<S> {
    async fn transaction(&self, cx: &Context<'_>) -> async_graphql::Result<Transaction<S>> {
        get_transaction_by_id(self.transaction_id, cx).await
    }

    async fn deploy(&self, cx: &Context<'_>) -> async_graphql::Result<ContractDeploy<S>> {
        let action = cx
            .get_storage::<S>()
            .get_contract_deploy_by_address(&self.raw_address)
            .await
            .internal("cannot get contract deploy by address")?
            .expect("contract call has contract deploy");

        let deploy = match ContractAction::from(action) {
            ContractAction::Deploy(deploy) => deploy,
            _ => panic!("unexpected contract action"),
        };

        Ok(deploy)
    }
}

/// A contract update.
#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex)]
struct ContractUpdate<S: Storage> {
    address: HexEncoded,

    state: HexEncoded,

    chain_state: HexEncoded,

    #[graphql(skip)]
    transaction_id: u64,

    #[graphql(skip)]
    _s: PhantomData<S>,
}

#[ComplexObject]
impl<S: Storage> ContractUpdate<S> {
    async fn transaction(&self, cx: &Context<'_>) -> async_graphql::Result<Transaction<S>> {
        get_transaction_by_id(self.transaction_id, cx).await
    }
}

async fn get_transaction_by_id<S>(
    id: u64,
    cx: &Context<'_>,
) -> async_graphql::Result<Transaction<S>>
where
    S: Storage,
{
    let transaction = cx
        .get_storage::<S>()
        .get_transaction_by_id(id)
        .await
        .internal("cannot get transaction by ID")?;

    Ok(transaction.into())
}

impl<S> From<domain::ContractAction> for ContractAction<S>
where
    S: Storage,
{
    fn from(action: domain::ContractAction) -> Self {
        let domain::ContractAction {
            address,
            state,
            attributes,
            zswap_state,
            transaction_id,
            ..
        } = action;

        match attributes {
            domain::ContractAttributes::Deploy => ContractAction::Deploy(ContractDeploy {
                address: address.hex_encode(),
                state: state.hex_encode(),
                chain_state: zswap_state.hex_encode(),
                transaction_id,
                _s: PhantomData,
            }),

            domain::ContractAttributes::Call { entry_point } => {
                ContractAction::Call(ContractCall {
                    address: address.hex_encode(),
                    state: state.hex_encode(),
                    entry_point: entry_point.hex_encode(),
                    chain_state: zswap_state.hex_encode(),
                    transaction_id,
                    raw_address: address,
                    _s: PhantomData,
                })
            }

            domain::ContractAttributes::Update => ContractAction::Update(ContractUpdate {
                address: address.hex_encode(),
                state: state.hex_encode(),
                chain_state: zswap_state.hex_encode(),
                transaction_id,
                _s: PhantomData,
            }),
        }
    }
}

/// Either a block offset or a transaction offset to query a contract action.
#[derive(Debug, OneofObject)]
enum ContractActionOffset {
    BlockOffset(BlockOffset),
    TransactionOffset(TransactionOffset),
}

#[derive(Debug, Union)]
enum WalletSyncEvent<S: Storage> {
    ViewingUpdate(ViewingUpdate<S>),
    ProgressUpdate(ProgressUpdate),
}

/// Aggregates information about the wallet indexing progress.
#[derive(Debug, SimpleObject)]
struct ProgressUpdate {
    /// The highest end index into the zswap state of all currently known transactions.
    highest_index: u64,

    /// The highest end index into the zswap state of all currently known relevant transactions,
    /// i.e. such that belong to any wallet. Less or equal `highest_index`.
    highest_relevant_index: u64,

    /// The highest end index into the zswap state of all currently known relevant transactions for
    /// a particular wallet. Less or equal `highest_relevant_index`.
    highest_relevant_wallet_index: u64,
}

/// Aggregates a relevant transaction with the next start index and an optional collapsed
/// Merkle-Tree update.
#[derive(Debug, SimpleObject)]
struct ViewingUpdate<S: Storage> {
    /// Next start index into the zswap state to be queried. Usually the end index of the included
    /// relevant transaction plus one unless that is a failure in which case just its end
    /// index.
    index: u64,

    /// Relevant transaction for the wallet and maybe a collapsed Merkle-Tree update.
    update: Vec<ZswapChainStateUpdate<S>>,
}

#[derive(Debug, Union)]
enum ZswapChainStateUpdate<S: Storage> {
    MerkleTreeCollapsedUpdate(MerkleTreeCollapsedUpdate),
    RelevantTransaction(RelevantTransaction<S>),
}

#[derive(Debug, SimpleObject)]
struct MerkleTreeCollapsedUpdate {
    /// The protocol version.
    protocol_version: u32,

    /// The start index into the zswap state.
    start: u64,

    /// The end index into the zswap state.
    end: u64,

    /// The hex-encoded merkle-tree collapsed update.
    #[debug(skip)]
    update: HexEncoded,
}

impl From<domain::MerkleTreeCollapsedUpdate> for MerkleTreeCollapsedUpdate {
    fn from(value: domain::MerkleTreeCollapsedUpdate) -> Self {
        let domain::MerkleTreeCollapsedUpdate {
            protocol_version,
            start_index,
            end_index,
            update,
        } = value;

        Self {
            protocol_version: protocol_version.0,
            start: start_index,
            end: end_index,
            update: update.hex_encode(),
        }
    }
}

#[derive(Debug, SimpleObject)]
struct RelevantTransaction<S: Storage> {
    /// Relevant transaction for the wallet.
    transaction: Transaction<S>,

    /// The start index.
    start: u64,

    /// The end index.
    end: u64,
}

impl<S> From<domain::Transaction> for RelevantTransaction<S>
where
    S: Storage,
{
    fn from(value: domain::Transaction) -> Self {
        Self {
            start: value.start_index,
            end: value.end_index,
            transaction: value.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unit;

scalar!(Unit);

/// Export the GraphQL schema in SDL format.
pub fn export_schema() -> String {
    //Once traits with async functions are object safe, `NoopStorage` can be replaced with
    // `<Box<dyn Storage>`.
    schema_builder::<NoopStorage, NoopSubscriber, NoopLedgerStateStorage>()
        .finish()
        .sdl()
}

pub fn make_app<S, B, Z>(
    network_id: NetworkId,
    zswap_state_cache: ZswapStateCache,
    storage: S,
    ledger_state_storage: Z,
    subscriber: B,
    max_complexity: usize,
    max_depth: usize,
) -> Router<Arc<AtomicBool>>
where
    S: Storage,
    B: Subscriber,
    Z: LedgerStateStorage,
{
    let schema = schema_builder::<S, B, Z>()
        .data(network_id)
        .data(zswap_state_cache)
        .data(storage)
        .data(ledger_state_storage)
        .data(subscriber)
        .limit_complexity(max_complexity)
        .limit_depth(max_depth)
        .limit_recursive_depth(max_depth)
        .finish();

    Router::new()
        .route("/graphql", post_service(GraphQL::new(schema.clone())))
        .route_service("/graphql/ws", GraphQLSubscription::new(schema))
}

fn schema_builder<S, B, Z>() -> SchemaBuilder<Query<S>, Mutation<S>, Subscription<S, B, Z>>
where
    S: Storage,
    B: Subscriber,
    Z: LedgerStateStorage,
{
    Schema::build(
        Query::<S>::default(),
        Mutation::<S>::default(),
        Subscription::<S, B, Z>::default(),
    )
}

async fn resolve_height(
    offset: Option<BlockOffset>,
    storage: &impl Storage,
) -> async_graphql::Result<u32> {
    match offset {
        Some(offset) => match offset {
            BlockOffset::Hash(hash) => {
                let hash = hash.hex_decode().context("hex-decode hash")?;

                let block = storage
                    .get_block_by_hash(hash)
                    .await
                    .internal("get block by hash")?
                    .with_context(|| format!("block with hash {hash:?} not found"))?;

                Ok(block.height)
            }

            BlockOffset::Height(height) => {
                storage
                    .get_block_by_height(height)
                    .await
                    .internal("get block by height")?
                    .with_context(|| format!("block with height {} not found", height))?;

                Ok(height)
            }
        },

        None => {
            let latest_block = storage
                .get_latest_block()
                .await
                .internal("get latest block")?;
            let height = latest_block.map(|block| block.height).unwrap_or_default();

            Ok(height)
        }
    }
}

fn hex_decode_session_id(session_id: HexEncoded) -> async_graphql::Result<SessionId> {
    let session_id = session_id
        .hex_decode::<Vec<u8>>()
        .context("hex-decode session ID")?;
    let session_id = SessionId::try_from(session_id.as_slice()).context("invalid session ID")?;

    Ok(session_id)
}
