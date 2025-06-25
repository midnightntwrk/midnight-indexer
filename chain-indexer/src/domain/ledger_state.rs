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
use derive_more::derive::{Deref, From};
use fastrace::trace;
use indexer_common::domain::{ByteArray, NetworkId, RawTransaction, ledger::ContractState};
use std::ops::DerefMut;
use thiserror::Error;

/// New type for ledger state from indexer_common.
#[derive(Debug, Clone, Default, From, Deref)]
pub struct LedgerState(pub indexer_common::domain::ledger::LedgerState);

impl DerefMut for LedgerState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl LedgerState {
    /// Apply the given raw transactions to this ledger state.
    #[trace(properties = {
        "block_parent_hash": "{block_parent_hash}",
        "network_id": "{network_id}"
    })]
    pub fn apply_raw_transactions<'a>(
        &mut self,
        transactions: impl Iterator<Item = &'a RawTransaction>,
        block_parent_hash: ByteArray<32>,
        block_timestamp: u64,
        network_id: NetworkId,
    ) -> Result<(), Error> {
        for transaction in transactions {
            self.apply_transaction(transaction, block_parent_hash, block_timestamp, network_id)?;
        }

        self.post_apply_transactions(block_timestamp);

        Ok(())
    }

    /// Apply the given transactions to this ledger state and also update relevant transaction data
    /// like start_index and end_index.
    #[trace(properties = {
        "block_parent_hash": "{block_parent_hash}",
        "network_id": "{network_id}"
    })]
    pub fn apply_and_update_transactions<'a>(
        &mut self,
        transactions: impl Iterator<Item = &'a mut Transaction>,
        block_parent_hash: ByteArray<32>,
        block_timestamp: u64,
        network_id: NetworkId,
    ) -> Result<(), Error> {
        for transaction in transactions {
            self.apply_transaction_mut(
                transaction,
                block_parent_hash,
                block_timestamp,
                network_id,
            )?;
        }

        self.post_apply_transactions(block_timestamp);

        Ok(())
    }

    /// The highest used zswap state index or none.
    pub fn highest_zswap_state_index(&self) -> Option<u64> {
        (self.zswap_first_free() != 0).then(|| self.zswap_first_free() - 1)
    }

    #[trace(properties = {
        "block_parent_hash": "{block_parent_hash}",
        "network_id": "{network_id}"
    })]
    fn apply_transaction_mut(
        &mut self,
        transaction: &mut Transaction,
        block_parent_hash: ByteArray<32>,
        block_timestamp: u64,
        network_id: NetworkId,
    ) -> Result<(), Error> {
        let start_index = self.zswap_first_free();
        let mut end_index = self.zswap_first_free();

        let transaction_result = self.apply_transaction(
            &transaction.raw,
            block_parent_hash,
            block_timestamp,
            network_id,
        )?;

        // Handle genesis block: extract any pre-funded unshielded UTXOs.
        // Check if this is genesis block by examining parent hash.
        if block_parent_hash == ByteArray([0; 32]) {
            let utxos = self.extract_utxos();
            transaction.created_unshielded_utxos.extend(utxos);
        }

        // Update end_index and contract zswap state if necessary.
        let first_free = self.zswap_first_free();
        if first_free > start_index {
            self.update_contract_zswap_state(transaction, network_id)?;
            end_index = first_free - 1;
        }

        // Update transaction.
        transaction.transaction_result = transaction_result;
        transaction.merkle_tree_root = self.zswap_merkle_tree_root().serialize(network_id)?;
        transaction.start_index = start_index;
        transaction.end_index = end_index;

        // Update extracted balances of contract actions.
        for contract_action in &mut transaction.contract_actions {
            let contract_state = ContractState::deserialize(
                &contract_action.state,
                network_id,
                transaction.protocol_version,
            )?;
            let balances = contract_state.balances(network_id)?;
            contract_action.extracted_balances = balances;
        }

        Ok(())
    }

    fn update_contract_zswap_state(
        &self,
        transaction: &mut Transaction,
        network_id: NetworkId,
    ) -> Result<(), Error> {
        for contract_action in transaction.contract_actions.iter_mut() {
            let zswap_state =
                self.extract_contract_zswap_state(&contract_action.address, network_id)?;
            contract_action.zswap_state = zswap_state;
        }

        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("cannot apply transaction")]
    ApplyTransaction(#[from] indexer_common::domain::ledger::Error),
}
