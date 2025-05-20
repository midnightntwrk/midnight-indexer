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
use derive_more::From;
use indexer_common::domain::{
    ApplyStage, Identifier, MerkleTreeRoot, ProtocolVersion, RawTransaction,
};
use midnight_ledger::structure::TransactionHash as LedgerTransactionHash;
use sqlx::FromRow;
use std::fmt::{self, Debug, Display};

/// Relevant transaction data from the perspective of the Chain Indexer.
#[derive(Debug, Clone, FromRow)]
pub struct Transaction {
    #[sqlx(skip)]
    pub hash: TransactionHash,

    #[sqlx(skip)]
    pub protocol_version: ProtocolVersion,

    pub apply_stage: ApplyStage,

    #[sqlx(skip)]
    pub identifiers: Vec<Identifier>,

    pub raw: RawTransaction,

    #[sqlx(skip)]
    pub contract_actions: Vec<ContractAction>,

    #[sqlx(skip)]
    pub created_unshielded_utxos: Vec<UnshieldedUtxo>,

    #[sqlx(skip)]
    pub spent_unshielded_utxos: Vec<UnshieldedUtxo>,

    pub merkle_tree_root: MerkleTreeRoot,

    #[sqlx(try_from = "i64")]
    pub start_index: u64,

    #[sqlx(try_from = "i64")]
    pub end_index: u64,
}

/// Hash for a [Transaction].
#[derive(Default, Clone, Copy, PartialEq, Eq, From)]
pub struct TransactionHash(pub LedgerTransactionHash);

impl AsRef<[u8]> for TransactionHash {
    fn as_ref(&self) -> &[u8] {
        self.0.0.0.as_slice()
    }
}

impl Debug for TransactionHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hex_encoded = const_hex::encode(self.as_ref());
        if hex_encoded.len() <= 8 {
            write!(f, "TransactionHash({hex_encoded})")
        } else {
            write!(f, "TransactionHash({}…)", &hex_encoded[0..8])
        }
    }
}

impl Display for TransactionHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hex_encoded = const_hex::encode(self.as_ref());
        write!(f, "{hex_encoded}")
    }
}
