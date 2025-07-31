// This file is part of midnight-indexer.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use indexer_common::domain::{
    ByteVec, CardanoStakeKey, DustAddress, DustMerkleRoot, DustMerkleUpdate, DustNonce,
    DustNullifier, DustOwner, DustPrefix, NightUtxoHash, TransactionHash,
};
use serde::{Deserialize, Serialize};

/// DUST system state containing current Merkle tree roots and statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DustSystemState {
    /// Current commitment tree root.
    pub commitment_tree_root: DustMerkleRoot,

    /// Current generation tree root.
    pub generation_tree_root: DustMerkleRoot,

    /// Current block height.
    pub block_height: u32,

    /// Current timestamp.
    pub timestamp: u64,

    /// Total number of registrations.
    pub total_registrations: u32,
}

/// DUST generation status for a specific Cardano stake key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DustGenerationStatus {
    /// Cardano stake key.
    pub cardano_stake_key: CardanoStakeKey,

    /// Associated DUST address if registered.
    pub dust_address: Option<DustAddress>,

    /// Whether this stake key is registered.
    pub is_registered: bool,

    /// Generation rate in Specks per second.
    pub generation_rate: u128,

    /// Current DUST capacity.
    pub current_capacity: u128,

    /// NIGHT balance backing generation.
    pub night_balance: u128,
}

/// Type of Merkle tree.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DustMerkleTreeType {
    /// Commitment Merkle tree.
    Commitment,
    /// Generation Merkle tree.
    Generation,
}

/// DUST generation information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DustGenerationInfo {
    /// Night UTXO hash (or cNIGHT hash for Cardano).
    pub night_utxo_hash: NightUtxoHash,

    /// Generation value in Specks.
    pub value: u128,

    /// DUST public key of owner.
    pub owner: DustOwner,

    /// Initial nonce for DUST chain.
    pub nonce: DustNonce,

    /// Creation time (UNIX timestamp).
    pub ctime: u32,

    /// Destruction time. None if still generating.
    pub dtime: Option<u32>,

    /// Index in generation Merkle tree.
    pub merkle_index: u32,
}

/// DUST generation Merkle tree update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DustGenerationMerkleUpdate {
    /// Tree index.
    pub index: u32,

    /// Collapsed update data.
    pub collapsed_update: DustMerkleUpdate,

    /// Block height of update.
    pub block_height: u32,
}

/// DUST generation progress information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DustGenerationProgress {
    /// Highest processed index.
    pub highest_index: u32,

    /// Number of active generations.
    pub active_generations: u32,
}

/// DUST generation event union type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DustGenerationEvent {
    /// Generation information.
    Info(DustGenerationInfo),
    /// Merkle tree update.
    MerkleUpdate(DustGenerationMerkleUpdate),
    /// Progress update.
    Progress(DustGenerationProgress),
}

/// Transaction containing DUST nullifiers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DustNullifierTransaction {
    /// Transaction hash.
    pub transaction_hash: TransactionHash,

    /// Block height.
    pub block_height: u32,

    /// Matching nullifier prefixes.
    pub matching_nullifier_prefixes: Vec<DustPrefix>,
}

/// DUST nullifier transaction progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DustNullifierTransactionProgress {
    /// Highest processed block.
    pub highest_block: u32,

    /// Number of matched transactions.
    pub matched_count: u32,
}

/// DUST nullifier transaction event union type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DustNullifierTransactionEvent {
    /// Transaction with nullifiers.
    Transaction(DustNullifierTransaction),
    /// Progress update.
    Progress(DustNullifierTransactionProgress),
}

/// DUST commitment information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DustCommitmentInfo {
    /// DUST commitment.
    pub commitment: indexer_common::domain::DustCommitment,

    /// DUST nullifier (if spent).
    pub nullifier: Option<DustNullifier>,

    /// Initial value.
    pub value: u128,

    /// DUST address of owner.
    pub owner: DustOwner,

    /// Nonce.
    pub nonce: DustNonce,

    /// Creation timestamp.
    pub created_at: u32,

    /// Spend timestamp (if spent).
    pub spent_at: Option<u32>,
}

/// DUST commitment Merkle tree update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DustCommitmentMerkleUpdate {
    /// Tree index.
    pub index: u32,

    /// Collapsed update data.
    pub collapsed_update: DustMerkleUpdate,

    /// Block height of update.
    pub block_height: u32,
}

/// DUST commitment progress information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DustCommitmentProgress {
    /// Highest processed index.
    pub highest_index: u32,

    /// Number of commitments in batch.
    pub commitment_count: u32,
}

/// DUST commitment event union type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DustCommitmentEvent {
    /// Commitment information.
    Commitment(DustCommitmentInfo),
    /// Merkle tree update.
    MerkleUpdate(DustCommitmentMerkleUpdate),
    /// Progress update.
    Progress(DustCommitmentProgress),
}

/// Address type for registration queries.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AddressType {
    /// Night address.
    Night,
    /// DUST address.
    Dust,
    /// Cardano stake key.
    CardanoStake,
}

/// Registration address input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationAddress {
    /// Type of address.
    pub address_type: AddressType,

    /// Address value.
    pub value: ByteVec,
}

/// Registration update information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationUpdate {
    /// Cardano stake key.
    pub cardano_stake_key: CardanoStakeKey,

    /// DUST address.
    pub dust_address: DustAddress,

    /// Whether this registration is active.
    pub is_active: bool,

    /// Registration timestamp.
    pub registered_at: u32,

    /// Removal timestamp (if removed).
    pub removed_at: Option<u32>,
}

/// Registration update progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationUpdateProgress {
    /// Latest processed timestamp.
    pub latest_timestamp: u32,

    /// Number of updates in batch.
    pub update_count: u32,
}

/// Registration update event union type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RegistrationUpdateEvent {
    /// Registration update.
    Update(RegistrationUpdate),
    /// Progress update.
    Progress(RegistrationUpdateProgress),
}
