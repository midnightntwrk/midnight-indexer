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

use indexer_common::domain::{
    DustCommitment as DustCommitmentHash, DustNonce, DustNullifier, DustOwner, NightUtxoHash,
};
use serde::{Deserialize, Serialize};

/// Current DUST system state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DustSystemState {
    /// Root of the commitment Merkle tree.
    pub commitment_tree_root: String,
    /// Root of the generation Merkle tree.
    pub generation_tree_root: String,
    /// Current block height.
    pub block_height: u32,
    /// Current timestamp.
    pub timestamp: i64,
    /// Total number of registrations.
    pub total_registrations: u32,
}

/// DUST generation status for a specific stake key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DustGenerationStatus {
    /// Cardano stake key.
    pub cardano_stake_key: String,
    /// DUST address if registered.
    pub dust_address: Option<String>,
    /// Whether this stake key is registered.
    pub is_registered: bool,
    /// Generation rate in Specks per second.
    pub generation_rate: String,
    /// Current DUST capacity.
    pub current_capacity: String,
    /// Night balance backing generation.
    pub night_balance: String,
}

/// DUST generation information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DustGenerationInfo {
    /// Night UTXO hash.
    pub night_utxo_hash: NightUtxoHash,
    /// Generation value in Specks.
    pub value: u128,
    /// DUST public key of owner.
    pub owner: DustOwner,
    /// Initial nonce for DUST chain.
    pub nonce: DustNonce,
    /// Creation time.
    pub ctime: i64,
    /// Destruction time.
    pub dtime: Option<i64>,
    /// Index in generation Merkle tree.
    pub merkle_index: u64,
}

/// DUST generation Merkle tree update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DustGenerationMerkleUpdate {
    /// Index in the tree.
    pub index: u64,
    /// Collapsed update data.
    pub collapsed_update: Vec<u8>,
    /// Block height when update occurred.
    pub block_height: u32,
}

/// DUST generation progress information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DustGenerationProgress {
    /// Highest generation index.
    pub highest_index: u64,
    /// Number of active generations.
    pub active_generations: u32,
}

/// DUST nullifier transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DustNullifierTransaction {
    /// The transaction ID.
    pub transaction_id: u64,
    /// The transaction hash.
    pub transaction_hash: String,
    /// Matching nullifier prefixes.
    pub matching_nullifier_prefixes: Vec<String>,
}

/// DUST nullifier transaction progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DustNullifierTransactionProgress {
    /// Highest processed block.
    pub highest_block: u32,
    /// Number of matched transactions.
    pub matched_count: u32,
}

/// DUST commitment information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DustCommitment {
    /// DUST commitment.
    pub commitment: DustCommitmentHash,
    /// DUST nullifier if spent.
    pub nullifier: Option<DustNullifier>,
    /// Initial value.
    pub value: u128,
    /// DUST address.
    pub owner: DustOwner,
    /// Nonce.
    pub nonce: DustNonce,
    /// Creation timestamp.
    pub created_at: i64,
    /// Spending timestamp.
    pub spent_at: Option<i64>,
}

/// DUST commitment Merkle tree update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DustCommitmentMerkleUpdate {
    /// Index in the tree.
    pub index: u64,
    /// Collapsed update data.
    pub collapsed_update: Vec<u8>,
    /// Block height when update occurred.
    pub block_height: u32,
}

/// DUST commitment progress information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DustCommitmentProgress {
    /// Highest commitment index.
    pub highest_index: u64,
    /// Total number of commitments.
    pub total_commitments: u32,
}

/// Registration update event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationUpdate {
    /// Address type.
    pub address_type: indexer_common::domain::AddressType,
    /// Address value.
    pub address_value: String,
    /// Related addresses.
    pub related_addresses: RelatedAddresses,
    /// Whether registration is active.
    pub is_active: bool,
    /// Timestamp.
    pub timestamp: i64,
}

/// Related addresses in registration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedAddresses {
    /// Night address.
    pub night_address: Option<String>,
    /// DUST address.
    pub dust_address: Option<String>,
    /// Cardano stake key.
    pub cardano_stake_key: Option<String>,
}

/// Registration update progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationUpdateProgress {
    /// Number of processed registrations.
    pub processed_count: u32,
    /// Current timestamp.
    pub timestamp: i64,
}

/// Event unions for streaming subscriptions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DustGenerationEvent {
    /// DUST generation information.
    DustGenerationInfo(DustGenerationInfo),
    /// Merkle tree update.
    DustGenerationMerkleUpdate(DustGenerationMerkleUpdate),
    /// Progress information.
    DustGenerationProgress(DustGenerationProgress),
}

/// DUST nullifier transaction events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DustNullifierTransactionEvent {
    /// Transaction with nullifiers.
    DustNullifierTransaction(DustNullifierTransaction),
    /// Progress information.
    DustNullifierTransactionProgress(DustNullifierTransactionProgress),
}

/// DUST commitment events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DustCommitmentEvent {
    /// DUST commitment.
    DustCommitment(DustCommitment),
    /// Merkle tree update.
    DustCommitmentMerkleUpdate(DustCommitmentMerkleUpdate),
    /// Progress information.
    DustCommitmentProgress(DustCommitmentProgress),
}

/// Registration update events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RegistrationUpdateEvent {
    /// Registration update.
    RegistrationUpdate(RegistrationUpdate),
    /// Progress information.
    RegistrationUpdateProgress(RegistrationUpdateProgress),
}
