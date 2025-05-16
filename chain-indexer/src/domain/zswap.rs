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
use indexer_common::{
    domain::{ApplyStage, ContractAddress, MerkleTreeRoot, NetworkId, RawZswapState},
    serialize::SerializableExt,
};
use log::debug;
use midnight_ledger::{
    coin_structure::contract::Address,
    serialize::deserialize,
    storage::DefaultDB,
    structure::{Proof, StandardTransaction},
    zswap::ledger::State as LedgerZswapState,
};
use std::io;
use thiserror::Error;

type LedgerTransaction = midnight_ledger::structure::Transaction<Proof, DefaultDB>;

/// Wrapper around ZswapState from indexer_common.
#[derive(Debug, Clone, Default, From, Deref)]
pub struct ZswapState(pub indexer_common::domain::ZswapState);

impl ZswapState {
    /// Serialize this [ZswapState] using the given [NetworkId].
    #[trace]
    pub fn serialize(&self, network_id: NetworkId) -> Result<RawZswapState, io::Error> {
        let bytes = (**self).serialize(network_id)?;
        Ok(bytes.into())
    }

    #[trace]
    pub fn apply_transactions(
        &mut self,
        transactions: &mut [Transaction],
        network_id: NetworkId,
    ) -> Result<(), Error> {
        for transaction in transactions.iter_mut() {
            self.apply_transaction(transaction, network_id)?;
        }

        Ok(())
    }

    pub fn end_index(&self) -> Option<u64> {
        (self.first_free != 0).then(|| self.first_free - 1)
    }

    #[trace]
    fn apply_transaction(
        &mut self,
        transaction: &mut Transaction,
        network_id: NetworkId,
    ) -> Result<(), Error> {
        debug!(hash:% = transaction.hash; "applying transaction");

        let start_index = self.first_free;
        let mut end_index = self.first_free;

        // For Failure the state is not changed.
        if transaction.apply_stage != ApplyStage::Failure {
            let ledger_transaction = deserialize::<LedgerTransaction, _>(
                &mut transaction.raw.as_ref(),
                network_id.into(),
            )
            .map_err(|error| Error::Io("cannot deserialize ledger transaction", error))?;

            if let LedgerTransaction::Standard(StandardTransaction {
                guaranteed_coins,
                fallible_coins,
                ..
            }) = ledger_transaction
            {
                // Guaranteed coins are applied for both Success and PartialSuccess.
                let (mut state, _) = self.try_apply(&guaranteed_coins, None)?;

                // Fallible coins are only applied for Success.
                if transaction.apply_stage == ApplyStage::Success {
                    if let Some(fallible_coins) = &fallible_coins {
                        (state, _) = state.try_apply(fallible_coins, None)?;
                    }
                }

                if state.first_free > start_index {
                    update_contract_zswap_state(&state, transaction, network_id)?;
                    end_index = state.first_free - 1;
                }

                *self = ZswapState(state.into());
            }
        }

        transaction.merkle_tree_root = extract_merkle_tree_root(self, network_id)?;
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
    for action in transaction.contract_actions.iter_mut() {
        let zswap_state = extract_contract_zswap_state(state, &action.address, network_id)?;
        action.zswap_state = zswap_state;
    }

    Ok(())
}

fn extract_contract_zswap_state(
    state: &LedgerZswapState<DefaultDB>,
    address: &ContractAddress,
    network_id: NetworkId,
) -> Result<RawZswapState, Error> {
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
