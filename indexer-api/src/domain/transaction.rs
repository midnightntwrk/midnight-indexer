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

use crate::domain::{BlockHash, UnshieldedUtxo};
use derive_more::Debug;
use indexer_common::domain::{
    Identifier, MerkleTreeRoot, ProtocolVersion, RawTransaction, TransactionHash, TransactionResult,
};
use sqlx::FromRow;

/// Relevant transaction data from the perspective of the Indexer API.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct Transaction {
    #[sqlx(try_from = "i64")]
    pub id: u64,

    #[cfg_attr(feature = "standalone", sqlx(try_from = "&'a [u8]"))]
    pub hash: TransactionHash,

    #[cfg_attr(feature = "standalone", sqlx(try_from = "&'a [u8]"))]
    pub block_hash: BlockHash,

    #[sqlx(try_from = "i64")]
    pub protocol_version: ProtocolVersion,

    #[sqlx(json)]
    pub transaction_result: TransactionResult,

    #[debug(skip)]
    #[cfg_attr(feature = "standalone", sqlx(skip))]
    pub identifiers: Vec<Identifier>,

    #[debug(skip)]
    pub raw: RawTransaction,

    #[debug(skip)]
    pub merkle_tree_root: MerkleTreeRoot,

    #[sqlx(try_from = "i64")]
    pub start_index: u64,

    #[sqlx(try_from = "i64")]
    pub end_index: u64,

    #[sqlx(skip)]
    pub unshielded_created_outputs: Vec<UnshieldedUtxo>,

    #[sqlx(skip)]
    pub unshielded_spent_outputs: Vec<UnshieldedUtxo>,
}
