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

use crate::domain::{ContractAction, UnshieldedUtxo};
use indexer_common::domain::{
    ByteArray, Identifier, MerkleTreeRoot, ProtocolVersion, RawTransaction, TransactionHash,
    TransactionResult,
};
use sqlx::FromRow;
use std::fmt::Debug;

/// Relevant transaction data from the perspective of the Chain Indexer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaction {
    pub id: u64, // 0 = not yet saved; valid DB IDs start from 1
    pub hash: TransactionHash,
    pub protocol_version: ProtocolVersion,
    pub transaction_result: TransactionResult,
    pub identifiers: Vec<Identifier>,
    pub raw: RawTransaction,
    pub contract_actions: Vec<ContractAction>,
    pub created_unshielded_utxos: Vec<UnshieldedUtxo>,
    pub spent_unshielded_utxos: Vec<UnshieldedUtxo>,
    pub merkle_tree_root: MerkleTreeRoot,
    pub start_index: u64,
    pub end_index: u64,
    pub paid_fees: u128,
    pub estimated_fees: u128,
}

/// All raw transactions from a single block along with metadata needed for ledger state
/// application.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct BlockTransactions {
    pub transactions: Vec<RawTransaction>,

    pub block_parent_hash: ByteArray<32>,

    #[sqlx(try_from = "i64")]
    pub block_timestamp: u64,
}
