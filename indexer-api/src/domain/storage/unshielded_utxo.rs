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
    UnshieldedUtxo,
    storage::{
        NoopStorage, block::BlockStorage, contract_action::ContractActionStorage,
        transaction::TransactionStorage, wallet::WalletStorage,
    },
};
use indexer_common::domain::{BlockHash, Identifier, TransactionHash, UnshieldedAddress};
use std::fmt::Debug;

/// Storage abstraction.
#[trait_variant::make(Send)]
pub trait UnshieldedUtxoStorage
where
    Self: BlockStorage
        + ContractActionStorage
        + TransactionStorage
        + WalletStorage
        + Debug
        + Clone
        + Send
        + Sync
        + 'static,
{
    /// Get unshielded UTXOs based on filter criteria.
    /// This consolidated method replaces multiple specific UTXO query methods.
    async fn get_unshielded_utxos(
        &self,
        address: Option<&UnshieldedAddress>,
        filter: UnshieldedUtxoFilter<'_>,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error>;
}

/// Filter criteria for unshielded UTXOs queries.
#[derive(Debug, Clone)]
pub enum UnshieldedUtxoFilter<'a> {
    /// All UTXOs (no filter)
    All,
    /// UTXOs created by a specific transaction
    CreatedByTx(u64),
    /// UTXOs spent by a specific transaction
    SpentByTx(u64),
    /// UTXOs created in a specific transaction for a specific address
    CreatedInTxForAddress(u64),
    /// UTXOs spent in a specific transaction for a specific address
    SpentInTxForAddress(u64),
    /// UTXOs created/spent in blocks with height >= the given value
    FromHeight(u32),
    /// UTXOs created/spent in a specific block
    FromBlockHash(&'a BlockHash),
    /// UTXOs created/spent in a transaction with given hash
    FromTxHash(&'a TransactionHash),
    /// UTXOs created/spent in a transaction with given identifier
    FromTxIdentifier(&'a Identifier),
}

#[allow(unused_variables)]
impl UnshieldedUtxoStorage for NoopStorage {
    #[cfg_attr(coverage, coverage(off))]
    async fn get_unshielded_utxos(
        &self,
        address: Option<&UnshieldedAddress>,
        filter: UnshieldedUtxoFilter<'_>,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        unimplemented!()
    }
}
