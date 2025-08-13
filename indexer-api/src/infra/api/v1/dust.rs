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

//! GraphQL API types for DUST operations.

use crate::{
    domain,
    infra::api::{AsBytesExt, HexDecodeError, HexEncoded},
};
use async_graphql::{Enum, InputObject, SimpleObject, Union};
use serde::{Deserialize, Serialize};

/// DUST system state containing current Merkle tree roots and statistics.
#[derive(Debug, Clone, SimpleObject, Serialize, Deserialize)]
pub struct DustSystemState {
    /// Current commitment tree root.
    pub commitment_tree_root: HexEncoded,

    /// Current generation tree root.
    pub generation_tree_root: HexEncoded,

    /// Current block height.
    pub block_height: u32,

    /// Current timestamp.
    pub timestamp: u64,

    /// Total number of registrations.
    pub total_registrations: u32,
}

impl From<domain::dust::DustSystemState> for DustSystemState {
    fn from(state: domain::dust::DustSystemState) -> Self {
        Self {
            commitment_tree_root: state.commitment_tree_root.hex_encode(),
            generation_tree_root: state.generation_tree_root.hex_encode(),
            block_height: state.block_height,
            timestamp: state.timestamp,
            total_registrations: state.total_registrations,
        }
    }
}

/// DUST generation status for a specific Cardano stake key.
#[derive(Debug, Clone, SimpleObject, Serialize, Deserialize)]
pub struct DustGenerationStatus {
    /// Cardano stake key.
    pub cardano_stake_key: HexEncoded,

    /// Associated DUST address if registered.
    pub dust_address: Option<HexEncoded>,

    /// Whether this stake key is registered.
    pub is_registered: bool,

    /// Generation rate in Specks per second.
    pub generation_rate: String,

    /// Current DUST capacity.
    pub current_capacity: String,

    /// NIGHT balance backing generation.
    pub night_balance: String,
}

impl From<domain::dust::DustGenerationStatus> for DustGenerationStatus {
    fn from(status: domain::dust::DustGenerationStatus) -> Self {
        Self {
            cardano_stake_key: status.cardano_stake_key.hex_encode(),
            dust_address: status.dust_address.map(|addr| addr.hex_encode()),
            is_registered: status.is_registered,
            generation_rate: status.generation_rate.to_string(),
            current_capacity: status.current_capacity.to_string(),
            night_balance: status.night_balance.to_string(),
        }
    }
}

/// Type of Merkle tree.
#[derive(Debug, Clone, Copy, Enum, Serialize, Deserialize, PartialEq, Eq)]
pub enum DustMerkleTreeType {
    /// Commitment Merkle tree.
    Commitment,
    /// Generation Merkle tree.
    Generation,
}

impl From<domain::dust::DustMerkleTreeType> for DustMerkleTreeType {
    fn from(tree_type: domain::dust::DustMerkleTreeType) -> Self {
        match tree_type {
            domain::dust::DustMerkleTreeType::Commitment => Self::Commitment,
            domain::dust::DustMerkleTreeType::Generation => Self::Generation,
        }
    }
}

impl From<DustMerkleTreeType> for domain::dust::DustMerkleTreeType {
    fn from(tree_type: DustMerkleTreeType) -> Self {
        match tree_type {
            DustMerkleTreeType::Commitment => Self::Commitment,
            DustMerkleTreeType::Generation => Self::Generation,
        }
    }
}

/// DUST generation event union type.
#[derive(Debug, Clone, Union, Serialize, Deserialize)]
pub enum DustGenerationEvent {
    /// Generation information.
    Info(DustGenerationInfo),
    /// Merkle tree update.
    MerkleUpdate(DustGenerationMerkleUpdate),
    /// Progress update.
    Progress(DustGenerationProgress),
}

/// DUST generation information.
#[derive(Debug, Clone, SimpleObject, Serialize, Deserialize)]
pub struct DustGenerationInfo {
    /// Night UTXO hash (or cNIGHT hash for Cardano).
    pub night_utxo_hash: HexEncoded,

    /// Generation value in Specks (u128 as string).
    pub value: String,

    /// DUST public key of owner.
    pub owner: HexEncoded,

    /// Initial nonce for DUST chain.
    pub nonce: HexEncoded,

    /// Creation time (UNIX timestamp).
    pub ctime: u64,

    /// Destruction time. None if still generating.
    pub dtime: Option<u64>,

    /// Index in generation Merkle tree.
    pub merkle_index: u64,
}

impl From<domain::dust::DustGenerationInfo> for DustGenerationInfo {
    fn from(info: domain::dust::DustGenerationInfo) -> Self {
        Self {
            night_utxo_hash: info.night_utxo_hash.hex_encode(),
            value: info.value.to_string(),
            owner: info.owner.hex_encode(),
            nonce: info.nonce.hex_encode(),
            ctime: info.ctime,
            dtime: info.dtime,
            merkle_index: info.merkle_index,
        }
    }
}

/// DUST generation Merkle tree update.
#[derive(Debug, Clone, SimpleObject, Serialize, Deserialize)]
pub struct DustGenerationMerkleUpdate {
    /// Tree index.
    pub index: u64,

    /// Collapsed update data.
    pub collapsed_update: HexEncoded,

    /// Block height of update.
    pub block_height: u32,
}

impl From<domain::dust::DustGenerationMerkleUpdate> for DustGenerationMerkleUpdate {
    fn from(update: domain::dust::DustGenerationMerkleUpdate) -> Self {
        let domain::dust::DustGenerationMerkleUpdate {
            index,
            collapsed_update,
            block_height,
        } = update;

        Self {
            index,
            collapsed_update: collapsed_update.hex_encode(),
            block_height,
        }
    }
}

/// DUST generation progress information.
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct DustGenerationProgress {
    /// Highest processed index.
    pub highest_index: u64,

    /// Number of active generations.
    pub active_generation_count: u32,
}

impl From<domain::dust::DustGenerationEvent> for DustGenerationEvent {
    fn from(event: domain::dust::DustGenerationEvent) -> Self {
        match event {
            domain::dust::DustGenerationEvent::Info(info) => Self::Info(info.into()),

            domain::dust::DustGenerationEvent::MerkleUpdate(update) => {
                Self::MerkleUpdate(update.into())
            }
        }
    }
}

/// Transaction containing DUST nullifiers.
#[derive(Debug, Clone, SimpleObject, Serialize, Deserialize)]
pub struct DustNullifierTransaction {
    /// Transaction hash.
    pub transaction_hash: HexEncoded,

    /// Block height.
    pub block_height: u32,

    /// Matching nullifier prefixes.
    pub matching_nullifier_prefixes: Vec<HexEncoded>,
}

impl From<domain::dust::DustNullifierTransaction> for DustNullifierTransaction {
    fn from(tx: domain::dust::DustNullifierTransaction) -> Self {
        Self {
            transaction_hash: tx.transaction_hash.hex_encode(),
            block_height: tx.block_height,
            matching_nullifier_prefixes: tx
                .matching_nullifier_prefixes
                .into_iter()
                .map(|prefix| prefix.hex_encode())
                .collect(),
        }
    }
}

/// DUST nullifier transaction progress.
#[derive(Debug, Clone, SimpleObject, Serialize, Deserialize)]
pub struct DustNullifierTransactionProgress {
    /// Highest processed block.
    pub highest_block: u32,

    /// Number of matched transactions.
    pub matched_count: u32,
}

/// DUST nullifier transaction event union type.
#[derive(Debug, Clone, Union, Serialize, Deserialize)]
pub enum DustNullifierTransactionEvent {
    /// Transaction with nullifiers.
    Transaction(DustNullifierTransaction),
    /// Progress update.
    Progress(DustNullifierTransactionProgress),
}

impl From<domain::dust::DustNullifierTransactionEvent> for DustNullifierTransactionEvent {
    fn from(event: domain::dust::DustNullifierTransactionEvent) -> Self {
        match event {
            domain::dust::DustNullifierTransactionEvent::Transaction(tx) => {
                Self::Transaction(tx.into())
            }
        }
    }
}

/// DUST commitment information.
#[derive(Debug, Clone, SimpleObject, Serialize, Deserialize)]
pub struct DustCommitment {
    /// DUST commitment.
    pub commitment: HexEncoded,

    /// DUST nullifier (if spent).
    pub nullifier: Option<HexEncoded>,

    /// Initial value.
    pub value: String,

    /// DUST address of owner.
    pub owner: HexEncoded,

    /// Nonce.
    pub nonce: HexEncoded,

    /// Creation timestamp.
    pub created_at: u64,

    /// Spend timestamp (if spent).
    pub spent_at: Option<u64>,
}

impl From<domain::dust::DustCommitmentInfo> for DustCommitment {
    fn from(commitment: domain::dust::DustCommitmentInfo) -> Self {
        Self {
            commitment: commitment.commitment.hex_encode(),
            nullifier: commitment.nullifier.map(|n| n.hex_encode()),
            value: commitment.value.to_string(),
            owner: commitment.owner.hex_encode(),
            nonce: commitment.nonce.hex_encode(),
            created_at: commitment.created_at,
            spent_at: commitment.spent_at,
        }
    }
}

/// DUST commitment Merkle tree update.
#[derive(Debug, Clone, SimpleObject, Serialize, Deserialize)]
pub struct DustCommitmentMerkleUpdate {
    /// Tree index.
    pub index: u64,

    /// Collapsed update data.
    pub collapsed_update: HexEncoded,

    /// Block height of update.
    pub block_height: u32,
}

impl From<domain::dust::DustCommitmentMerkleUpdate> for DustCommitmentMerkleUpdate {
    fn from(update: domain::dust::DustCommitmentMerkleUpdate) -> Self {
        Self {
            index: update.index,
            collapsed_update: update.collapsed_update.hex_encode(),
            block_height: update.block_height,
        }
    }
}

/// DUST commitment progress information.
#[derive(Debug, Clone, SimpleObject, Serialize, Deserialize)]
pub struct DustCommitmentProgress {
    /// Highest processed index.
    pub highest_index: u64,

    /// Number of commitments in batch.
    pub commitment_count: u32,
}

/// DUST commitment event union type.
#[derive(Debug, Clone, Union, Serialize, Deserialize)]
pub enum DustCommitmentEvent {
    /// Commitment information.
    Commitment(DustCommitment),

    /// Merkle tree update.
    MerkleUpdate(DustCommitmentMerkleUpdate),

    /// Progress update.
    Progress(DustCommitmentProgress),
}

impl From<domain::dust::DustCommitmentEvent> for DustCommitmentEvent {
    fn from(event: domain::dust::DustCommitmentEvent) -> Self {
        match event {
            domain::dust::DustCommitmentEvent::Commitment(commitment) => {
                Self::Commitment(commitment.into())
            }

            domain::dust::DustCommitmentEvent::MerkleUpdate(update) => {
                Self::MerkleUpdate(update.into())
            }
        }
    }
}

/// Address type for registration queries.
#[derive(Debug, Clone, Copy, Enum, Serialize, Deserialize, PartialEq, Eq)]
pub enum AddressType {
    /// Night address.
    Night,

    /// DUST address.
    Dust,

    /// Cardano stake key.
    CardanoStake,
}

impl From<domain::dust::AddressType> for AddressType {
    fn from(address_type: domain::dust::AddressType) -> Self {
        match address_type {
            domain::dust::AddressType::Night => Self::Night,
            domain::dust::AddressType::Dust => Self::Dust,
            domain::dust::AddressType::CardanoStake => Self::CardanoStake,
        }
    }
}

impl From<AddressType> for domain::dust::AddressType {
    fn from(address_type: AddressType) -> Self {
        match address_type {
            AddressType::Night => Self::Night,
            AddressType::Dust => Self::Dust,
            AddressType::CardanoStake => Self::CardanoStake,
        }
    }
}

/// Registration address input.
#[derive(Debug, Clone, InputObject, Serialize, Deserialize)]
pub struct RegistrationAddress {
    /// Type of address.
    pub address_type: AddressType,

    /// Address value.
    pub value: HexEncoded,
}

impl From<domain::dust::RegistrationAddress> for RegistrationAddress {
    fn from(address: domain::dust::RegistrationAddress) -> Self {
        Self {
            address_type: address.address_type.into(),
            value: address.value.hex_encode(),
        }
    }
}

impl TryFrom<RegistrationAddress> for domain::dust::RegistrationAddress {
    type Error = HexDecodeError;

    fn try_from(address: RegistrationAddress) -> Result<Self, Self::Error> {
        let value = address.value.hex_decode()?;

        Ok(Self {
            address_type: address.address_type.into(),
            value,
        })
    }
}

/// Registration update event union type.
#[derive(Debug, Clone, Union, Serialize, Deserialize)]
pub enum RegistrationUpdateEvent {
    /// Registration update.
    Update(RegistrationUpdate),

    /// Progress update.
    Progress(RegistrationUpdateProgress),
}

/// Registration update information.
#[derive(Debug, Clone, SimpleObject, Serialize, Deserialize)]
pub struct RegistrationUpdate {
    /// Cardano stake key.
    pub cardano_stake_key: HexEncoded,

    /// DUST address.
    pub dust_address: HexEncoded,

    /// Whether this registration is active.
    pub is_active: bool,

    /// Registration timestamp.
    pub registered_at: u64,

    /// Removal timestamp (if removed).
    pub removed_at: Option<u64>,
}

impl From<domain::dust::RegistrationUpdate> for RegistrationUpdate {
    fn from(update: domain::dust::RegistrationUpdate) -> Self {
        Self {
            cardano_stake_key: update.cardano_stake_key.hex_encode(),
            dust_address: update.dust_address.hex_encode(),
            is_active: update.is_active,
            registered_at: update.registered_at,
            removed_at: update.removed_at,
        }
    }
}

/// Registration update progress.
#[derive(Debug, Clone, SimpleObject, Serialize, Deserialize)]
pub struct RegistrationUpdateProgress {
    /// Latest processed timestamp.
    pub latest_timestamp: u64,

    /// Number of updates in batch.
    pub update_count: u32,
}
