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

use crate::{
    domain::{self, storage::Storage},
    infra::api::{
        ApiResult, ContextExt, OptionExt, ResultExt,
        v1::{
            AsBytesExt, HexEncoded,
            block::Block,
            contract_action::ContractAction,
            ledger_events::{DustLedgerEvent, ZswapLedgerEvent},
            unshielded::UnshieldedUtxo,
        },
    },
};
use async_graphql::{ComplexObject, Context, Enum, Interface, OneofObject, SimpleObject};
use derive_more::Debug;
use indexer_common::domain::{BlockHash, LedgerEventGrouping};
use std::marker::PhantomData;

/// A Midnight transaction.
#[derive(Debug, Clone, Interface)]
#[allow(clippy::duplicated_attributes)]
#[graphql(
    field(name = "id", ty = "&u64"),
    field(name = "hash", ty = "&HexEncoded"),
    field(name = "protocol_version", ty = "&u32"),
    field(name = "raw", ty = "&HexEncoded"),
    field(name = "block", ty = "ApiResult<Block<S>>"),
    field(name = "contract_actions", ty = "ApiResult<Vec<ContractAction<S>>>"),
    field(
        name = "unshielded_created_outputs",
        ty = "ApiResult<Vec<UnshieldedUtxo<S>>>"
    ),
    field(
        name = "unshielded_spent_outputs",
        ty = "ApiResult<Vec<UnshieldedUtxo<S>>>"
    ),
    field(name = "zswap_ledger_events", ty = "ApiResult<Vec<ZswapLedgerEvent>>"),
    field(name = "dust_ledger_events", ty = "ApiResult<Vec<DustLedgerEvent>>")
)]
pub enum Transaction<S: Storage> {
    /// A regular Midnight transaction.
    Regular(RegularTransaction<S>),

    /// A system Midnight transaction.
    System(SystemTransaction<S>),
}

impl<S> From<domain::Transaction> for Transaction<S>
where
    S: Storage,
{
    fn from(transaction: domain::Transaction) -> Self {
        match transaction {
            domain::Transaction::Regular(t) => Transaction::Regular(t.into()),
            domain::Transaction::System(t) => Transaction::System(t.into()),
        }
    }
}

/// A regular Midnight transaction.
#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex)]
pub struct RegularTransaction<S>
where
    S: Storage,
{
    /// The transaction ID.
    id: u64,

    /// The hex-encoded transaction hash.
    hash: HexEncoded,

    /// The protocol version.
    protocol_version: u32,

    /// The hex-encoded serialized transaction content.
    #[debug(skip)]
    raw: HexEncoded,

    #[graphql(skip)]
    block_hash: BlockHash,

    /// The result of applying this transaction to the ledger state.
    transaction_result: TransactionResult,

    /// The hex-encoded serialized transaction identifiers.
    #[debug(skip)]
    identifiers: Vec<HexEncoded>,

    /// The hex-encoded serialized merkle-tree root.
    #[debug(skip)]
    merkle_tree_root: HexEncoded,

    /// The zswap state start index.
    start_index: u64,

    /// The zswap state end index.
    end_index: u64,

    /// Fee information for this transaction.
    fees: TransactionFees,

    #[graphql(skip)]
    #[debug(skip)]
    _s: PhantomData<S>,
}

#[ComplexObject]
impl<S> RegularTransaction<S>
where
    S: Storage,
{
    /// The block for this transaction.
    async fn block(&self, cx: &Context<'_>) -> ApiResult<Block<S>> {
        block(self.block_hash, cx).await
    }

    /// The contract actions for this transaction.
    async fn contract_actions(&self, cx: &Context<'_>) -> ApiResult<Vec<ContractAction<S>>> {
        contract_actions(self.id, cx).await
    }

    /// Unshielded UTXOs created by this transaction.
    async fn unshielded_created_outputs(
        &self,
        cx: &Context<'_>,
    ) -> ApiResult<Vec<UnshieldedUtxo<S>>> {
        unshielded_created_outputs(self.id, cx).await
    }

    /// Unshielded UTXOs spent (consumed) by this transaction.
    async fn unshielded_spent_outputs(
        &self,
        cx: &Context<'_>,
    ) -> ApiResult<Vec<UnshieldedUtxo<S>>> {
        unshielded_spent_outputs(self.id, cx).await
    }

    /// Zswap ledger events of this transaction.
    async fn zswap_ledger_events(&self, cx: &Context<'_>) -> ApiResult<Vec<ZswapLedgerEvent>> {
        zswap_ledger_events::<S>(self.id, cx).await
    }

    /// Dust ledger events of this transaction.
    async fn dust_ledger_events(&self, cx: &Context<'_>) -> ApiResult<Vec<DustLedgerEvent>> {
        dust_ledger_events::<S>(self.id, cx).await
    }
}

impl<S> From<domain::RegularTransaction> for RegularTransaction<S>
where
    S: Storage,
{
    fn from(transaction: domain::RegularTransaction) -> Self {
        let domain::RegularTransaction {
            id,
            hash,
            protocol_version,
            raw,
            block_hash,
            transaction_result,
            identifiers,
            merkle_tree_root,
            start_index,
            end_index,
            ..
        } = transaction;

        // Use fees information from database (calculated by chain-indexer)
        let fees = TransactionFees {
            paid_fees: transaction
                .paid_fees
                .map(|f| f.to_string())
                .unwrap_or_else(|| "0".to_owned()),
            estimated_fees: transaction
                .estimated_fees
                .map(|f| f.to_string())
                .unwrap_or_else(|| "0".to_owned()),
        };

        Self {
            id,
            hash: hash.hex_encode(),
            protocol_version: protocol_version.0,
            raw: raw.hex_encode(),
            block_hash,
            transaction_result: transaction_result.into(),
            fees,
            identifiers: identifiers
                .into_iter()
                .map(|identifier| identifier.hex_encode())
                .collect::<Vec<_>>(),
            merkle_tree_root: merkle_tree_root.hex_encode(),
            start_index,
            end_index,
            _s: PhantomData,
        }
    }
}

/// A system Midnight transaction.
#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex)]
pub struct SystemTransaction<S>
where
    S: Storage,
{
    /// The transaction ID.
    id: u64,

    /// The hex-encoded transaction hash.
    hash: HexEncoded,

    /// The protocol version.
    protocol_version: u32,

    /// The hex-encoded serialized transaction content.
    #[debug(skip)]
    raw: HexEncoded,

    #[graphql(skip)]
    block_hash: BlockHash,

    #[graphql(skip)]
    #[debug(skip)]
    _s: PhantomData<S>,
}

// TODO: This duplicates the ComplexObject implementation for RegularTransaction which is necessary
// for async-graphql's #[ComplexObject] macro to work with GraphQL interfaces. Revisit when
// async-graphql provides better support for shared implementations across interface types.
#[ComplexObject]
impl<S> SystemTransaction<S>
where
    S: Storage,
{
    /// The block for this transaction.
    async fn block(&self, cx: &Context<'_>) -> ApiResult<Block<S>> {
        block(self.block_hash, cx).await
    }

    /// The contract actions for this transaction.
    async fn contract_actions(&self, cx: &Context<'_>) -> ApiResult<Vec<ContractAction<S>>> {
        contract_actions(self.id, cx).await
    }

    /// Unshielded UTXOs created by this transaction.
    async fn unshielded_created_outputs(
        &self,
        cx: &Context<'_>,
    ) -> ApiResult<Vec<UnshieldedUtxo<S>>> {
        unshielded_created_outputs(self.id, cx).await
    }

    /// Unshielded UTXOs spent (consumed) by this transaction.
    async fn unshielded_spent_outputs(
        &self,
        cx: &Context<'_>,
    ) -> ApiResult<Vec<UnshieldedUtxo<S>>> {
        unshielded_spent_outputs(self.id, cx).await
    }

    /// Zswap ledger events of this transaction.
    async fn zswap_ledger_events(&self, cx: &Context<'_>) -> ApiResult<Vec<ZswapLedgerEvent>> {
        zswap_ledger_events::<S>(self.id, cx).await
    }

    /// Dust ledger events of this transaction.
    async fn dust_ledger_events(&self, cx: &Context<'_>) -> ApiResult<Vec<DustLedgerEvent>> {
        dust_ledger_events::<S>(self.id, cx).await
    }
}

impl<S> From<domain::SystemTransaction> for SystemTransaction<S>
where
    S: Storage,
{
    fn from(transaction: domain::SystemTransaction) -> Self {
        let domain::SystemTransaction {
            id,
            hash,
            protocol_version,
            raw,
            block_hash,
        } = transaction;

        Self {
            id,
            hash: hash.hex_encode(),
            protocol_version: protocol_version.0,
            raw: raw.hex_encode(),
            block_hash,
            _s: PhantomData,
        }
    }
}

/// Either a transaction hash or a transaction identifier.
#[derive(Debug, Clone, OneofObject)]
pub enum TransactionOffset {
    /// A hex-encoded transaction hash.
    Hash(HexEncoded),

    /// A hex-encoded transaction identifier.
    Identifier(HexEncoded),
}

/// The result of applying a transaction to the ledger state. In case of a partial success (status),
/// there will be segments.
#[derive(Debug, Clone, SimpleObject)]
pub struct TransactionResult {
    pub status: TransactionResultStatus,
    pub segments: Option<Vec<Segment>>,
}

/// The status of the transaction result: success, partial success or failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum TransactionResultStatus {
    Success,
    PartialSuccess,
    Failure,
}

/// One of many segments for a partially successful transaction result showing success for some
/// segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, SimpleObject)]
pub struct Segment {
    /// Segment ID.
    id: u16,

    /// Successful or not.
    success: bool,
}

/// Fees information for a transaction, including both paid and estimated fees.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct TransactionFees {
    /// The actual fees paid for this transaction in DUST.
    paid_fees: String,
    /// The estimated fees that was calculated for this transaction in DUST.
    estimated_fees: String,
}

/// Result for a specific segment within a transaction.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
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

async fn block<S>(block_hash: BlockHash, cx: &Context<'_>) -> ApiResult<Block<S>>
where
    S: Storage,
{
    let block = cx
        .get_storage::<S>()
        .get_block_by_hash(block_hash)
        .await
        .map_err_into_server_error(|| format!("get block by hash {}", block_hash))?
        .ok_or_server_error(|| format!("block with hash {} not found", block_hash))?;

    Ok(block.into())
}

async fn contract_actions<S>(id: u64, cx: &Context<'_>) -> ApiResult<Vec<ContractAction<S>>>
where
    S: Storage,
{
    let contract_actions = cx
        .get_storage::<S>()
        .get_contract_actions_by_transaction_id(id)
        .await
        .map_err_into_server_error(|| {
            format!("cannot get contract actions by transaction ID {id}")
        })?;

    Ok(contract_actions.into_iter().map(Into::into).collect())
}

async fn unshielded_created_outputs<S>(
    id: u64,
    cx: &Context<'_>,
) -> ApiResult<Vec<UnshieldedUtxo<S>>>
where
    S: Storage,
{
    let utxos = cx
        .get_storage::<S>()
        .get_unshielded_utxos_created_by_transaction(id)
        .await
        .map_err_into_server_error(|| {
            format!("cannot get unshielded UTXOs created by transaction with ID {id}")
        })?
        .into_iter()
        .map(|utxo| UnshieldedUtxo::<S>::from((utxo, cx.get_network_id())))
        .collect();

    Ok(utxos)
}

async fn unshielded_spent_outputs<S>(id: u64, cx: &Context<'_>) -> ApiResult<Vec<UnshieldedUtxo<S>>>
where
    S: Storage,
{
    let utxos = cx
        .get_storage::<S>()
        .get_unshielded_utxos_spent_by_transaction(id)
        .await
        .map_err_into_server_error(|| {
            format!("cannot get unshielded UTXOs spent by transaction with ID {id}")
        })?
        .into_iter()
        .map(|utxo| UnshieldedUtxo::<S>::from((utxo, cx.get_network_id())))
        .collect();

    Ok(utxos)
}

async fn zswap_ledger_events<S>(id: u64, cx: &Context<'_>) -> ApiResult<Vec<ZswapLedgerEvent>>
where
    S: Storage,
{
    let zswap_ledger_events = cx
        .get_storage::<S>()
        .get_ledger_events_by_transaction_id(LedgerEventGrouping::Zswap, id)
        .await
        .map_err_into_server_error(|| {
            format!("cannot get zswap ledger events for transaction with ID {id}")
        })?
        .into_iter()
        .map(Into::into)
        .collect();

    Ok(zswap_ledger_events)
}

async fn dust_ledger_events<S>(id: u64, cx: &Context<'_>) -> ApiResult<Vec<DustLedgerEvent>>
where
    S: Storage,
{
    let dust_ledger_events = cx
        .get_storage::<S>()
        .get_ledger_events_by_transaction_id(LedgerEventGrouping::Dust, id)
        .await
        .map_err_into_server_error(|| {
            format!("cannot get dust ledger events for transaction with ID {id}")
        })?
        .into_iter()
        .map(Into::into)
        .collect();

    Ok(dust_ledger_events)
}
