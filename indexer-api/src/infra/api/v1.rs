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
    domain::{
        self, AsBytesExt, BlockHash, HexEncoded, NoopStorage, Storage,
        UnshieldedAddressFormatError, UnshieldedUtxoExt, ZswapStateCache,
    },
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
    ByteVec, NetworkId, NoopSubscriber, NoopZswapStateStorage, ProtocolVersion, SessionId,
    Subscriber, UnshieldedAddress as CommonUnshieldedAddress, ZswapStateStorage,
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

    /// Unshielded UTXOs created by this transaction.
    async fn unshielded_created_outputs(
        &self,
        cx: &Context<'_>,
    ) -> async_graphql::Result<Vec<UnshieldedUtxo<S>>> {
        let storage = cx.get_storage::<S>();
        let network_id = cx.get_network_id();

        let utxos = storage
            .get_unshielded_utxos_by_creating_tx_id(self.id)
            .await
            .internal("cannot get unshielded UTXOs created by transaction")?;

        Ok(utxos
            .into_iter()
            .map(|utxo| UnshieldedUtxo::<S>::from((utxo, network_id)))
            .collect())
    }

    /// Unshielded UTXOs spent (consumed) by this transaction.
    async fn unshielded_spent_outputs(
        &self,
        cx: &Context<'_>,
    ) -> async_graphql::Result<Vec<UnshieldedUtxo<S>>> {
        let storage = cx.get_storage::<S>();
        let network_id = cx.get_network_id();

        let utxos = storage
            .get_unshielded_utxos_by_spending_tx_id(self.id)
            .await
            .internal("cannot get unshielded UTXOs spent by transaction")?;

        Ok(utxos
            .into_iter()
            .map(|utxo| UnshieldedUtxo::<S>::from((utxo, network_id)))
            .collect())
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

/// The apply stage of a transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApplyStage {
    SucceedEntirely,
    FailFallible,
    FailEntirely,
}

scalar!(ApplyStage);

impl From<indexer_common::domain::ApplyStage> for ApplyStage {
    fn from(apply_stage: indexer_common::domain::ApplyStage) -> Self {
        match apply_stage {
            indexer_common::domain::ApplyStage::Success => Self::SucceedEntirely,
            indexer_common::domain::ApplyStage::PartialSuccess => Self::FailFallible,
            indexer_common::domain::ApplyStage::Failure => Self::FailEntirely,
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

/// Represents an unshielded UTXO.
#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex)]
struct UnshieldedUtxo<S: Storage> {
    /// Owner address (Bech32m, `mn_addr…`)
    owner: UnshieldedAddress,
    /// The hash of the intent that created this output (hex-encoded)
    intent_hash: HexEncoded,
    /// UTXO value (quantity) as a string to support u128
    value: String,
    /// Token type (hex-encoded)
    token_type: HexEncoded,
    /// Index of this output within its creating transaction
    output_index: u32,

    #[graphql(skip)]
    created_at_transaction_data: Option<domain::Transaction>,
    #[graphql(skip)]
    spent_at_transaction_data: Option<domain::Transaction>,
    #[graphql(skip)]
    _s: PhantomData<S>,
}

#[ComplexObject]
impl<S: Storage> UnshieldedUtxo<S> {
    /// Transaction that created this UTXO
    async fn created_at_transaction(&self) -> async_graphql::Result<Option<Transaction<S>>> {
        //can't change the return type to be non-optional because the node ut data is mocked and
        // the test fails
        Ok(self
            .created_at_transaction_data
            .clone()
            .map(Transaction::<S>::from))
    }

    /// Transaction that spent this UTXO, if spent
    async fn spent_at_transaction(&self) -> async_graphql::Result<Option<Transaction<S>>> {
        Ok(self
            .spent_at_transaction_data
            .clone()
            .map(Transaction::<S>::from))
    }
}

impl<S: Storage> From<(domain::UnshieldedUtxo, NetworkId)> for UnshieldedUtxo<S> {
    fn from((domain_utxo, network_id): (domain::UnshieldedUtxo, NetworkId)) -> Self {
        let owner_bech32m = indexer_common::domain::unshielded::to_bech32m(
            domain_utxo.owner_address.as_ref(),
            network_id,
        )
        .expect("owner address can convert to Bech32m");

        Self {
            owner: UnshieldedAddress(owner_bech32m),
            value: domain_utxo.value.to_string(),
            intent_hash: domain_utxo.intent_hash.hex_encode(),
            token_type: domain_utxo.token_type.hex_encode(),
            output_index: domain_utxo.output_index,
            created_at_transaction_data: domain_utxo.created_at_transaction,
            spent_at_transaction_data: domain_utxo.spent_at_transaction,
            _s: PhantomData,
        }
    }
}

/// Bech32m-encoded address, e.g. `mn_addr_test1…`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct UnshieldedAddress(pub String);
scalar!(UnshieldedAddress);

/// Types of events emitted by the unshielded UTXO subscription
#[derive(Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnshieldedUtxoEventType {
    /// Indicates a transaction that created or spent UTXOs for the address
    UPDATE,
    /// Status message for synchronization progress or keep-alive
    PROGRESS,
}

/// Payload emitted by `subscription { unshieldedUtxos … }`
#[derive(SimpleObject)]
struct UnshieldedUtxoEvent<S>
where
    S: Storage,
{
    /// The type of event - UPDATE for changes, PROGRESS for status messages
    event_type: UnshieldedUtxoEventType,
    /// The transaction associated with this event
    transaction: Transaction<S>,
    /// UTXOs created in this transaction for the subscribed address
    created_utxos: Vec<UnshieldedUtxo<S>>,
    /// UTXOs spent in this transaction for the subscribed address
    spent_utxos: Vec<UnshieldedUtxo<S>>,
}

/// Either a [BlockOffset] or a [TransactionOffset] to query for a [UnshieldedUtxo].
#[derive(Debug, OneofObject)]
enum UnshieldedOffset {
    BlockOffset(BlockOffset),
    TransactionOffset(TransactionOffset),
}

impl UnshieldedAddress {
    pub fn try_into_domain(
        self,
        network_id: NetworkId,
    ) -> Result<CommonUnshieldedAddress, UnshieldedAddressFormatError> {
        domain::UnshieldedAddress(self.0).try_into_domain(network_id)
    }
}

/// Convert GraphQL wrapper into the raw-bytes domain type.
fn addr_to_common(
    addr: &UnshieldedAddress,
    network_id: NetworkId,
) -> async_graphql::Result<CommonUnshieldedAddress> {
    addr.clone()
        .try_into_domain(network_id)
        .map_err(|error| async_graphql::Error::new(error.to_string()))
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
    schema_builder::<NoopStorage, NoopSubscriber, NoopZswapStateStorage>()
        .finish()
        .sdl()
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
    let schema = schema_builder::<S, B, Z>()
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

fn schema_builder<S, B, Z>() -> SchemaBuilder<Query<S>, Mutation<S>, Subscription<S, B, Z>>
where
    S: Storage,
    B: Subscriber,
    Z: ZswapStateStorage,
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
