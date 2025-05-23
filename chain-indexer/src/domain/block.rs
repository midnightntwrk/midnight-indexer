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

use crate::domain::Transaction;
use derive_more::{From, derive::AsRef};
use indexer_common::domain::{BlockAuthor, ProtocolVersion};
use midnight_ledger::transient_crypto::merkle_tree::MerkleTreeDigest;
use std::fmt::{self, Debug, Display};
use subxt::utils::H256;

/// Relevant block data from the perspective of the Chain Indexer.
#[derive(Debug, Clone)]
pub struct Block {
    pub hash: BlockHash,
    pub height: u32,
    pub protocol_version: ProtocolVersion,
    pub parent_hash: BlockHash,
    pub author: Option<BlockAuthor>,
    pub timestamp: u64,
    pub zswap_state_root: MerkleTreeDigest,
    pub transactions: Vec<Transaction>,
}

#[derive(Debug, Clone, Copy)]
pub struct BlockInfo {
    pub hash: BlockHash,
    pub height: u32,
}

impl From<&Block> for BlockInfo {
    fn from(block: &Block) -> Self {
        Self {
            hash: block.hash,
            height: block.height,
        }
    }
}

/// Hash for a [Block].
#[derive(Clone, Copy, Default, PartialEq, Eq, From, AsRef)]
#[as_ref([u8])]
pub struct BlockHash(pub H256);

impl Debug for BlockHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hex_encoded = const_hex::encode(self.as_ref());

        if hex_encoded.len() <= 8 {
            write!(f, "BlockHash({hex_encoded})")
        } else {
            write!(f, "BlockHash({}…)", &hex_encoded[0..8])
        }
    }
}

impl Display for BlockHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hex_encoded = const_hex::encode(self.as_ref());
        write!(f, "{hex_encoded}")
    }
}
