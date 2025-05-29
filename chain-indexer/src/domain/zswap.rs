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

use crate::domain::{BlockHash, Transaction};
use derive_more::derive::{Deref, From};
use fastrace::trace;
use indexer_common::{
    domain::{ApplyStage, ContractAddress, MerkleTreeRoot, NetworkId, RawLedgerState},
    serialize::SerializableExt,
};
use log::debug;
use midnight_ledger::{
    coin_structure::contract::Address,
    onchain_runtime::context::BlockContext,
    semantics::{TransactionContext, TransactionResult},
    serialize::deserialize,
    storage::DefaultDB,
    structure::Proof,
    zswap::ledger::State as LedgerZswapState,
};
use std::io;
use thiserror::Error;

type LedgerTransaction = midnight_ledger::structure::Transaction<Proof, DefaultDB>;

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

    /// Apply the given transactions to this ledger state.
    #[trace]
    pub fn apply_transactions<'a>(
        &mut self,
        transactions: impl Iterator<Item = (&'a Transaction, BlockHash)>,
        network_id: NetworkId,
    ) -> Result<(), Error> {
        for (transaction, block_hash) in transactions {
            self.apply_transaction(transaction, block_hash, network_id)?;
        }

        Ok(())
    }

    /// Apply the given transactions to this ledger state and also update relevant transaction data
    /// like start_index and end_index.
    #[trace]
    pub fn apply_transactions_mut<'a>(
        &mut self,
        transactions: impl Iterator<Item = (&'a mut Transaction, BlockHash)>,
        network_id: NetworkId,
    ) -> Result<(), Error> {
        for (transaction, block_hash) in transactions {
            self.apply_transaction_mut(transaction, block_hash, network_id)?;
        }

        Ok(())
    }

    /// The last used index.
    pub fn end_index(&self) -> Option<u64> {
        (self.zswap.first_free != 0).then(|| self.zswap.first_free - 1)
    }

    #[trace]
    fn apply_transaction(
        &mut self,
        transaction: &Transaction,
        block_hash: BlockHash,
        network_id: NetworkId,
    ) -> Result<TransactionResult<DefaultDB>, Error> {
        debug!(hash:% = transaction.hash; "applying transaction");

        let ledger_transaction =
            deserialize::<LedgerTransaction, _>(&mut transaction.raw.as_ref(), network_id.into())
                .map_err(|error| Error::Io("cannot deserialize ledger transaction", error))?;

        // Apply transaction to ledger state.
        let cx = TransactionContext {
            ref_state: self.0.0.clone(),
            block_context: BlockContext {
                seconds_since_epoch: 42,     // This value seems not important.
                seconds_since_epoch_err: 30, // This value seems not important.
                block_hash: block_hash.into(),
            },
            whitelist: None,
        };
        let (state, transaction_result) = self.apply(&ledger_transaction, &cx);
        *self = LedgerState(state.into());

        Ok(transaction_result)
    }

    #[trace]
    fn apply_transaction_mut(
        &mut self,
        transaction: &mut Transaction,
        block_hash: BlockHash,
        network_id: NetworkId,
    ) -> Result<(), Error> {
        let start_index = self.zswap.first_free;
        let mut end_index = self.zswap.first_free;

        let transaction_result = self.apply_transaction(transaction, block_hash, network_id)?;
        let zswap = &self.zswap;

        // Update end_index and contract zswap state if necessary.
        if zswap.first_free > start_index {
            update_contract_zswap_state(zswap, transaction, network_id)?;
            end_index = zswap.first_free - 1;
        }

        // Update transaction.
        transaction.apply_stage = match transaction_result {
            TransactionResult::Success => ApplyStage::Success,
            TransactionResult::PartialSuccess(_) => ApplyStage::PartialSuccess,
            TransactionResult::Failure(_) => ApplyStage::Failure,
        };
        transaction.merkle_tree_root = extract_merkle_tree_root(zswap, network_id)?;
        transaction.start_index = start_index;
        transaction.end_index = end_index;

        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("cannot apply transaction")]
    ApplyTransaction(#[from] midnight_ledger::zswap::error::TransactionInvalid),

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
    let address = deserialize::<Address, _>(&mut address.as_ref(), network_id.into())
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
