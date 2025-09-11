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

pub mod dust;
pub mod ledger;

mod address;
mod bytes;
mod ledger_state_storage;
mod network_id;
mod protocol_version;
mod pub_sub;
mod viewing_key;

pub use address::*;
pub use bytes::*;
pub use ledger_state_storage::*;
pub use network_id::*;
pub use protocol_version::*;
pub use pub_sub::*;
pub use viewing_key::*;

pub type BlockAuthor = ByteArray<32>;
pub type BlockHash = ByteArray<32>;

// DUST-specific types
pub type DustCommitment = ByteArray<32>;
pub type DustNonce = ByteArray<32>;
pub type DustNullifier = ByteArray<32>;
pub type DustOwner = ByteArray<32>;
pub type DustAddress = ByteArray<32>;
pub type CardanoStakeKey = ByteVec;
pub type DustMerkleRoot = ByteArray<32>;
pub type DustPrefix = ByteVec;
pub type DustMerkleUpdate = ByteVec;
pub type DustMerkleTreeData = ByteVec;  // Serialized merkle tree path data
pub type NightUtxoHash = ByteArray<32>;
pub type NightUtxoNonce = ByteArray<32>;
pub type DustUtxoId = ByteVec;
