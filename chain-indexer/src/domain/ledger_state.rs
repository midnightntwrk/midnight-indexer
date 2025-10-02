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
    RegularTransaction, SystemTransaction, Transaction, dust::extract_dust_operations, node,
};
use derive_more::derive::{Deref, From};
use fastrace::trace;
use indexer_common::domain::{
    ApplyRegularTransactionResult, BlockHash, LedgerEvent, LedgerEventGrouping, NetworkId,
    SerializedTransaction, TransactionVariant,
    dust::DustEvent,
    ledger::{self, LedgerParameters},
};
use std::ops::DerefMut;
use thiserror::Error;

/// New type for ledger state from indexer_common.
#[derive(Debug, Clone, From, Deref)]
pub struct LedgerState(pub indexer_common::domain::ledger::LedgerState);

impl DerefMut for LedgerState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl LedgerState {
    #[allow(missing_docs)]
    pub fn new(network_id: NetworkId) -> Self {
        Self(indexer_common::domain::ledger::LedgerState::new(network_id))
    }

    /// Apply the given storecd transactions to this ledger state.
    #[trace(properties = { "block_parent_hash": "{block_parent_hash}" })]
    pub fn apply_stored_transactions<'a>(
        &mut self,
        transactions: impl Iterator<Item = &'a (TransactionVariant, SerializedTransaction)>,
        block_parent_hash: BlockHash,
        block_timestamp: u64,
    ) -> Result<LedgerParameters, Error> {
        for (variant, transaction) in transactions {
            match variant {
                TransactionVariant::Regular => {
                    self.apply_regular_transaction(
                        transaction,
                        block_parent_hash,
                        block_timestamp,
                    )?;
                }

                TransactionVariant::System => {
                    self.apply_system_transaction(transaction, block_timestamp)?;
                }
            }
        }

        let ledger_parameters = self.post_apply_transactions(block_timestamp)?;

        Ok(ledger_parameters)
    }

    /// Apply the given node transactions to this ledger state and return domain transactions.
    #[trace(properties = { "block_parent_hash": "{block_parent_hash}" })]
    pub fn apply_node_transactions(
        &mut self,
        transactions: impl IntoIterator<Item = node::Transaction>,
        block_parent_hash: BlockHash,
        block_timestamp: u64,
    ) -> Result<(Vec<Transaction>, LedgerParameters), Error> {
        let transactions = transactions
            .into_iter()
            .map(|transaction| {
                self.apply_node_transaction(transaction, block_parent_hash, block_timestamp)
            })
            .collect::<Result<Vec<_>, _>>()?;

        let ledger_parameters = self.post_apply_transactions(block_timestamp)?;

        Ok((transactions, ledger_parameters))
    }

    /// The highest used zswap state index or none.
    pub fn highest_zswap_state_index(&self) -> Option<u64> {
        (self.zswap_first_free() != 0).then(|| self.zswap_first_free() - 1)
    }

    #[trace(properties = {
        "block_parent_hash": "{block_parent_hash}",
        "block_timestamp": "{block_timestamp}"
    })]
    fn apply_node_transaction(
        &mut self,
        transaction: node::Transaction,
        block_parent_hash: BlockHash,
        block_timestamp: u64,
    ) -> Result<Transaction, Error> {
        match transaction {
            node::Transaction::Regular(transaction) => {
                self.apply_regular_node_transaction(transaction, block_parent_hash, block_timestamp)
            }

            node::Transaction::System(transaction) => {
                self.apply_system_node_transaction(transaction, block_parent_hash, block_timestamp)
            }
        }
    }

    #[trace(properties = {
        "block_parent_hash": "{block_parent_hash}",
        "block_timestamp": "{block_timestamp}"
    })]
    fn apply_regular_node_transaction(
        &mut self,
        transaction: node::RegularTransaction,
        block_parent_hash: BlockHash,
        block_timestamp: u64,
    ) -> Result<Transaction, Error> {
        let mut transaction = RegularTransaction::from(transaction);

        // Apply transaction.
        let start_index = self.zswap_first_free();
        let ApplyRegularTransactionResult {
            transaction_result,
            created_unshielded_utxos,
            spent_unshielded_utxos,
            ledger_events,
        } = self.apply_regular_transaction(&transaction.raw, block_parent_hash, block_timestamp)?;

        // Update transaction.
        transaction.transaction_result = transaction_result;
        transaction.merkle_tree_root = self.zswap_merkle_tree_root().serialize()?;
        transaction.start_index = start_index;
        transaction.end_index = self.zswap_first_free();
        transaction.created_unshielded_utxos = created_unshielded_utxos;
        transaction.spent_unshielded_utxos = spent_unshielded_utxos;
        transaction.ledger_events = ledger_events.clone();

        // Extract and process DUST events into projections
        let dust_events = extract_dust_events_from_ledger_events(&ledger_events)?;
        if !dust_events.is_empty() {
            transaction.dust_projections = Some(extract_dust_operations(&dust_events));
        }

        // Update contract actions.
        if transaction.end_index > transaction.start_index {
            for contract_action in transaction.contract_actions.iter_mut() {
                let zswap_state = self.extract_contract_zswap_state(&contract_action.address)?;
                contract_action.chain_state = zswap_state;
            }
        }

        // Update extracted balances of contract actions.
        for contract_action in &mut transaction.contract_actions {
            let contract_state = ledger::ContractState::deserialize(
                &contract_action.state,
                transaction.protocol_version,
            )?;
            let balances = contract_state.balances()?;
            contract_action.extracted_balances = balances;
        }

        Ok(Transaction::Regular(transaction.into()))
    }

    #[trace(properties = {
        "block_parent_hash": "{block_parent_hash}",
        "block_timestamp": "{block_timestamp}"
    })]
    fn apply_system_node_transaction(
        &mut self,
        transaction: node::SystemTransaction,
        block_parent_hash: BlockHash,
        block_timestamp: u64,
    ) -> Result<Transaction, Error> {
        let mut transaction = SystemTransaction::from(transaction);
        let ledger_events = self.apply_system_transaction(&transaction.raw, block_timestamp)?;
        transaction.ledger_events = ledger_events.clone();

        // Extract and process DUST events into projections
        let dust_events = extract_dust_events_from_ledger_events(&ledger_events)?;
        if !dust_events.is_empty() {
            transaction.dust_projections = Some(extract_dust_operations(&dust_events));
        }

        Ok(Transaction::System(transaction))
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("cannot apply transaction")]
    ApplyTransaction(#[from] indexer_common::domain::ledger::Error),

    #[error("cannot deserialize DUST event: {0}")]
    DeserializeDustEvent(String),
}

/// Extract DUST events from generic ledger events.
///
/// For now, this is a simplified implementation that reconstructs DustEvent from
/// LedgerEventAttributes. In a full implementation, we would deserialize the raw bytes directly.
fn extract_dust_events_from_ledger_events(
    ledger_events: &[LedgerEvent],
) -> Result<Vec<DustEvent>, Error> {
    use indexer_common::domain::{
        LedgerEventAttributes, TransactionHash,
        dust::{DustEventAttributes, DustGenerationInfo, DustParameters, QualifiedDustOutput},
    };

    let mut dust_events = Vec::new();

    for ledger_event in ledger_events {
        // Check if this is a DUST event by checking the grouping
        if ledger_event.grouping == LedgerEventGrouping::Dust {
            // Convert LedgerEventAttributes to DustEventAttributes
            // For now, we create simplified events based on the attributes
            // In production, we would deserialize from raw bytes
            let event_details = match &ledger_event.attributes {
                LedgerEventAttributes::DustInitialUtxo { output: _ } => {
                    // Create a simplified DustEventAttributes for initial UTXO
                    // This is a placeholder - actual implementation would extract from raw
                    DustEventAttributes::DustInitialUtxo {
                        output: QualifiedDustOutput {
                            initial_value: 0,
                            owner: Default::default(),
                            nonce: Default::default(),
                            seq: 0,
                            ctime: 0,
                            backing_night: Default::default(),
                            mt_index: 0,
                        },
                        generation_info: DustGenerationInfo {
                            night_utxo_hash: Default::default(),
                            value: 0,
                            owner: Default::default(),
                            nonce: Default::default(),
                            ctime: 0,
                            dtime: 0, // 0 means not spent yet
                        },
                        generation_index: 0,
                    }
                }
                LedgerEventAttributes::DustGenerationDtimeUpdate => {
                    // Create a simplified DustEventAttributes for dtime update
                    DustEventAttributes::DustGenerationDtimeUpdate {
                        generation_info: DustGenerationInfo {
                            night_utxo_hash: Default::default(),
                            value: 0,
                            owner: Default::default(),
                            nonce: Default::default(),
                            ctime: 0,
                            dtime: 1, // Non-zero means spent (would be actual timestamp)
                        },
                        generation_index: 0,
                        merkle_path: vec![],
                    }
                }
                LedgerEventAttributes::DustSpendProcessed => {
                    // Create a simplified DustEventAttributes for spend
                    DustEventAttributes::DustSpendProcessed {
                        commitment: Default::default(),
                        commitment_index: 0,
                        nullifier: Default::default(),
                        v_fee: 0,
                        time: 0,
                        params: DustParameters {
                            night_dust_ratio: 5_000_000_000, // 5 DUST per NIGHT
                            generation_decay_rate: 8_267,    // ~1 week generation time
                            dust_grace_period: 3 * 60 * 60,  // 3 hours in seconds
                        },
                    }
                }
                _ => continue, // Not a DUST-specific event
            };

            // Create DustEvent with the extracted details
            dust_events.push(DustEvent {
                transaction_hash: TransactionHash::default(), // Would come from transaction context
                logical_segment: 0,                           // Would come from segment info
                physical_segment: 0,                          // Would come from segment info
                event_details,
            });
        }
    }

    Ok(dust_events)
}
