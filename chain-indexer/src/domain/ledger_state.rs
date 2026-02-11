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

use crate::domain::{RegularTransaction, SystemTransaction, Transaction, node};
use derive_more::derive::{Deref, From};
use fastrace::trace;
use indexer_common::domain::{
    ApplyRegularTransactionOutcome, ApplySystemTransactionOutcome, BlockHash, GenesisSettings,
    NetworkId, ProtocolVersion, SerializedContractAddress, SerializedLedgerStateKey,
    TransactionHash,
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
    pub fn new(
        network_id: NetworkId,
        protocol_version: ProtocolVersion,
        genesis_settings: Option<GenesisSettings>,
    ) -> Result<Self, Error> {
        indexer_common::domain::ledger::LedgerState::new(
            network_id,
            protocol_version,
            genesis_settings,
        )
        .map_err(Error::Create)
        .map(Into::into)
    }

    pub fn load(
        key: &SerializedLedgerStateKey,
        protocol_version: ProtocolVersion,
    ) -> Result<Self, Error> {
        indexer_common::domain::ledger::LedgerState::load(key, protocol_version)
            .map_err(Error::Load)
            .map(Into::into)
    }

    /// Apply the given node transactions to this ledger state and return domain transactions.
    #[trace(properties = { "parent_block_hash": "{parent_block_hash}" })]
    pub fn apply_transactions(
        &mut self,
        transactions: impl IntoIterator<Item = node::Transaction>,
        parent_block_hash: BlockHash,
        block_timestamp: u64,
        parent_block_timestamp: u64,
    ) -> Result<(Vec<Transaction>, LedgerParameters), Error> {
        let transactions = transactions
            .into_iter()
            .map(|transaction| match transaction {
                node::Transaction::Regular(transaction) => self.apply_regular_transaction(
                    transaction,
                    parent_block_hash,
                    block_timestamp,
                    parent_block_timestamp,
                ),

                node::Transaction::System(transaction) => {
                    self.apply_system_transaction(transaction, block_timestamp)
                }
            })
            .collect::<Result<Vec<_>, _>>()?;

        let ledger_parameters = self
            .finalize_apply_transactions(block_timestamp)
            .map_err(Error::PostApplyTransactions)?;

        Ok((transactions, ledger_parameters))
    }

    /// The highest used zswap state index or none.
    pub fn highest_zswap_state_index(&self) -> Option<u64> {
        (self.zswap_first_free() != 0).then(|| self.zswap_first_free() - 1)
    }

    #[trace(properties = {
        "parent_block_hash": "{parent_block_hash}",
        "block_timestamp": "{block_timestamp}"
    })]
    fn apply_regular_transaction(
        &mut self,
        transaction: node::RegularTransaction,
        parent_block_hash: BlockHash,
        block_timestamp: u64,
        parent_block_timestamp: u64,
    ) -> Result<Transaction, Error> {
        let mut transaction = RegularTransaction::from(transaction);

        // Apply transaction.
        let start_index = self.zswap_first_free();
        let ApplyRegularTransactionOutcome {
            transaction_result,
            created_unshielded_utxos,
            spent_unshielded_utxos,
            ledger_events,
        } = self
            .0
            .apply_regular_transaction(
                &transaction.raw,
                parent_block_hash,
                block_timestamp,
                parent_block_timestamp,
            )
            .map_err(|error| Error::ApplyRegularTransaction(Some(transaction.hash), error))?;

        // Update transaction.
        transaction.transaction_result = transaction_result;
        transaction.merkle_tree_root = self
            .zswap_merkle_tree_root()
            .serialize()
            .map_err(|error| Error::SerializeMerkleTreeRoot(transaction.hash, error))?;
        transaction.start_index = start_index;
        transaction.end_index = self.zswap_first_free();
        transaction.created_unshielded_utxos = created_unshielded_utxos;
        transaction.spent_unshielded_utxos = spent_unshielded_utxos;
        transaction.ledger_events = ledger_events;

        // Update contract actions.
        for contract_action in transaction.contract_actions.iter_mut() {
            let zswap_state = self
                .extract_contract_zswap_state(&contract_action.address)
                .map_err(|error| Error::ExtractContractZswapState(transaction.hash, error))?;
            contract_action.zswap_state = zswap_state;

            // TODO: Workaround until we filter failed contract actions (empty state means failed).
            if !contract_action.state.is_empty() {
                let contract_state = ledger::ContractState::deserialize(
                    &contract_action.state,
                    transaction.protocol_version,
                )
                .map_err(|error| {
                    Error::DeserializeContractState(
                        transaction.hash,
                        contract_action.address.clone(),
                        error,
                    )
                })?;
                let balances = contract_state.balances().map_err(|error| {
                    Error::GetContractBalances(
                        transaction.hash,
                        contract_action.address.clone(),
                        error,
                    )
                })?;
                contract_action.extracted_balances = balances;
            }
        }

        Ok(Transaction::Regular(transaction.into()))
    }

    #[trace(properties = {
        "block_timestamp": "{block_timestamp}"
    })]
    fn apply_system_transaction(
        &mut self,
        transaction: node::SystemTransaction,
        block_timestamp: u64,
    ) -> Result<Transaction, Error> {
        let mut transaction = SystemTransaction::from(transaction);

        // Apply transaction.
        let ApplySystemTransactionOutcome {
            created_unshielded_utxos,
            ledger_events,
        } = self
            .0
            .apply_system_transaction(&transaction.raw, block_timestamp)
            .map_err(|error| Error::ApplySystemTransaction(Some(transaction.hash), error))?;

        // Update transaction.
        transaction.created_unshielded_utxos = created_unshielded_utxos;
        transaction.ledger_events = ledger_events;

        Ok(Transaction::System(transaction))
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Create(indexer_common::domain::ledger::Error),

    #[error(transparent)]
    Load(indexer_common::domain::ledger::Error),

    #[error("cannot apply regular transaction {hash}", hash = stringify_hash(.0))]
    ApplyRegularTransaction(
        Option<TransactionHash>,
        #[source] indexer_common::domain::ledger::Error,
    ),

    #[error("cannot apply system transaction {hash}", hash = stringify_hash(.0))]
    ApplySystemTransaction(
        Option<TransactionHash>,
        #[source] indexer_common::domain::ledger::Error,
    ),

    #[error("cannot finalize transaction application")]
    PostApplyTransactions(#[source] indexer_common::domain::ledger::Error),

    #[error("cannot serialize merkle tree root for transaction {0}")]
    SerializeMerkleTreeRoot(
        TransactionHash,
        #[source] indexer_common::domain::ledger::Error,
    ),

    #[error("cannot extract contract zswap state for transaction {0}")]
    ExtractContractZswapState(
        TransactionHash,
        #[source] indexer_common::domain::ledger::Error,
    ),

    #[error("cannot deserialize contract state for transaction {0} and contract address {1}")]
    DeserializeContractState(
        TransactionHash,
        SerializedContractAddress,
        #[source] indexer_common::domain::ledger::Error,
    ),

    #[error("cannot get contract balances for transaction {0} and contract address {1}")]
    GetContractBalances(
        TransactionHash,
        SerializedContractAddress,
        #[source] indexer_common::domain::ledger::Error,
    ),
}

fn stringify_hash(hash: &Option<TransactionHash>) -> String {
    hash.map(|hash| hash.to_string())
        .unwrap_or_else(|| "<hash unavailable>".to_string())
}
