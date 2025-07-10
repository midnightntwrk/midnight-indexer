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
        ApiError, ApiResult, AsBytesExt, ContextExt, HexEncoded, OptionExt, ResultExt,
        v1::{block::Block, contract_action::ContractAction, unshielded::UnshieldedUtxo},
    },
};
use async_graphql::{ComplexObject, Context, Enum, OneofObject, SimpleObject};
use derive_more::Debug;
use indexer_common::domain::{BlockHash, ProtocolVersion};
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;

/// A transaction with its relevant data.
#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex)]
pub struct Transaction<S>
where
    S: Storage,
{
    /// The transaction ID.
    id: u64,

    /// The transaction hash.
    hash: HexEncoded,

    /// The protocol version.
    protocol_version: u32,

    /// The result of applying a transaction to the ledger state.
    transaction_result: TransactionResult,

    /// Fee information for this transaction.
    fees: TransactionFees,

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
    block_hash: BlockHash,

    #[graphql(skip)]
    #[debug(skip)]
    _s: PhantomData<S>,
}

// Needed to derive `Interface` for `ContractAction`. Weird!
impl<S> From<Result<Transaction<S>, ApiError>> for Transaction<S>
where
    S: Storage,
{
    fn from(_value: Result<Transaction<S>, ApiError>) -> Self {
        unimplemented!()
    }
}

#[ComplexObject]
impl<S> Transaction<S>
where
    S: Storage,
{
    /// The block for this transaction.
    async fn block(&self, cx: &Context<'_>) -> ApiResult<Block<S>> {
        let block = cx
            .get_storage::<S>()
            .get_block_by_hash(self.block_hash)
            .await
            .map_err_into_server_error(|| format!("get block by hash {}", self.block_hash))?
            .ok_or_server_error(|| format!("block with hash {} not found", self.block_hash))?;

        Ok(block.into())
    }

    /// The contract actions.
    async fn contract_actions(&self, cx: &Context<'_>) -> ApiResult<Vec<ContractAction<S>>> {
        let id = self.id;

        let contract_actions = cx
            .get_storage::<S>()
            .get_contract_actions_by_transaction_id(id)
            .await
            .map_err_into_server_error(|| {
                format!("cannot get contract actions by transaction ID {id}")
            })?;

        Ok(contract_actions.into_iter().map(Into::into).collect())
    }

    /// Unshielded UTXOs created by this transaction.
    async fn unshielded_created_outputs(
        &self,
        cx: &Context<'_>,
    ) -> ApiResult<Vec<UnshieldedUtxo<S>>> {
        let id = self.id;

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

    /// Unshielded UTXOs spent (consumed) by this transaction.
    async fn unshielded_spent_outputs(
        &self,
        cx: &Context<'_>,
    ) -> ApiResult<Vec<UnshieldedUtxo<S>>> {
        let id = self.id;

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

        Self {
            hash: hash.hex_encode(),
            protocol_version,
            transaction_result: transaction_result.into(),
            fees,
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

/// Either a transaction hash or a transaction identifier.
#[derive(Debug, OneofObject)]
pub enum TransactionOffset {
    /// A hex-encoded transaction hash.
    Hash(HexEncoded),

    /// A hex-encoded transaction identifier.
    Identifier(HexEncoded),
}

/// The result of applying a transaction to the ledger state. In case of a partial success (status),
/// there will be segments.
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
