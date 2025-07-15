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
    domain::{
        DustCommitmentEvent, DustGenerationEvent,
        DustGenerationStatus as DomainDustGenerationStatus, DustNullifierTransactionEvent,
        DustSystemState as DomainDustSystemState, RegistrationUpdateEvent,
    },
    infra::api::{AsBytesExt, HexEncoded},
};
use async_graphql::{Enum, InputObject, SimpleObject, Union};
use indexer_common::domain::{AddressType, DustMerkleTreeType};
use serde::{Deserialize, Serialize};

/// Current DUST system state.
#[derive(Debug, SimpleObject)]
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

impl From<DomainDustSystemState> for DustSystemState {
    fn from(value: DomainDustSystemState) -> Self {
        Self {
            commitment_tree_root: value.commitment_tree_root,
            generation_tree_root: value.generation_tree_root,
            block_height: value.block_height,
            timestamp: value.timestamp,
            total_registrations: value.total_registrations,
        }
    }
}

/// DUST generation status for a specific stake key.
#[derive(Debug, SimpleObject)]
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

impl From<DomainDustGenerationStatus> for DustGenerationStatus {
    fn from(value: DomainDustGenerationStatus) -> Self {
        Self {
            cardano_stake_key: value.cardano_stake_key,
            dust_address: value.dust_address,
            is_registered: value.is_registered,
            generation_rate: value.generation_rate,
            current_capacity: value.current_capacity,
            night_balance: value.night_balance,
        }
    }
}

/// DUST Merkle tree type.
#[derive(Debug, Enum, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum DustMerkleTreeTypeGraphQL {
    /// Commitment tree.
    Commitment,
    /// Generation tree.
    Generation,
}

impl From<DustMerkleTreeTypeGraphQL> for DustMerkleTreeType {
    fn from(value: DustMerkleTreeTypeGraphQL) -> Self {
        match value {
            DustMerkleTreeTypeGraphQL::Commitment => DustMerkleTreeType::Commitment,
            DustMerkleTreeTypeGraphQL::Generation => DustMerkleTreeType::Generation,
        }
    }
}

/// Address type for registration queries.
#[derive(Debug, Enum, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum AddressTypeGraphQL {
    /// Night address.
    Night,
    /// DUST address.
    Dust,
    /// Cardano stake key.
    CardanoStake,
}

impl From<AddressTypeGraphQL> for AddressType {
    fn from(value: AddressTypeGraphQL) -> Self {
        match value {
            AddressTypeGraphQL::Night => AddressType::Night,
            AddressTypeGraphQL::Dust => AddressType::Dust,
            AddressTypeGraphQL::CardanoStake => AddressType::CardanoStake,
        }
    }
}

/// Registration address input.
#[derive(Debug, InputObject)]
pub struct RegistrationAddress {
    /// Address type.
    pub address_type: AddressTypeGraphQL,
    /// Address value.
    pub value: String,
}

/// DUST generation information.
#[derive(Debug, SimpleObject)]
pub struct DustGenerationInfo {
    /// Night UTXO hash.
    pub night_utxo_hash: HexEncoded,
    /// Generation value in Specks.
    pub value: String,
    /// DUST public key of owner.
    pub owner: String,
    /// Initial nonce for DUST chain.
    pub nonce: HexEncoded,
    /// Creation time.
    pub ctime: i64,
    /// Destruction time.
    pub dtime: Option<i64>,
    /// Index in generation Merkle tree.
    pub merkle_index: u64,
}

impl From<crate::domain::DustGenerationInfo> for DustGenerationInfo {
    fn from(value: crate::domain::DustGenerationInfo) -> Self {
        Self {
            night_utxo_hash: value.night_utxo_hash.hex_encode(),
            value: value.value.to_string(),
            owner: value.owner.hex_encode().0,
            nonce: value.nonce.hex_encode(),
            ctime: value.ctime,
            dtime: value.dtime,
            merkle_index: value.merkle_index,
        }
    }
}

/// DUST generation Merkle tree update.
#[derive(Debug, SimpleObject)]
pub struct DustGenerationMerkleUpdate {
    /// Index in the tree.
    pub index: u64,
    /// Collapsed update data.
    pub collapsed_update: HexEncoded,
    /// Block height when update occurred.
    pub block_height: u32,
}

impl From<crate::domain::DustGenerationMerkleUpdate> for DustGenerationMerkleUpdate {
    fn from(value: crate::domain::DustGenerationMerkleUpdate) -> Self {
        Self {
            index: value.index,
            collapsed_update: value.collapsed_update.hex_encode(),
            block_height: value.block_height,
        }
    }
}

/// DUST generation progress information.
#[derive(Debug, SimpleObject)]
pub struct DustGenerationProgress {
    /// Highest generation index.
    pub highest_index: u64,
    /// Number of active generations.
    pub active_generations: u32,
}

impl From<crate::domain::DustGenerationProgress> for DustGenerationProgress {
    fn from(value: crate::domain::DustGenerationProgress) -> Self {
        Self {
            highest_index: value.highest_index,
            active_generations: value.active_generations,
        }
    }
}

/// DUST nullifier transaction.
#[derive(Debug, SimpleObject)]
pub struct DustNullifierTransaction {
    /// The transaction ID.
    pub transaction_id: u64,
    /// The transaction hash.
    pub transaction_hash: HexEncoded,
    /// Matching nullifier prefixes.
    pub matching_nullifier_prefixes: Vec<String>,
}

impl From<crate::domain::DustNullifierTransaction> for DustNullifierTransaction {
    fn from(value: crate::domain::DustNullifierTransaction) -> Self {
        Self {
            transaction_id: value.transaction_id,
            transaction_hash: HexEncoded(value.transaction_hash),
            matching_nullifier_prefixes: value.matching_nullifier_prefixes,
        }
    }
}

/// DUST nullifier transaction progress.
#[derive(Debug, SimpleObject)]
pub struct DustNullifierTransactionProgress {
    /// Highest processed block.
    pub highest_block: u32,
    /// Number of matched transactions.
    pub matched_count: u32,
}

impl From<crate::domain::DustNullifierTransactionProgress> for DustNullifierTransactionProgress {
    fn from(value: crate::domain::DustNullifierTransactionProgress) -> Self {
        Self {
            highest_block: value.highest_block,
            matched_count: value.matched_count,
        }
    }
}

/// DUST commitment information.
#[derive(Debug, SimpleObject)]
pub struct DustCommitment {
    /// DUST commitment.
    pub commitment: HexEncoded,
    /// DUST nullifier if spent.
    pub nullifier: Option<HexEncoded>,
    /// Initial value.
    pub value: String,
    /// DUST address.
    pub owner: String,
    /// Nonce.
    pub nonce: HexEncoded,
    /// Creation timestamp.
    pub created_at: i64,
    /// Spending timestamp.
    pub spent_at: Option<i64>,
}

impl From<crate::domain::DustCommitment> for DustCommitment {
    fn from(value: crate::domain::DustCommitment) -> Self {
        Self {
            commitment: value.commitment.hex_encode(),
            nullifier: value.nullifier.map(|n| n.hex_encode()),
            value: value.value.to_string(),
            owner: value.owner.hex_encode().0,
            nonce: value.nonce.hex_encode(),
            created_at: value.created_at,
            spent_at: value.spent_at,
        }
    }
}

/// DUST commitment Merkle tree update.
#[derive(Debug, SimpleObject)]
pub struct DustCommitmentMerkleUpdate {
    /// Index in the tree.
    pub index: u64,
    /// Collapsed update data.
    pub collapsed_update: HexEncoded,
    /// Block height when update occurred.
    pub block_height: u32,
}

impl From<crate::domain::DustCommitmentMerkleUpdate> for DustCommitmentMerkleUpdate {
    fn from(value: crate::domain::DustCommitmentMerkleUpdate) -> Self {
        Self {
            index: value.index,
            collapsed_update: value.collapsed_update.hex_encode(),
            block_height: value.block_height,
        }
    }
}

/// DUST commitment progress information.
#[derive(Debug, SimpleObject)]
pub struct DustCommitmentProgress {
    /// Highest commitment index.
    pub highest_index: u64,
    /// Total number of commitments.
    pub total_commitments: u32,
}

impl From<crate::domain::DustCommitmentProgress> for DustCommitmentProgress {
    fn from(value: crate::domain::DustCommitmentProgress) -> Self {
        Self {
            highest_index: value.highest_index,
            total_commitments: value.total_commitments,
        }
    }
}

/// Registration update event.
#[derive(Debug, SimpleObject)]
pub struct RegistrationUpdate {
    /// Address type.
    pub address_type: AddressTypeGraphQL,
    /// Address value.
    pub address_value: String,
    /// Related addresses.
    pub related_addresses: RelatedAddresses,
    /// Whether registration is active.
    pub is_active: bool,
    /// Timestamp.
    pub timestamp: i64,
}

impl From<crate::domain::RegistrationUpdate> for RegistrationUpdate {
    fn from(value: crate::domain::RegistrationUpdate) -> Self {
        Self {
            address_type: match value.address_type {
                AddressType::Night => AddressTypeGraphQL::Night,
                AddressType::Dust => AddressTypeGraphQL::Dust,
                AddressType::CardanoStake => AddressTypeGraphQL::CardanoStake,
            },
            address_value: value.address_value,
            related_addresses: value.related_addresses.into(),
            is_active: value.is_active,
            timestamp: value.timestamp,
        }
    }
}

/// Related addresses in registration.
#[derive(Debug, SimpleObject)]
pub struct RelatedAddresses {
    /// Night address.
    pub night_address: Option<String>,
    /// DUST address.
    pub dust_address: Option<String>,
    /// Cardano stake key.
    pub cardano_stake_key: Option<String>,
}

impl From<crate::domain::RelatedAddresses> for RelatedAddresses {
    fn from(value: crate::domain::RelatedAddresses) -> Self {
        Self {
            night_address: value.night_address,
            dust_address: value.dust_address,
            cardano_stake_key: value.cardano_stake_key,
        }
    }
}

/// Registration update progress.
#[derive(Debug, SimpleObject)]
pub struct RegistrationUpdateProgress {
    /// Number of processed registrations.
    pub processed_count: u32,
    /// Current timestamp.
    pub timestamp: i64,
}

impl From<crate::domain::RegistrationUpdateProgress> for RegistrationUpdateProgress {
    fn from(value: crate::domain::RegistrationUpdateProgress) -> Self {
        Self {
            processed_count: value.processed_count,
            timestamp: value.timestamp,
        }
    }
}

/// Event unions for streaming subscriptions.
#[derive(Debug, Union)]
pub enum DustGenerationEventGraphQL {
    /// DUST generation information.
    DustGenerationInfo(Box<DustGenerationInfo>),
    /// Merkle tree update.
    DustGenerationMerkleUpdate(Box<DustGenerationMerkleUpdate>),
    /// Progress information.
    DustGenerationProgress(Box<DustGenerationProgress>),
}

impl From<DustGenerationEvent> for DustGenerationEventGraphQL {
    fn from(value: DustGenerationEvent) -> Self {
        match value {
            DustGenerationEvent::DustGenerationInfo(info) => {
                DustGenerationEventGraphQL::DustGenerationInfo(Box::new(info.into()))
            }
            DustGenerationEvent::DustGenerationMerkleUpdate(update) => {
                DustGenerationEventGraphQL::DustGenerationMerkleUpdate(Box::new(update.into()))
            }
            DustGenerationEvent::DustGenerationProgress(progress) => {
                DustGenerationEventGraphQL::DustGenerationProgress(Box::new(progress.into()))
            }
        }
    }
}

/// DUST nullifier transaction events.
#[derive(Debug, Union)]
pub enum DustNullifierTransactionEventGraphQL {
    /// Transaction with nullifiers.
    DustNullifierTransaction(Box<DustNullifierTransaction>),
    /// Progress information.
    DustNullifierTransactionProgress(Box<DustNullifierTransactionProgress>),
}

impl From<DustNullifierTransactionEvent> for DustNullifierTransactionEventGraphQL {
    fn from(value: DustNullifierTransactionEvent) -> Self {
        match value {
            DustNullifierTransactionEvent::DustNullifierTransaction(tx) => {
                DustNullifierTransactionEventGraphQL::DustNullifierTransaction(Box::new(tx.into()))
            }
            DustNullifierTransactionEvent::DustNullifierTransactionProgress(progress) => {
                DustNullifierTransactionEventGraphQL::DustNullifierTransactionProgress(Box::new(
                    progress.into(),
                ))
            }
        }
    }
}

/// DUST commitment events.
#[derive(Debug, Union)]
pub enum DustCommitmentEventGraphQL {
    /// DUST commitment.
    DustCommitment(Box<DustCommitment>),
    /// Merkle tree update.
    DustCommitmentMerkleUpdate(Box<DustCommitmentMerkleUpdate>),
    /// Progress information.
    DustCommitmentProgress(Box<DustCommitmentProgress>),
}

impl From<DustCommitmentEvent> for DustCommitmentEventGraphQL {
    fn from(value: DustCommitmentEvent) -> Self {
        match value {
            DustCommitmentEvent::DustCommitment(commitment) => {
                DustCommitmentEventGraphQL::DustCommitment(Box::new(commitment.into()))
            }
            DustCommitmentEvent::DustCommitmentMerkleUpdate(update) => {
                DustCommitmentEventGraphQL::DustCommitmentMerkleUpdate(Box::new(update.into()))
            }
            DustCommitmentEvent::DustCommitmentProgress(progress) => {
                DustCommitmentEventGraphQL::DustCommitmentProgress(Box::new(progress.into()))
            }
        }
    }
}

/// Registration update events.
#[derive(Debug, Union)]
pub enum RegistrationUpdateEventGraphQL {
    /// Registration update.
    RegistrationUpdate(Box<RegistrationUpdate>),
    /// Progress information.
    RegistrationUpdateProgress(Box<RegistrationUpdateProgress>),
}

impl From<RegistrationUpdateEvent> for RegistrationUpdateEventGraphQL {
    fn from(value: RegistrationUpdateEvent) -> Self {
        match value {
            RegistrationUpdateEvent::RegistrationUpdate(update) => {
                RegistrationUpdateEventGraphQL::RegistrationUpdate(Box::new(update.into()))
            }
            RegistrationUpdateEvent::RegistrationUpdateProgress(progress) => {
                RegistrationUpdateEventGraphQL::RegistrationUpdateProgress(Box::new(
                    progress.into(),
                ))
            }
        }
    }
}
