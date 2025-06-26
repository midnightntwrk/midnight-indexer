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

pub mod contract_balance;
mod mutation;
mod query;
mod subscription;

use self::contract_balance::ContractBalance;
use crate::{
    domain::{
        self, AsBytesExt, HexEncoded, ZswapStateCache,
        storage::{NoopStorage, Storage},
    },
    infra::api::{
        ContextExt, OptionExt, ResultExt,
        v1::{mutation::Mutation, query::Query, subscription::Subscription},
    },
};
use anyhow::Context as AnyhowContext;
use async_graphql::{
    ComplexObject, Context, Enum, ErrorExtensions, Interface, OneofObject, Schema, SchemaBuilder,
    SimpleObject, Union, scalar,
};
use async_graphql_axum::{GraphQL, GraphQLSubscription};
use axum::{Router, routing::post_service};
use bech32::{Bech32m, Hrp};
use derive_more::Debug;
use indexer_common::{
    domain::{
        BlockHash, ByteVec, LedgerStateStorage, NetworkId, NoopLedgerStateStorage, NoopSubscriber,
        ProtocolVersion, SessionId, Subscriber, UnknownNetworkIdError,
        UnshieldedAddress as CommonUnshieldedAddress,
    },
    error::NotFoundError,
};
use log::error;
use serde::{Deserialize, Serialize};
use std::{
    fmt::Display,
    marker::PhantomData,
    sync::{Arc, atomic::AtomicBool},
};
use thiserror::Error;

const HRP_UNSHIELDED_BASE: &str = "mn_addr";

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

    /// Fee information for this transaction.
    fees: TransactionFees,

    /// Segment results for client consumption.
    segment_results: Vec<SegmentResult>,

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
            .get_unshielded_utxos_created_by_transaction(self.id)
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
            .get_unshielded_utxos_spent_by_transaction(self.id)
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
            transaction_result,
            identifiers,
            raw,
            merkle_tree_root,
            ..
        } = value;

        // Use fees information from database (calculated by chain-indexer)
        let fees = TransactionFees {
            paid_fees: value
                .paid_fees
                .map(|f| f.to_string())
                .unwrap_or_else(|| "0".to_owned()),
            estimated_fees: value
                .estimated_fees
                .map(|f| f.to_string())
                .unwrap_or_else(|| "0".to_owned()),
        };

        // Convert segment results for client consumption
        let segment_results = match &transaction_result {
            indexer_common::domain::TransactionResult::PartialSuccess(segments) => segments
                .iter()
                .map(|(segment_id, success)| SegmentResult {
                    segment_id: *segment_id,
                    success: *success,
                })
                .collect(),
            indexer_common::domain::TransactionResult::Success => vec![],
            indexer_common::domain::TransactionResult::Failure => vec![],
        };

        Self {
            hash: hash.hex_encode(),
            protocol_version,
            transaction_result: transaction_result.into(),
            fees,
            segment_results,
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

/// Fees information for a transaction, including both paid and estimated fees.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, SimpleObject)]
pub struct TransactionFees {
    /// The actual fees paid for this transaction in DUST.
    paid_fees: String,
    /// The estimated fees that was calculated for this transaction in DUST.
    estimated_fees: String,
}

/// Result for a specific segment within a transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, SimpleObject)]
pub struct SegmentResult {
    /// The segment identifier.
    segment_id: u16,
    /// Whether this segment was successfully executed.
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
    contract_action_id: u64,

    #[graphql(skip)]
    _s: PhantomData<S>,
}

#[ComplexObject]
impl<S: Storage> ContractDeploy<S> {
    async fn transaction(&self, cx: &Context<'_>) -> async_graphql::Result<Transaction<S>> {
        get_transaction_by_id(self.transaction_id, cx).await
    }

    /// Unshielded token balances held by this contract.
    /// According to the architecture, deployed contracts must have zero balance.
    async fn unshielded_balances(
        &self,
        cx: &Context<'_>,
    ) -> async_graphql::Result<Vec<ContractBalance>> {
        let storage = cx.get_storage::<S>();
        let balances = storage
            .get_unshielded_balances_by_action_id(self.contract_action_id)
            .await
            .internal("get contract balances by action id")?;

        Ok(balances
            .into_iter()
            .map(|balance| balance.to_api())
            .collect())
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
    contract_action_id: u64,

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

    /// Unshielded token balances held by this contract.
    async fn unshielded_balances(
        &self,
        cx: &Context<'_>,
    ) -> async_graphql::Result<Vec<ContractBalance>> {
        let storage = cx.get_storage::<S>();
        let balances = storage
            .get_unshielded_balances_by_action_id(self.contract_action_id)
            .await
            .internal("get contract balances by action id")?;

        Ok(balances
            .into_iter()
            .map(|balance| balance.to_api())
            .collect())
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
    contract_action_id: u64,

    #[graphql(skip)]
    _s: PhantomData<S>,
}

#[ComplexObject]
impl<S: Storage> ContractUpdate<S> {
    async fn transaction(&self, cx: &Context<'_>) -> async_graphql::Result<Transaction<S>> {
        get_transaction_by_id(self.transaction_id, cx).await
    }

    /// Unshielded token balances held by this contract after the update.
    async fn unshielded_balances(
        &self,
        cx: &Context<'_>,
    ) -> async_graphql::Result<Vec<ContractBalance>> {
        let storage = cx.get_storage::<S>();
        let balances = storage
            .get_unshielded_balances_by_action_id(self.contract_action_id)
            .await
            .internal("get contract balances by action id")?;

        Ok(balances
            .into_iter()
            .map(|balance| balance.to_api())
            .collect())
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
        .internal("cannot get transaction by ID")?
        .ok_or_else(|| NotFoundError(format!("transaction with ID {id}")))
        .internal("cannot get transaction by ID")?;

    Ok(transaction.into())
}

impl<S> From<domain::ContractAction> for ContractAction<S>
where
    S: Storage,
{
    fn from(action: domain::ContractAction) -> Self {
        let domain::ContractAction {
            id,
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
                contract_action_id: id,
                _s: PhantomData,
            }),

            domain::ContractAttributes::Call { entry_point } => {
                ContractAction::Call(ContractCall {
                    address: address.hex_encode(),
                    state: state.hex_encode(),
                    entry_point: entry_point.hex_encode(),
                    chain_state: zswap_state.hex_encode(),
                    transaction_id,
                    contract_action_id: id,
                    raw_address: address,
                    _s: PhantomData,
                })
            }

            domain::ContractAttributes::Update => ContractAction::Update(ContractUpdate {
                address: address.hex_encode(),
                state: state.hex_encode(),
                chain_state: zswap_state.hex_encode(),
                transaction_id,
                contract_action_id: id,
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
    fn from((utxo, network_id): (domain::UnshieldedUtxo, NetworkId)) -> Self {
        Self {
            owner: UnshieldedAddress::bech32m_encode(utxo.owner_address, network_id),
            value: utxo.value.to_string(),
            intent_hash: utxo.intent_hash.hex_encode(),
            token_type: utxo.token_type.hex_encode(),
            output_index: utxo.output_index,
            created_at_transaction_data: utxo.created_at_transaction,
            spent_at_transaction_data: utxo.spent_at_transaction,
            _s: PhantomData,
        }
    }
}

/// Bech32m-encoded unshielded address.
///
/// Format:
/// - MainNet: `mn_addr` + bech32m data
/// - DevNet: `mn_addr_dev` + bech32m data
/// - TestNet: `mn_addr_test` + bech32m data
/// - Undeployed: `mn_addr_undeployed` + bech32m data
///
/// The inner string is validated to ensure proper bech32m-encoding and correct HRP prefix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct UnshieldedAddress(pub String);

scalar!(UnshieldedAddress);

impl UnshieldedAddress {
    /// Converts this API address into a domain address, validating the bech32m format and
    /// network ID.
    ///
    /// Format expectations:
    /// - For mainnet: "mn_addr" + bech32m data
    /// - For other networks: "mn_addr_" + network-id + bech32m data where network-id is one of:
    ///   "dev", "test", "undeployed"
    pub fn try_into_domain(
        &self,
        network_id: NetworkId,
    ) -> Result<CommonUnshieldedAddress, UnshieldedAddressFormatError> {
        let (hrp, bytes) = bech32::decode(&self.0).map_err(UnshieldedAddressFormatError::Decode)?;
        let hrp = hrp.to_lowercase();

        let Some(n) = hrp.strip_prefix(HRP_UNSHIELDED_BASE) else {
            return Err(UnshieldedAddressFormatError::InvalidHrp(hrp));
        };
        let n = n.strip_prefix("_").unwrap_or(n).try_into()?;
        if n != network_id {
            return Err(UnshieldedAddressFormatError::UnexpectedNetworkId(
                n, network_id,
            ));
        }

        Ok(CommonUnshieldedAddress::from(bytes))
    }

    /// Encode raw bytes into a Bech32m-encoded address.
    pub fn bech32m_encode(bytes: impl AsRef<[u8]>, network_id: NetworkId) -> Self {
        let hrp = match network_id {
            NetworkId::MainNet => HRP_UNSHIELDED_BASE.to_string(),
            NetworkId::DevNet => format!("{}_dev", HRP_UNSHIELDED_BASE),
            NetworkId::TestNet => format!("{}_test", HRP_UNSHIELDED_BASE),
            NetworkId::Undeployed => format!("{}_undeployed", HRP_UNSHIELDED_BASE),
        };
        let hrp = Hrp::parse(&hrp).expect("unshielded address HRP can be parsed");

        let encoded = bech32::encode::<Bech32m>(hrp, bytes.as_ref())
            .expect("bytes for unshielded address can be Bech32m-encoded");
        Self(encoded)
    }
}

#[derive(Debug, Error)]
pub enum UnshieldedAddressFormatError {
    #[error("cannot bech32m-decode unshielded address")]
    Decode(#[from] bech32::DecodeError),

    #[error("invalid bech32m HRP {0}, expected 'mn_addr' prefix")]
    InvalidHrp(String),

    #[error(transparent)]
    TryFromStrForNetworkIdError(#[from] UnknownNetworkIdError),

    #[error("network ID mismatch: got {0}, expected {1}")]
    UnexpectedNetworkId(NetworkId, NetworkId),
}

/// Progress tracking information for unshielded token synchronization.
#[derive(SimpleObject, Clone, Debug)]
pub struct UnshieldedProgress {
    /// The highest transaction ID of all currently known transactions for a particular address.
    highest_transaction_id: u64,
    /// The current transaction ID for a particular address.
    current_transaction_id: u64,
}

/// Payload emitted by `subscription { unshieldedUtxos … }`
#[derive(SimpleObject)]
struct UnshieldedUtxoEvent<S>
where
    S: Storage,
{
    /// Progress information for wallet synchronization (always present)
    progress: UnshieldedProgress,
    /// The transaction associated with this event (present for actual updates)
    transaction: Option<Transaction<S>>,
    /// UTXOs created in this transaction for the subscribed address (None for progress-only
    /// events)
    created_utxos: Option<Vec<UnshieldedUtxo<S>>>,
    /// UTXOs spent in this transaction for the subscribed address (None for progress-only events)
    spent_utxos: Option<Vec<UnshieldedUtxo<S>>>,
}

/// Either a [BlockOffset] or a [TransactionOffset] to query for a [UnshieldedUtxo].
#[derive(Debug, OneofObject)]
enum UnshieldedOffset {
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
    /// i.e. those that belong to any known wallet. Less or equal `highest_index`.
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
#[allow(clippy::large_enum_variant)]
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

pub(crate) trait UnshieldedAddressResultExt<T> {
    /// Handle UnshieldedAddressFormatError by converting all variants to user-friendly GraphQL
    /// errors. All address format errors are client errors and should be exposed to users.
    fn address_validation<C>(self, context: C) -> async_graphql::Result<T>
    where
        C: Display + Send + Sync + 'static;
}

impl<T> UnshieldedAddressResultExt<T> for Result<T, UnshieldedAddressFormatError> {
    fn address_validation<C>(self, context: C) -> async_graphql::Result<T>
    where
        C: Display + Send + Sync + 'static,
    {
        self.map_err(|e| {
            let graphql_error = match &e {
                UnshieldedAddressFormatError::UnexpectedNetworkId(got, expected) => {
                    async_graphql::Error::new(format!(
                        "Invalid address: address is for {} network but indexer is configured for {}",
                        got, expected
                    ))
                    .extend_with(|_, e| e.set("code", "NETWORK_MISMATCH"))
                }

                UnshieldedAddressFormatError::Decode(_) => {
                    async_graphql::Error::new("Invalid address: cannot decode bech32m-encoded address")
                        .extend_with(|_, e| e.set("code", "INVALID_ADDRESS_FORMAT"))
                }

                UnshieldedAddressFormatError::InvalidHrp(hrp) => async_graphql::Error::new(format!(
                    "Invalid address: invalid prefix '{}', expected 'mn_addr' prefix",
                    hrp
                ))
                .extend_with(|_, e| e.set("code", "INVALID_ADDRESS_PREFIX")),

                UnshieldedAddressFormatError::TryFromStrForNetworkIdError(_) => {
                    async_graphql::Error::new("Invalid address: unknown network ID in address")
                        .extend_with(|_, e| e.set("code", "UNKNOWN_NETWORK_ID"))
                }
            };

            // Log the detailed error for debugging
            let detailed_error = anyhow::Error::new(e).context(context);
            error!(error = format!("{detailed_error:#}"); "address validation error");

            graphql_error
        })
    }
}
