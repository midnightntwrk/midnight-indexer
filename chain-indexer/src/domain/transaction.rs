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

use crate::domain::{
    ContractAction,
    dust::{DustEvent, ProcessedDustEvents},
    node,
};
use indexer_common::domain::{
    ByteArray, ProtocolVersion,
    ledger::{
        NightDistributionData, ParameterUpdateData, SerializedTransaction,
        SerializedTransactionIdentifier, SerializedZswapStateRoot,
        SystemTransaction as DomainSystemTransaction, TransactionHash, TransactionResult,
        TreasuryPaymentShieldedData, TreasuryPaymentUnshieldedData, UnshieldedUtxo,
    },
};
use sqlx::{FromRow, Type};
use std::fmt::Debug;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transaction {
    Regular(Box<RegularTransaction>),
    System(Box<SystemTransaction>),
}

impl Transaction {
    pub fn variant(&self) -> TransactionVariant {
        match self {
            Transaction::Regular(_) => TransactionVariant::Regular,
            Transaction::System(_) => TransactionVariant::System,
        }
    }

    pub fn hash(&self) -> TransactionHash {
        match self {
            Transaction::Regular(transaction) => transaction.hash,
            Transaction::System(transaction) => transaction.hash,
        }
    }

    pub fn protocol_version(&self) -> ProtocolVersion {
        match self {
            Transaction::Regular(transaction) => transaction.protocol_version,
            Transaction::System(transaction) => transaction.protocol_version,
        }
    }

    pub fn raw(&self) -> &[u8] {
        match self {
            Transaction::Regular(transaction) => &transaction.raw,
            Transaction::System(transaction) => &transaction.raw,
        }
    }
}

impl TryFrom<node::Transaction> for Transaction {
    type Error = SystemTransactionError;

    fn try_from(transaction: node::Transaction) -> Result<Self, Self::Error> {
        match transaction {
            node::Transaction::Regular(regular_transaction) => {
                Ok(Transaction::Regular(Box::new(regular_transaction.into())))
            }

            node::Transaction::System(system_transaction) => Ok(Transaction::System(Box::new(
                system_transaction.try_into()?,
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegularTransaction {
    // These fields come from node::Transaction.
    pub hash: TransactionHash,
    pub protocol_version: ProtocolVersion,
    pub raw: SerializedTransaction,
    pub identifiers: Vec<SerializedTransactionIdentifier>,
    pub contract_actions: Vec<ContractAction>,
    pub paid_fees: u128,
    pub estimated_fees: u128,

    // These fields come from applying the node transactions to the ledger state.
    pub transaction_result: TransactionResult,
    pub merkle_tree_root: SerializedZswapStateRoot,
    pub start_index: u64,
    pub end_index: u64,
    pub created_unshielded_utxos: Vec<UnshieldedUtxo>,
    pub spent_unshielded_utxos: Vec<UnshieldedUtxo>,
    pub dust_events: Vec<DustEvent>,
    pub processed_dust_events: ProcessedDustEvents,
}

impl From<node::RegularTransaction> for RegularTransaction {
    fn from(transaction: node::RegularTransaction) -> Self {
        Self {
            hash: transaction.hash,
            protocol_version: transaction.protocol_version,
            identifiers: transaction.identifiers,
            raw: transaction.raw,
            contract_actions: transaction.contract_actions,
            paid_fees: transaction.paid_fees,
            estimated_fees: transaction.estimated_fees,
            transaction_result: Default::default(),
            merkle_tree_root: Default::default(),
            start_index: Default::default(),
            end_index: Default::default(),
            created_unshielded_utxos: Default::default(),
            spent_unshielded_utxos: Default::default(),
            dust_events: Vec::default(),
            processed_dust_events: ProcessedDustEvents {
                generations: Vec::new(),
                utxos: Vec::new(),
                merkle_tree_updates: Vec::new(),
                spends: Vec::new(),
                dtime_update: None,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemTransaction {
    pub hash: TransactionHash,
    pub protocol_version: ProtocolVersion,
    pub raw: SerializedTransaction,
    // DUST events from system transactions (e.g., CNightGeneratesDustUpdate).
    pub dust_events: Vec<DustEvent>,
    pub processed_dust_events: ProcessedDustEvents,
    // Additional processed data from system transactions.
    pub reserve_distribution: Option<u128>,
    pub parameter_update: Option<ParameterUpdateData>,
    pub night_distribution: Option<NightDistributionData>,
    pub treasury_income: Option<(u128, String)>,
    pub treasury_payment_shielded: Option<TreasuryPaymentShieldedData>,
    pub treasury_payment_unshielded: Option<TreasuryPaymentUnshieldedData>,
}

/// Error type for system transaction processing.
#[derive(Debug, Error)]
pub enum SystemTransactionError {
    #[error("cannot deserialize system transaction: {0}")]
    DeserializationError(String),

    #[error("cannot process system transaction: {0}")]
    ProcessingError(String),
}

impl TryFrom<node::SystemTransaction> for SystemTransaction {
    type Error = SystemTransactionError;

    fn try_from(transaction: node::SystemTransaction) -> Result<Self, Self::Error> {
        // Deserialize the raw transaction to process it.
        let domain_tx =
            DomainSystemTransaction::deserialize(&transaction.raw, transaction.protocol_version)
                .map_err(|error| SystemTransactionError::DeserializationError(error.to_string()))?;

        // Extract metadata from the transaction.
        let metadata = domain_tx.extract_metadata(&transaction.hash);

        Ok(Self {
            hash: transaction.hash,
            protocol_version: transaction.protocol_version,
            raw: transaction.raw,
            // Populated during ledger state application in
            // ledger_state.rs::apply_system_node_transaction()
            dust_events: Vec::new(),
            processed_dust_events: ProcessedDustEvents {
                // Populated during ledger state application in
                // ledger_state.rs::apply_system_node_transaction() via extract_dust_operations()
                generations: Vec::new(),
                utxos: Vec::new(),
                merkle_tree_updates: Vec::new(),
                spends: Vec::new(),
                dtime_update: None,
            },
            reserve_distribution: metadata.reserve_distribution,
            parameter_update: metadata.parameter_update,
            night_distribution: metadata.night_distribution,
            treasury_income: metadata.treasury_income,
            treasury_payment_shielded: metadata.treasury_payment_shielded,
            treasury_payment_unshielded: metadata.treasury_payment_unshielded,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Type)]
#[cfg_attr(feature = "cloud", sqlx(type_name = "TRANSACTION_VARIANT"))]
pub enum TransactionVariant {
    Regular,
    System,
}

/// All serialized transactions from a single block along with metadata needed for ledger state
/// application.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct BlockTransactions {
    pub transactions: Vec<(TransactionVariant, SerializedTransaction)>,

    #[sqlx(try_from = "i64")]
    pub protocol_version: ProtocolVersion,

    pub block_parent_hash: ByteArray<32>,

    #[sqlx(try_from = "i64")]
    pub block_timestamp: u64,
}
