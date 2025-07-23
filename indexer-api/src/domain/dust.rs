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

use serde::{Deserialize, Serialize};

/// DUST system state containing current Merkle tree roots and statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DustSystemState {
    /// Current commitment tree root.
    pub commitment_tree_root: String,
    /// Current generation tree root.
    pub generation_tree_root: String,
    /// Current block height.
    pub block_height: i32,
    /// Current timestamp.
    pub timestamp: i64,
    /// Total number of registrations.
    pub total_registrations: i32,
}

/// DUST generation status for a specific Cardano stake key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DustGenerationStatus {
    /// Cardano stake key.
    pub cardano_stake_key: String,
    /// Associated DUST address if registered.
    pub dust_address: Option<String>,
    /// Whether this stake key is registered.
    pub is_registered: bool,
    /// Generation rate in Specks per second.
    pub generation_rate: String,
    /// Current DUST capacity.
    pub current_capacity: String,
    /// NIGHT balance backing generation.
    pub night_balance: String,
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
    pub night_utxo_hash: String,
    /// Generation value in Specks (u128 as string).
    pub value: String,
    /// DUST public key of owner.
    pub owner: String,
    /// Initial nonce for DUST chain.
    pub nonce: String,
    /// Creation time (UNIX timestamp).
    pub ctime: i32,
    /// Destruction time. None if still generating.
    pub dtime: Option<i32>,
    /// Index in generation Merkle tree.
    pub merkle_index: i32,
}

/// DUST generation Merkle tree update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DustGenerationMerkleUpdate {
    /// Tree index.
    pub index: i32,
    /// Collapsed update data.
    pub collapsed_update: String,
    /// Block height of update.
    pub block_height: i32,
}

/// DUST generation progress information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DustGenerationProgress {
    /// Highest processed index.
    pub highest_index: i32,
    /// Number of active generations.
    pub active_generations: i32,
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
    pub transaction_hash: String,
    /// Block height.
    pub block_height: i32,
    /// Matching nullifier prefixes.
    pub matching_nullifier_prefixes: Vec<String>,
}

/// DUST nullifier transaction progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DustNullifierTransactionProgress {
    /// Highest processed block.
    pub highest_block: i32,
    /// Number of matched transactions.
    pub matched_count: i32,
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
pub struct DustCommitment {
    /// DUST commitment.
    pub commitment: String,
    /// DUST nullifier (if spent).
    pub nullifier: Option<String>,
    /// Initial value.
    pub value: String,
    /// DUST address of owner.
    pub owner: String,
    /// Nonce.
    pub nonce: String,
    /// Creation timestamp.
    pub created_at: i32,
    /// Spend timestamp (if spent).
    pub spent_at: Option<i32>,
}

/// DUST commitment Merkle tree update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DustCommitmentMerkleUpdate {
    /// Tree index.
    pub index: i32,
    /// Collapsed update data.
    pub collapsed_update: String,
    /// Block height of update.
    pub block_height: i32,
}

/// DUST commitment progress information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DustCommitmentProgress {
    /// Highest processed index.
    pub highest_index: i32,
    /// Number of commitments in batch.
    pub commitment_count: i32,
}

/// DUST commitment event union type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DustCommitmentEvent {
    /// Commitment information.
    Commitment(DustCommitment),
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
    pub value: String,
}

/// Registration update information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationUpdate {
    /// Cardano stake key.
    pub cardano_stake_key: String,
    /// DUST address.
    pub dust_address: String,
    /// Whether this registration is active.
    pub is_active: bool,
    /// Registration timestamp.
    pub registered_at: i32,
    /// Removal timestamp (if removed).
    pub removed_at: Option<i32>,
}

/// Registration update progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationUpdateProgress {
    /// Latest processed timestamp.
    pub latest_timestamp: i32,
    /// Number of updates in batch.
    pub update_count: i32,
}

/// Registration update event union type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RegistrationUpdateEvent {
    /// Registration update.
    Update(RegistrationUpdate),
    /// Progress update.
    Progress(RegistrationUpdateProgress),
}
