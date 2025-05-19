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
        self, AsBytesExt, HexEncoded, NoopStorage, Storage, UnshieldedAddressFormatError,
        ZswapStateCache,
    },
    infra::api::{
        v1::{mutation::Mutation, query::Query, subscription::Subscription},
        ContextExt,
    },
};
use anyhow::Context as AnyhowContext;
use async_graphql::{
    scalar, ComplexObject, Context, Enum, Interface, OneofObject, Schema, SchemaBuilder,
    SimpleObject, Union,
};
use async_graphql_axum::{GraphQL, GraphQLSubscription};
use axum::{routing::post_service, Router};
use indexer_common::{
    domain::{
        NetworkId, NoopSubscriber, NoopZswapStateStorage, ProtocolVersion, Subscriber,
        UnshieldedAddress as CommonUnshieldedAddress, ZswapStateStorage,
    },
    error::StdErrorExt,
};
use log::error;
use serde::{Deserialize, Serialize};
use std::{
    marker::PhantomData,
    sync::{atomic::AtomicBool, Arc},
};

/// A block with its relevant data.
#[derive(Debug, SimpleObject)]
#[graphql(complex)]
struct Block<S>
where
    S: Storage,
{
    /// The block hash.
    hash: HexEncoded,

    /// The block height (number).
    height: u32,

    /// The protocol version.
    protocol_version: u32,

    /// The UNIX timestamp.
    timestamp: u64,

    /// The block author.
    author: Option<HexEncoded>,

    /// The transactions.
    transactions: Vec<Transaction<S>>,

    #[graphql(skip)]
    parent_hash: HexEncoded,

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
        let storage = cx.get_storage::<S>()?;

        let parent_hash = self.parent_hash.hex_decode().inspect_err(|error| {
            error!(
                error = error.as_chain(),
                parent_hash:? = self.parent_hash;
                "cannot hex-decode parent hash"
            )
        })?;
        let block = storage
            .get_block_by_hash(&parent_hash)
            .await
            .inspect_err(|error| {
                error!(
                    error = error.as_chain(),
                    parent_hash:%;
                    "cannot get block by hash"
                )
            })?;

        Ok(block.map(Into::into))
    }
}

impl<S> From<domain::Block> for Block<S>
where
    S: Storage,
{
    fn from(value: domain::Block) -> Self {
        let domain::Block {
            hash,
            height,
            protocol_version: ProtocolVersion(protocol_version),
            author,
            timestamp,
            transactions,
            parent_hash,
            ..
        } = value;

        Block {
            hash: hash.hex_encode(),
            height,
            protocol_version,
            author: author.map(|author| author.hex_encode()),
            timestamp,
            transactions: transactions.into_iter().map(Into::into).collect::<Vec<_>>(),
            parent_hash: parent_hash.hex_encode(),
            _s: PhantomData,
        }
    }
}

/// Either a hash or a height to query for a [crate::infra::api::query::Block].
#[derive(Debug, OneofObject)]
enum BlockOffsetInput {
    Hash(HexEncoded),
    Height(u32),
}

impl BlockOffsetInput {
    /// Resolves the block height from the given offset by querying storage
    async fn resolve_height<S>(&self, storage: &S) -> async_graphql::Result<u32>
    where
        S: Storage,
    {
        match self {
            BlockOffsetInput::Hash(hash) => {
                let hash = hash.hex_decode().context("decode hash")?;
                let block = storage
                    .get_block_by_hash(&hash)
                    .await
                    .inspect_err(
                        |error| error!(error:? = error.as_chain(); "cannot get block by hash"),
                    )?
                    .ok_or_else(|| {
                        async_graphql::Error::new(format!("block with hash {hash:?} not found"))
                    })?;
                Ok(block.height)
            }

            BlockOffsetInput::Height(height) => {
                storage
                    .get_block_by_height(*height)
                    .await
                    .inspect_err(
                        |error| error!(error:? = error.as_chain(); "cannot get block by height"),
                    )?
                    .ok_or_else(|| {
                        async_graphql::Error::new(format!("block with height {} not found", height))
                    })?;
                Ok(*height)
            }
        }
    }
}

async fn into_from_height(
    offset: Option<BlockOffsetInput>,
    storage: &impl Storage,
) -> async_graphql::Result<u32> {
    match offset {
        Some(offset) => offset.resolve_height(storage).await,

        None => {
            let latest_block = storage.get_latest_block().await.inspect_err(
                |error| error!(error:% = error.as_chain(); "cannot get latest block"),
            )?;
            let height = latest_block.map(|block| block.height).unwrap_or(1);
            Ok(height)
        }
    }
}

/// A transaction with its relevant data.
#[derive(Debug, SimpleObject)]
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
    identifiers: Vec<HexEncoded>,

    /// The raw transaction content.
    raw: HexEncoded,

    /// The contract actions.
    #[graphql(deprecation = "use v2/contract_actions")]
    contract_calls: Vec<ContractCallOrDeploy>,

    /// The merkle-tree root.
    merkle_tree_root: HexEncoded,

    /// Unshielded UTXOs created by this transaction.
    unshielded_created_outputs: Vec<UnshieldedUtxo<S>>,

    /// Unshielded UTXOs spent (consumed) by this transaction.
    unshielded_spent_outputs: Vec<UnshieldedUtxo<S>>,

    #[graphql(skip)]
    block_hash: HexEncoded,

    #[graphql(skip)]
    _s: PhantomData<S>,
}

#[ComplexObject]
impl<S> Transaction<S>
where
    S: Storage,
{
    /// The block for this transaction.
    async fn block(&self, cx: &Context<'_>) -> async_graphql::Result<Block<S>> {
        Query::<S>::default()
            .block(cx, Some(BlockOffsetInput::Hash(self.block_hash.to_owned())))
            .await?
            .ok_or_else(|| {
                async_graphql::Error::new(format!(
                    "no block for tx {:?} with block hash {:?}",
                    self.hash, self.block_hash
                ))
            })
    }
}

impl<S> From<domain::Transaction> for Transaction<S>
where
    S: Storage,
{
    fn from(value: domain::Transaction) -> Self {
        let domain::Transaction {
            hash,
            block_hash,
            protocol_version: ProtocolVersion(protocol_version),
            apply_stage,
            identifiers,
            raw,
            merkle_tree_root,
            contract_actions,
            unshielded_created_outputs,
            unshielded_spent_outputs,
            ..
        } = value;

        Self {
            hash: hash.hex_encode(),
            block_hash: block_hash.hex_encode(),
            protocol_version,
            apply_stage: apply_stage.into(),
            identifiers: identifiers
                .into_iter()
                .map(|identifier| identifier.hex_encode())
                .collect::<Vec<_>>(),
            raw: raw.hex_encode(),
            merkle_tree_root: merkle_tree_root.hex_encode(),
            contract_calls: contract_actions
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>(),
            unshielded_created_outputs: unshielded_created_outputs
                .into_iter()
                .map(UnshieldedUtxo::<S>::from)
                .collect(),
            unshielded_spent_outputs: unshielded_spent_outputs
                .into_iter()
                .map(UnshieldedUtxo::<S>::from)
                .collect(),
            _s: PhantomData,
        }
    }
}

/// Either a hash or an identifier to query for a [Transaction].
#[derive(Debug, OneofObject)]
enum TransactionOffset {
    Hash(HexEncoded),
    Identifier(HexEncoded),
}

/// Wrapper around [indexer_common::domain::ApplyStage] for the purpose of turning it into a
/// `Scalar`.
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
#[derive(Debug, Interface)]
#[allow(clippy::duplicated_attributes)]
#[graphql(
    field(name = "address", ty = "HexEncoded"),
    field(name = "state", ty = "HexEncoded"),
    field(name = "zswap_chain_state", ty = "HexEncoded")
)]
enum ContractCallOrDeploy {
    /// A contract deployment.
    Deploy(ContractDeploy),

    /// A contract call.
    Call(ContractCall),

    /// A contract update.
    Update(ContractUpdate),
}

/// A contract call.
#[derive(Debug, SimpleObject)]
struct ContractCall {
    address: HexEncoded,
    state: HexEncoded,
    entry_point: HexEncoded,
    zswap_chain_state: HexEncoded,
}

/// A contract deployment.
#[derive(Debug, SimpleObject)]
struct ContractDeploy {
    address: HexEncoded,
    state: HexEncoded,
    zswap_chain_state: HexEncoded,
}

/// A contract update.
#[derive(Debug, SimpleObject)]
struct ContractUpdate {
    address: HexEncoded,
    state: HexEncoded,
    zswap_chain_state: HexEncoded,
}

impl From<domain::ContractAction> for ContractCallOrDeploy {
    fn from(action: domain::ContractAction) -> Self {
        let domain::ContractAction {
            address,
            state,
            attributes,
            zswap_state,
            ..
        } = action;

        match attributes {
            domain::ContractAttributes::Deploy => ContractCallOrDeploy::Deploy(ContractDeploy {
                address: address.hex_encode(),
                state: state.hex_encode(),
                zswap_chain_state: zswap_state.hex_encode(),
            }),

            domain::ContractAttributes::Call { entry_point } => {
                ContractCallOrDeploy::Call(ContractCall {
                    address: address.hex_encode(),
                    state: state.hex_encode(),
                    entry_point: entry_point.hex_encode(),
                    zswap_chain_state: zswap_state.hex_encode(),
                })
            }

            domain::ContractAttributes::Update => ContractCallOrDeploy::Update(ContractUpdate {
                address: address.hex_encode(),
                state: state.hex_encode(),
                zswap_chain_state: zswap_state.hex_encode(),
            }),
        }
    }
}

/// Either a [BlockOffsetInput] or a [TransactionOffset] to query for a [ContractCallOrDeploy].
#[derive(Debug, OneofObject)]
enum ContractOffset {
    BlockOffsetInput(BlockOffsetInput),
    TransactionOffset(TransactionOffset),
}

/// Represents an unshielded UTXO.
#[derive(Debug, SimpleObject)]
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
    async fn created_at_transaction(&self) -> async_graphql::Result<Transaction<S>> {
        self.created_at_transaction_data
            .clone()
            .map(Transaction::<S>::from)
            .ok_or_else(|| async_graphql::Error::new("Missing creating transaction data"))
    }

    /// Transaction that spent this UTXO, if spent
    async fn spent_at_transaction(&self) -> async_graphql::Result<Option<Transaction<S>>> {
        Ok(self
            .spent_at_transaction_data
            .clone()
            .map(Transaction::<S>::from))
    }
}

impl<S: Storage> From<domain::UnshieldedUtxo> for UnshieldedUtxo<S> {
    fn from(domain_utxo: domain::UnshieldedUtxo) -> Self {
        let owner_bech32m = indexer_common::domain::unshielded::to_bech32m(
            domain_utxo.owner_address.as_ref(),
            domain_utxo.network_id.unwrap(),
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// Either a [BlockOffsetInput] or a [TransactionOffset] to query for a [UnshieldedUtxo].
#[derive(Debug, OneofObject)]
enum UnshieldedOffset {
    BlockOffsetInput(BlockOffsetInput),
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

#[derive(Debug, SimpleObject)]
struct ProgressUpdate {
    /// Last synced end index for the wallet.
    synced: u64,

    /// Last processed transaction end index for the wallet.
    total: u64,
}

#[derive(Debug, SimpleObject)]
struct ViewingUpdate<S: Storage> {
    /// Update end index
    index: u64,

    /// Relevant transaction for the wallet and (maybe) a collapsed Merkle-Tree update
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

    /// The start index.
    start: u64,

    /// The end index.
    end: u64,

    /// The hex-encoded merkle-tree collapsed update.
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
    /// Relevant transaction for the wallet
    transaction: Transaction<S>,

    /// Start index
    start: u64,

    /// End index
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
