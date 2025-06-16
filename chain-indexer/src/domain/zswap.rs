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

use crate::domain::{Transaction, UnshieldedUtxo};
use derive_more::derive::{Deref, From};
use fastrace::trace;
use indexer_common::{
    LedgerTransaction,
    domain::{
        ByteArray, ContractAddress, MerkleTreeRoot, NetworkId, RawLedgerState, RawTransaction,
        TransactionResult,
    },
    serialize::SerializableExt,
};
use midnight_base_crypto::{hash::HashOutput, time::Timestamp};
use midnight_ledger::semantics::{
    TransactionContext, TransactionResult as LedgerTransactionResult,
};
use midnight_onchain_runtime::context::BlockContext;
use midnight_serialize::deserialize;
use midnight_storage::DefaultDB;
use midnight_zswap::ledger::State as LedgerZswapState;
use std::io;
use thiserror::Error;

/// New type for ledger state from indexer_common.
#[derive(Debug, Clone, Default, From, Deref)]
pub struct LedgerState(pub indexer_common::domain::LedgerState);

impl LedgerState {
    /// Serialize this ledger state using the given network ID.
    #[trace]
    pub fn serialize(&self, network_id: NetworkId) -> Result<RawLedgerState, io::Error> {
        let bytes = self.0.serialize(network_id)?;
        Ok(bytes.into())
    }

    /// Apply the given raw transactions to this ledger state.
    #[trace]
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
    #[trace]
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

    /// The last used index.
    pub fn end_index(&self) -> Option<u64> {
        (self.zswap.first_free != 0).then(|| self.zswap.first_free - 1)
    }

    #[trace]
    pub fn apply_transaction(
        &mut self,
        transaction: &RawTransaction,
        block_parent_hash: ByteArray<32>,
        block_timestamp: u64,
        network_id: NetworkId,
    ) -> Result<TransactionResult, Error> {
        let ledger_transaction =
            deserialize::<LedgerTransaction, _>(&mut transaction.as_ref(), network_id.into())
                .map_err(|error| Error::Io("cannot deserialize ledger transaction", error))?;

        // Apply transaction to ledger state.
        let cx = TransactionContext {
            ref_state: self.0.0.clone(),
            block_context: BlockContext {
                tblock: timestamp(block_timestamp),
                tblock_err: 30,
                parent_block_hash: HashOutput(block_parent_hash.0),
            },
            whitelist: None,
        };
        let (state, transaction_result) = self.apply(&ledger_transaction, &cx);
        *self = LedgerState(state.into());

        let transaction_result = match transaction_result {
            LedgerTransactionResult::Success => TransactionResult::Success,

            LedgerTransactionResult::PartialSuccess(segments) => {
                let segments = segments
                    .into_iter()
                    .map(|(id, result)| (id, result.is_ok()))
                    .collect::<Vec<_>>();
                TransactionResult::PartialSuccess(segments)
            }

            LedgerTransactionResult::Failure(_) => TransactionResult::Failure,
        };

        Ok(transaction_result)
    }

    #[trace]
    fn apply_transaction_mut(
        &mut self,
        transaction: &mut Transaction,
        block_parent_hash: ByteArray<32>,
        block_timestamp: u64,
        network_id: NetworkId,
    ) -> Result<(), Error> {
        let start_index = self.zswap.first_free;
        let mut end_index = self.zswap.first_free;

        let transaction_result = self.apply_transaction(
            &transaction.raw,
            block_parent_hash,
            block_timestamp,
            network_id,
        )?;
        let zswap = &self.zswap;

        // Handle genesis block: extract any pre-funded unshielded UTXOs.
        // Check if this is genesis block by examining parent hash.
        if block_parent_hash == ByteArray([0; 32]) {
            let utxos = extract_utxos_from_ledger_state(self);
            transaction.created_unshielded_utxos.extend(utxos);
        }

        // Update end_index and contract zswap state if necessary.
        if zswap.first_free > start_index {
            update_contract_zswap_state(zswap, transaction, network_id)?;
            end_index = zswap.first_free - 1;
        }

        // Update transaction.
        transaction.transaction_result = transaction_result;
        transaction.merkle_tree_root = extract_merkle_tree_root(zswap, network_id)?;
        transaction.start_index = start_index;
        transaction.end_index = end_index;

        Ok(())
    }

    fn post_apply_transactions(&mut self, block_timestamp: u64) {
        let timestamp = timestamp(block_timestamp);
        *self = LedgerState(self.post_block_update(timestamp).into());
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("cannot apply transaction")]
    ApplyTransaction(#[from] midnight_zswap::error::TransactionInvalid),

    #[error("{0}")]
    Io(&'static str, #[source] io::Error),
}

fn update_contract_zswap_state(
    state: &LedgerZswapState<DefaultDB>,
    transaction: &mut Transaction,
    network_id: NetworkId,
) -> Result<(), Error> {
    for contract_action in transaction.contract_actions.iter_mut() {
        let zswap_state =
            extract_contract_zswap_state(state, &contract_action.address, network_id)?;
        contract_action.zswap_state = zswap_state;
    }

    Ok(())
}

fn extract_contract_zswap_state(
    state: &LedgerZswapState<DefaultDB>,
    address: &ContractAddress,
    network_id: NetworkId,
) -> Result<RawLedgerState, Error> {
    let address = deserialize::<midnight_coin_structure::contract::ContractAddress, _>(
        &mut address.as_ref(),
        network_id.into(),
    )
    .map_err(|error| Error::Io("cannot deserialize contract address", error))?;

    let mut contract_zswap_state = LedgerZswapState::new();
    contract_zswap_state.coin_coms = state.filter(&[address]);
    let state = contract_zswap_state
        .serialize(network_id)
        .map_err(|error| Error::Io("cannot serialize Zswap state", error))?;

    Ok(state.into())
}

fn extract_merkle_tree_root(
    state: &LedgerZswapState<DefaultDB>,
    network_id: NetworkId,
) -> Result<MerkleTreeRoot, Error> {
    let root = state
        .coin_coms
        .root()
        .serialize(network_id)
        .map_err(|error| Error::Io("cannot serialize merkle tree root", error))?;

    Ok(root.into())
}

/// Extract UTXOs from the midnight-ledger state and convert them to indexer format.
fn extract_utxos_from_ledger_state(ledger_state: &LedgerState) -> Vec<UnshieldedUtxo> {
    let midnight_ledger_state = &ledger_state.0.0;
    let utxo_state = &midnight_ledger_state.utxo;

    utxo_state
        .utxos
        .iter()
        .map(|utxo| UnshieldedUtxo {
            creating_transaction_id: 0,
            output_index: utxo.output_no,
            owner_address: utxo.owner.0.0.as_slice().into(),
            token_type: utxo.type_.0.0.into(),
            intent_hash: utxo.intent_hash.0.0.into(),
            value: utxo.value,
        })
        .collect()
}

/// Converts a block timestamp which is in milliseconds to a ledger timestamp.
fn timestamp(block_timestamp: u64) -> Timestamp {
    Timestamp::from_secs(block_timestamp / 1000)
}
